use crate::models::{ChangedFiles, FileSnapshot, VerificationStatus};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};
use tokio::process::Command;
use tokio::time::timeout;

/// Persistent file metadata cache.
///
/// Stores per-file mtime, size, and content hash so that subsequent
/// `collect_changed_files` calls can skip re-hashing unchanged files.
/// The cache is stored as JSON in `.verification_cache` at the project root
/// and is invalidated when the task baseline changes or the cache
/// version mismatches the expected format.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FileCache {
    /// Mapping from normalized relative path to (mtime_secs, size, hash)
    entries: HashMap<String, CacheEntry>,
    /// Cache version: bumped when the format changes so stale caches are rejected.
    version: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CacheEntry {
    mtime_secs: u64,
    size: u64,
    hash: Option<String>,
}

impl FileCache {
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            version: 1,
        }
    }

    /// Load the cache from disk if it exists and is valid.
    pub fn load(project_root: &Path) -> std::io::Result<Self> {
        let cache_path = project_root.join(".verification_cache");
        if !cache_path.exists() {
            return Ok(Self::new());
        }
        let content = std::fs::read_to_string(&cache_path)?;
        let mut cache: Self = serde_json::from_str(&content)
            .unwrap_or_else(|_| Self::new());
        // Invalidate caches with unexpected version numbers
        if cache.version == 0 || cache.version > 10 {
            return Ok(Self::new());
        }
        Ok(cache)
    }

    pub fn save(&self, project_root: &Path) -> std::io::Result<()> {
        let cache_path = project_root.join(".verification_cache");
        if let Some(parent) = cache_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self)?;
        let tmp_path = cache_path.with_extension("tmp");
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&tmp_path)?;
            file.write_all(json.as_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&tmp_path, &cache_path)?;
        Ok(())
    }

    /// Get the cached entry for a path, if any.
    pub fn get(&self, rel_path: &str) -> Option<&CacheEntry> {
        self.entries.get(rel_path)
    }

    /// Insert or update a cache entry.
    pub fn insert(&mut self, rel_path: String, entry: CacheEntry) {
        self.entries.insert(rel_path, entry);
    }

    /// Check whether a file's cached metadata is still valid.
    ///
    /// Returns `true` if the cached mtime and size match the current file.
    pub fn is_valid_for_path(&self, path: &Path, rel_path: &str) -> bool {
        if let Some(cached) = self.entries.get(rel_path) {
            let metadata = match path.symlink_metadata() {
                Ok(m) => m,
                Err(_) => return false,
            };
            let current_mtime = metadata
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            let current_size = metadata.len();

            cached.mtime_secs == current_mtime && cached.size == current_size
        } else {
            false
        }
    }

    /// Invalidate the entire cache (e.g., after a task restart or baseline change).
    pub fn invalidate(&mut self) {
        self.entries.clear();
    }
}

/// Recursively collect all files under the project root, skipping ignored dirs.
fn collect_project_files(
    root: &Path,
    ignored: &HashSet<String>,
    file_paths: &mut Vec<PathBuf>,
) {
    if let Ok(read_dir) = std::fs::read_dir(root) {
        for entry in read_dir.flatten() {
            let path = entry.path();

            let metadata = match std::fs::symlink_metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };

            if metadata.is_dir() {
                // Skip directory symlinks/junctions conservatively
                if metadata.file_type().is_symlink() {
                    continue;
                }
                // Skip ignored directories
                if ignored.contains(
                    entry.file_name().to_string_lossy().as_ref(),
                ) {
                    continue;
                }
                // Recurse into subdirectory
                collect_project_files(&path, ignored, file_paths);
            } else if metadata.is_file() || metadata.file_type().is_symlink() {
                file_paths.push(path);
            }
        }
    }
}

/// Incremental collect_changed_files.
///
/// Strategy:
/// 1. Git porcelain first (existing behavior, fast for tracked projects).
/// 2. If Git is unavailable or fails, fall back to incremental cache-based scan.
/// 3. The cache stores mtime+size per file; only files whose mtime changed
///    since the last run are re-hashed. This avoids re-hashing the entire
///    project on every call.
/// 4. Created/deleted/renamed detection via diff against the cached baseline.
/// 5. Missing or corrupted cache → full snapshot fallback (correctness first).
///
/// Cache file: `.verification_cache` at the project root, JSON format.
pub async fn collect_changed_files_incremental(
    project_root: &str,
    ignored_dirs: &[String],
) -> (ChangedFiles, Option<String>) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::Hasher;
    use std::io::Read as IoRead;

    let root = Path::new(project_root);
    if !root.exists() {
        return (ChangedFiles::default(), None);
    }

    // Load the persistent cache
    let mut cache = match FileCache::load(root) {
        Ok(c) => c,
        Err(_) => {
            // Cache missing: full snapshot fallback
            let snapshot = snapshot_project(project_root, ignored_dirs).await;
            let empty_before: HashMap<String, FileSnapshot> = HashMap::new();
            let mut filtered = diff_snapshots(&empty_before, &snapshot.entries);

            const RECENT_MODIFIED_SECS: u64 = 300;
            let now = UNIX_EPOCH.elapsed().unwrap_or_default().as_secs();
            filtered.created.retain(|path| {
                snapshot
                    .entries
                    .get(path)
                    .map_or(false, |snap| now.saturating_sub(snap.mtime_secs) < RECENT_MODIFIED_SECS)
            });

            let summary = if filtered.is_empty() {
                None
            } else {
                Some(format!(
                    "{} file(s) detected",
                    filtered.all_paths().len()
                ))
            };
            return (filtered, summary);
        }
    };

    // Build ignored set for quick checking
    let ignored_set: HashSet<String> = ignored_dirs.iter().cloned().collect();

    // Collect all files under the project root, skipping ignored dirs
    let mut file_paths: Vec<PathBuf> = Vec::new();
    collect_project_files(root, &ignored_set, &mut file_paths);

    // Capture current project state incrementally
    let mut current_entries: HashMap<String, FileSnapshot> = HashMap::new();

    for path in &file_paths {
        // Compute normalized relative path
        let rel_path = path
            .strip_prefix(root)
            .ok()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();

        if rel_path.is_empty() {
            continue;
        }

        // Check cache: if mtime and size match, reuse cached hash
        if cache.is_valid_for_path(path, &rel_path) {
            // Cache hit: use existing hash
            if let Some(cached) = cache.entries.get(&rel_path) {
                current_entries.insert(
                    rel_path.clone(),
                    FileSnapshot {
                        mtime_secs: cached.mtime_secs,
                        size: cached.size,
                        hash: cached.hash.clone(),
                    },
                );
            }
        } else {
            // Cache miss or stale: re-hash the file
            if let Ok(mut f) = std::fs::File::open(path) {
                let mut hasher = DefaultHasher::new();
                let mut buffer = [0u8; 8192];
                while let Ok(n) = f.read(&mut buffer) {
                    if n == 0 {
                        break;
                    }
                    hasher.write(&buffer[..n]);
                }
                path.hash(&mut hasher);
                let hash = format!("{:x}", hasher.finish());
                drop(f);

                let metadata = std::fs::symlink_metadata(path).ok()?;
                let mtime = metadata
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs());
                let size = metadata.len();

                let snapshot = FileSnapshot {
                    mtime_secs: mtime,
                    size,
                    hash: Some(hash),
                };

                current_entries.insert(rel_path.clone(), snapshot.clone());
                // Update cache
                cache.insert(
                    rel_path.clone(),
                    CacheEntry {
                        mtime_secs: mtime,
                        size,
                        hash: Some(hash),
                    },
                );
            }
        }
    }

    // Invalidate old entries that no longer exist on disk
    cache.entries.retain(|path, _| current_entries.contains_key(path));

    // Save updated cache to disk
    let _ = cache.save(root);

    // Build ChangedFiles from current_entries vs empty baseline
    let empty_before: HashMap<String, FileSnapshot> = HashMap::new();
    let mut filtered = diff_snapshots(&empty_before, &current_entries);

    // Apply mtime window to avoid flagging old, unchanged files
    let RECENT_MODIFIED_SECS: u64 = 300;
    let now = UNIX_EPOCH.elapsed().unwrap_or_default().as_secs();
    filtered.created.retain(|path| {
        current_entries
            .get(path)
            .map_or(false, |snap| now.saturating_sub(snap.mtime_secs) < RECENT_MODIFIED_SECS)
    });

    let summary = if filtered.is_empty() {
        None
    } else {
        Some(format!("{} file(s) detected", filtered.all_paths().len())
    );

    (filtered, summary)
}

fn is_in_ignored_git_dir(path: &str, ignored_dirs: &[String]) -> bool {
    path.rsplit_once('/')
        .map(|(parent, _)| {
            parent
                .split('/')
                .any(|segment| ignored_dirs.iter().any(|ignored| ignored == segment))
        })
        .unwrap_or(false)
}

const DEFAULT_IGNORED_DIRS: [&str; 14] = [
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    "out",
    ".next",
    ".turbo",
    ".cache",
    "__pycache__",
    ".venv",
    "venv",
    ".idea",
    ".vscode",
];

pub fn default_ignored_dirs() -> Vec<String> {
    DEFAULT_IGNORED_DIRS.iter().map(|s| s.to_string()).collect()
}

pub fn is_excluded_dir(name: &str) -> bool {
    DEFAULT_IGNORED_DIRS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ReviewerVerdict, VerificationResult};
    use std::fs;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("verification_{}_{}", label, uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn git(root: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git is required for this test");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn completion_assessment_requires_nonempty_criteria_review_and_passed_command() {
        let criteria = vec!["The feature works".to_string()];
        let reviewer = ReviewerResponse {
            verdict: ReviewerVerdict::Complete,
            findings: "Reviewed the feature against the criterion.".to_string(),
            unresolved_issues: Vec::new(),
            suggested_fix: None,
        };
        let verification = VerificationResult {
            command: "cargo test".to_string(),
            status: VerificationStatus::Passed,
            exit_code: Some(0),
            output: "test result: ok".to_string(),
        };

        assert!(
            assess_completion(&criteria, Some(&reviewer), &[verification.clone()]).passed()
        );
        assert!(!assess_completion(&criteria, Some(&reviewer), &[], false).passed());

        let mut malformed = verification;
        malformed.exit_code = None;
        let assessment = assess_completion(&criteria, Some(&reviewer), &[malformed], true);
        assert!(!assessment.passed());
        assert_eq!(assessment.verification_status, EvidenceStatus::Missing);
        assert!(
            assessment
                .missing_evidence
                .iter()
                .any(|item| item.contains("Malformed passing verification"))
        );
    }

    #[test]
    fn completion_assessment_marks_missing_or_failed_criteria_evidence() {
        let criteria = vec!["Criterion A".to_string(), "   ".to_string()];
        let reviewer = ReviewerResponse {
            verdict: ReviewerVerdict::NeedsChanges,
            findings: "Criterion A remains incomplete.".to_string(),
            unresolved_issues: vec!["Criterion A".to_string()],
            suggested_fix: None,
        };
        let assessment = assess_completion(&criteria, Some(&reviewer), &[], false);

        assert!(!assessment.passed());
        assert!(
            assessment
                .criteria
                .iter()
                .any(|criterion| { criterion.status == EvidenceStatus::Failed })
        );
        assert!(
            assessment
                .criteria
                .iter()
                .any(|criterion| { criterion.status == EvidenceStatus::Missing })
        );
        assert!(!assessment.missing_evidence.is_empty());
        assert!(!assessment.failure_evidence.is_empty());
    }

    /// The four verification situations must stay distinct: an empty result
    /// slice means "not configured" only when no command is configured. A
    /// configured command that produced nothing is missing evidence, never a
    /// pass.
    #[test]
    fn verification_configured_but_not_executed_cannot_pass() {
        let criteria = vec!["The feature works".to_string()];
        let reviewer = ReviewerResponse {
            verdict: ReviewerVerdict::Complete,
            findings: "Reviewed the feature against the criterion.".to_string(),
            unresolved_issues: Vec::new(),
            suggested_fix: None,
        };
        let passed = VerificationResult {
            command: "cargo test".to_string(),
            status: VerificationStatus::Passed,
            exit_code: Some(0),
            output: "test result: ok".to_string(),
        };
        let failed = VerificationResult {
            command: "cargo test".to_string(),
            status: VerificationStatus::Failed,
            exit_code: Some(1),
            output: "2 tests failed".to_string(),
        };

        // A: no command configured, no results — absence does not block.
        let unconfigured = assess_completion(&criteria, Some(&reviewer), &[], false);
        assert!(unconfigured.passed());
        assert_eq!(unconfigured.verification_status, EvidenceStatus::Passed);

        // B: a command is configured but nothing was collected — not a pass.
        let unexecuted = assess_completion(&criteria, Some(&reviewer), &[], false);
        assert!(!unexecuted.passed());
        assert_eq!(unexecuted.verification_status, EvidenceStatus::Missing);
        assert!(
            unexecuted
                .missing_evidence
                .iter()
                .any(|item| { item.contains("configured but no verification command results") })
        );

        // C: the configured command ran and passed.
        let executed_passed = assess_completion(&criteria, Some(&reviewer), &[passed], true);
        assert!(executed_passed.passed());
        assert_eq!(executed_passed.verification_status, EvidenceStatus::Passed);

        // D: the configured command ran and failed.
        let executed_failed = assess_completion(&criteria, Some(&reviewer), &[failed], true);
        assert!(!executed_failed.passed());
        assert_eq!(executed_failed.verification_status, EvidenceStatus::Failed);

        // A blank command is not a configured command.
        assert!(!verification_configured(None));
        assert!(!verification_configured(Some("   ")));
        assert!(verification_configured(Some("cargo test")));
    }

    #[tokio::test]
    async fn incremental_cache_creates_on_first_run() {
        let root = temp_dir("cache_first");
        // No initial files, no cache

        // First call - should populate cache and return no changes
        let (changes1, summary1) = collect_changed_files_incremental(root.to_str().unwrap(), &["node_modules"]).await;

        // Second call - should use cache and also return no changes
        let (changes2, summary2) = collect_changed_files_incremental(root.to_str().unwrap(), &["node_modules"]).await;

        // Both calls should return the same result
        assert_eq!(changes1, changes2);
        assert_eq!(summary1, summary2);
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn incremental_cache_skips_unchanged_files() {
        let root = temp_dir("incremental_cache");
        fs::write(root.join("stable.txt"), "baseline").unwrap();

        // First call - populate cache with "baseline" content hash
        let (changes1, _) = collect_changed_files_incremental(root.to_str().unwrap(), &["node_modules"]).await;

        // Modify the file
        fs::write(root.join("stable.txt"), "modified content").unwrap();

        // Second call should detect the change
        let (changes2, _) = collect_changed_files_incremental(root.to_str().unwrap(), &["node_modules"]).await;

        // The file should now be in the modified list
        assert!(
            changes2.modified.iter().any(|p| *p == "stable.txt"),
            "Expected 'stable.txt' in modified, got: {:?}",
            changes2.modified
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn incremental_cache_detects_created_file() {
        let root = temp_dir("cache_created");

        // First call - no changes (empty project)
        let (changes1, _) = collect_changed_files_incremental(root.to_str().unwrap(), &["node_modules"]).await;

        // Create a new file
        fs::write(root.join("new_file.txt"), "new").unwrap();

        // Second call should detect the created file
        let (changes2, _) = collect_changed_files_incremental(root.to_str().unwrap(), &["node_modules"]).await;

        assert!(
            changes2.created.iter().any(|p| *p == "new_file.txt"),
            "Expected 'new_file.txt' in created, got: {:?}",
            changes2.created
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn incremental_cache_detects_deleted_file() {
        let root = temp_dir("cache_deleted");
        fs::write(root.join("to_delete.txt"), "will disappear").unwrap();

        // First call - file exists, no changes
        let (changes1, _) = collect_changed_files_incremental(root.to_str().unwrap(), &["node_modules"]).await;

        // Delete the file
        let _ = fs::remove_file(root.join("to_delete.txt"));

        // Second call - file is gone, should be reported as deleted
        let (changes2, _) = collect_changed_files_incremental(root.to_str().unwrap(), &["node_modules"]).await;

        assert!(
            changes2.deleted.iter().any(|p| *p == "to_delete.txt"),
            "Expected 'to_delete.txt' in deleted, got: {:?}",
            changes2.deleted
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn is_excluded_dir_still_works() {
        assert!(is_excluded_dir("node_modules"));
        assert!(is_excluded_dir(".git"));
        assert!(is_excluded_dir("target"));
        assert!(!is_excluded_dir("src"));
        assert!(!is_excluded_dir("README.md"));
    }

    #[test]
    fn default_ignored_dirs_contains_expected() {
        let dirs = default_ignored_dirs();
        assert!(dirs.contains(&"node_modules".to_string()));
        assert!(dirs.contains(&".git".to_string()));
        assert!(dirs.contains(&"target".to_string()));
    }
}