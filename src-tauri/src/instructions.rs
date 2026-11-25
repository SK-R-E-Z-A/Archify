use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const MAX_INSTRUCTION_CHARS: usize = 64 * 1024;
pub const MAX_APPLIED_INSTRUCTION_CHARS: usize = 24 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstructionScope {
    Global,
    Project,
    /// Scoped to a single coding agent/provider (or a small set of agents).
    /// Requires `agent_targets` to be non-empty.
    Agent,
    /// Scoped to a single task. Requires `task_id` to match.
    Task,
}

impl Default for InstructionScope {
    fn default() -> Self {
        Self::Global
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InstructionFormat {
    Markdown,
    PlainText,
    /// A discovered file in the project (read-only reference; not managed/deletable here).
    DiscoveredFile,
}

impl Default for InstructionFormat {
    fn default() -> Self {
        Self::Markdown
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstructionRecord {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub content: String,
    #[serde(default)]
    pub scope: InstructionScope,
    /// Canonical project root for project-scoped instructions.
    #[serde(default)]
    pub project_root: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
    /// Task identifier for task-scoped instructions.
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub format: InstructionFormat,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Empty means "all agents". Values may include: planner, executor, reviewer, claude, codex, aider, gemini, opencode, cursor-agent.
    #[serde(default)]
    pub agent_targets: Vec<String>,
    /// Lower sorts earlier; deterministic application order.
    #[serde(default)]
    pub priority: i32,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    /// For discovered files only: absolute path to the source file.
    #[serde(default)]
    pub source_path: Option<String>,
    /// Managed records are editable/deletable; discovered records are read-only references.
    #[serde(default = "default_true")]
    pub managed: bool,
    #[serde(default)]
    pub read_error: Option<String>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstructionStoreFile {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub instructions: Vec<InstructionRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstructionUpsertInput {
    pub id: Option<String>,
    pub title: Option<String>,
    pub content: Option<String>,
    pub scope: Option<InstructionScope>,
    pub project_root: Option<String>,
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub format: Option<InstructionFormat>,
    pub enabled: Option<bool>,
    pub agent_targets: Option<Vec<String>>,
    pub priority: Option<i32>,
    pub tags: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedInstructionRef {
    pub id: String,
    pub title: String,
    pub scope: InstructionScope,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedInstructionBlock {
    pub refs: Vec<AppliedInstructionRef>,
    pub text: String,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstructionValidationIssue {
    pub id: Option<String>,
    pub level: String,
    pub message: String,
}

pub fn default_instruction_store_path(base_dir: &Path) -> PathBuf {
    base_dir.join("instructions.json")
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| format!("Failed to serialize {}: {}", path.display(), e))?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, json).map_err(|e| format!("Failed to write {}: {}", tmp.display(), e))?;
    match fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(first) => {
            let _ = fs::remove_file(path);
            fs::rename(&tmp, path).map_err(|second| {
                let _ = fs::remove_file(&tmp);
                format!(
                    "Failed to replace {}: {} (original: {})",
                    path.display(),
                    second,
                    first
                )
            })
        }
    }
}

/// Move (or copy) an unparseable store out of the way so the next save cannot
/// silently destroy whatever the user had. Returns the backup path when one was
/// created.
fn quarantine_malformed_store(path: &Path) -> Option<PathBuf> {
    let stamp = Utc::now().format("%Y%m%dT%H%M%S%.3fZ").to_string();
    let name = path.file_name()?.to_string_lossy().to_string();
    let backup = path.with_file_name(format!("{name}.corrupt-{stamp}"));
    if fs::rename(path, &backup).is_ok() {
        return Some(backup);
    }
    match fs::copy(path, &backup) {
        Ok(_) => Some(backup),
        Err(_) => None,
    }
}

pub fn load_instruction_store(path: &Path) -> InstructionStoreFile {
    if !path.exists() {
        return InstructionStoreFile {
            version: 1,
            instructions: Vec::new(),
        };
    }
    match fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<InstructionStoreFile>(&content) {
            Ok(mut store) => {
                if store.version == 0 {
                    store.version = 1;
                }
                store.instructions.retain(|r| !r.id.trim().is_empty());
                store
            }
            Err(e) => {
                match quarantine_malformed_store(path) {
                    Some(backup) => eprintln!(
                        "[instructions] malformed store {}: {} (preserved as {})",
                        path.display(),
                        e,
                        backup.display()
                    ),
                    None => eprintln!("[instructions] malformed store {}: {}", path.display(), e),
                }
                InstructionStoreFile {
                    version: 1,
                    instructions: Vec::new(),
                }
            }
        },
        Err(e) => {
            eprintln!("[instructions] unreadable store {}: {}", path.display(), e);
            InstructionStoreFile {
                version: 1,
                instructions: Vec::new(),
            }
        }
    }
}

pub fn save_instruction_store(path: &Path, store: &InstructionStoreFile) -> Result<(), String> {
    write_json_atomic(path, store)
}

fn sanitize_text(label: &str, value: &str, max: usize) -> Result<String, String> {
    let trimmed = value.trim().to_string();
    if trimmed.chars().count() > max {
        return Err(format!("{} exceeds {} characters", label, max));
    }
    if trimmed.chars().any(|c| c == '\0') {
        return Err(format!("{} contains invalid control characters", label));
    }
    Ok(trimmed)
}

fn normalize_targets(targets: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for t in targets {
        let norm = t.trim().to_ascii_lowercase();
        if norm.is_empty() || norm.len() > 64 {
            continue;
        }
        if seen.insert(norm.clone()) {
            out.push(norm);
        }
    }
    out
}

fn canonical_project_root(root: Option<String>) -> Result<Option<String>, String> {
    let Some(r) = root else {
        return Ok(None);
    };
    let trimmed = r.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let path = Path::new(trimmed);
    let canon = dunce::canonicalize(path)
        .map_err(|e| format!("Invalid project root '{}': {}", trimmed, e))?;
    if !canon.is_dir() {
        return Err(format!("Project root is not a directory: {}", trimmed));
    }
    Ok(Some(canon.to_string_lossy().to_string()))
}

pub fn upsert_instruction(
    store: &mut InstructionStoreFile,
    input: InstructionUpsertInput,
) -> Result<InstructionRecord, String> {
    let now = Utc::now().to_rfc3339();
    if let Some(id) = input.id.as_ref().filter(|s| !s.trim().is_empty()) {
        let idx = store
            .instructions
            .iter()
            .position(|r| r.id == *id && r.managed)
            .ok_or_else(|| format!("Managed instruction '{}' not found", id))?;
        let mut rec = store.instructions[idx].clone();
        if let Some(t) = input.title {
            rec.title = sanitize_text("title", &t, 200)?;
        }
        if let Some(c) = input.content {
            rec.content = sanitize_text("content", &c, MAX_INSTRUCTION_CHARS)?;
        }
        if let Some(s) = input.scope {
            rec.scope = s;
        }
        match rec.scope.clone() {
            InstructionScope::Global => {
                // A global instruction must not retain project linkage; keeping
                // it would make later scope changes ambiguous.
                rec.project_root = None;
                rec.project_id = None;
                rec.task_id = None;
            }
            InstructionScope::Project => {
                // Only re-canonicalize when the caller supplies a root. A
                // content-only edit of a project whose directory has since
                // moved must not fail (the stored linkage is preserved).
                if let Some(root) = input.project_root {
                    rec.project_root = canonical_project_root(Some(root))?;
                }
                if rec.project_root.as_deref().unwrap_or("").trim().is_empty() {
                    return Err(
                        "project_root is required for project-scoped instructions".to_string()
                    );
                }
                if let Some(pid) = input.project_id {
                    rec.project_id = if pid.trim().is_empty() {
                        None
                    } else {
                        Some(pid.trim().to_string())
                    };
                }
                rec.task_id = None;
            }
            InstructionScope::Agent => {
                if let Some(targets) = input.agent_targets.clone() {
                    rec.agent_targets = normalize_targets(targets);
                }
                if rec.agent_targets.is_empty() {
                    return Err(
                        "agent_targets are required for agent-scoped instructions".to_string()
                    );
                }
                if let Some(root) = input.project_root.clone() {
                    rec.project_root = canonical_project_root(Some(root))?;
                }
                if let Some(pid) = input.project_id.clone() {
                    rec.project_id = if pid.trim().is_empty() {
                        None
                    } else {
                        Some(pid.trim().to_string())
                    };
                }
                rec.task_id = None;
            }
            InstructionScope::Task => {
                // Task-scoped instructions require a task_id.
                if let Some(tid) = input.task_id {
                    rec.task_id = if tid.trim().is_empty() {
                        None
                    } else {
                        Some(tid.trim().to_string())
                    };
                }
                if rec.task_id.as_deref().unwrap_or("").trim().is_empty() {
                    return Err("task_id is required for task-scoped instructions".to_string());
                }
                rec.project_root = None;
                rec.project_id = None;
            }
        }
        if let Some(f) = input.format {
            if f != InstructionFormat::DiscoveredFile {
                rec.format = f;
            }
        }
        if let Some(e) = input.enabled {
            rec.enabled = e;
        }
        if let Some(t) = input.agent_targets {
            rec.agent_targets = normalize_targets(t);
        }
        if let Some(p) = input.priority {
            rec.priority = p.clamp(-10_000, 10_000);
        }
        if let Some(t) = input.tags {
            rec.tags = normalize_targets(t);
        }
        validate_managed_record(&rec)?;
        rec.updated_at = now;
        store.instructions[idx] = rec.clone();
        return Ok(rec);
    }

    let title = sanitize_text("title", input.title.as_deref().unwrap_or(""), 200)?;
    if title.is_empty() {
        return Err("title is required".to_string());
    }
    let content = sanitize_text(
        "content",
        input.content.as_deref().unwrap_or(""),
        MAX_INSTRUCTION_CHARS,
    )?;
    let scope = input.scope.unwrap_or_default();
    let project_root = match scope {
        InstructionScope::Project => canonical_project_root(input.project_root)?,
        InstructionScope::Agent => {
            if input
                .project_root
                .as_ref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(true)
            {
                None
            } else {
                canonical_project_root(input.project_root)?
            }
        }
        _ => None,
    };
    if scope == InstructionScope::Project && project_root.is_none() {
        return Err("project_root is required for project-scoped instructions".to_string());
    }
    if scope == InstructionScope::Agent {
        let targets = normalize_targets(input.agent_targets.clone().unwrap_or_default());
        if targets.is_empty() {
            return Err("agent_targets are required for agent-scoped instructions".to_string());
        }
    }
    let task_id = if scope == InstructionScope::Task {
        let tid = input.task_id.as_deref().unwrap_or("").trim().to_string();
        if tid.is_empty() {
            return Err("task_id is required for task-scoped instructions".to_string());
        }
        Some(tid)
    } else {
        None
    };
    let rec = InstructionRecord {
        id: Uuid::new_v4().to_string(),
        title,
        content,
        scope,
        project_root,
        project_id: input.project_id.filter(|s| !s.trim().is_empty()),
        task_id,
        format: match input.format.unwrap_or_default() {
            InstructionFormat::DiscoveredFile => InstructionFormat::Markdown,
            f => f,
        },
        enabled: input.enabled.unwrap_or(true),
        agent_targets: normalize_targets(input.agent_targets.unwrap_or_default()),
        priority: input.priority.unwrap_or(0).clamp(-10_000, 10_000),
        tags: normalize_targets(input.tags.unwrap_or_default()),
        created_at: now.clone(),
        updated_at: now,
        source_path: None,
        managed: true,
        read_error: None,
    };
    validate_managed_record(&rec)?;
    store.instructions.push(rec.clone());
    Ok(rec)
}

fn validate_managed_record(rec: &InstructionRecord) -> Result<(), String> {
    if rec.title.trim().is_empty() {
        return Err("title cannot be empty".to_string());
    }
    if rec.scope == InstructionScope::Project
        && rec.project_root.as_deref().unwrap_or("").trim().is_empty()
    {
        return Err("project-scoped instructions require a project_root".to_string());
    }
    if rec.scope == InstructionScope::Task && rec.task_id.as_deref().unwrap_or("").trim().is_empty()
    {
        return Err("task-scoped instructions require a task_id".to_string());
    }
    if rec.scope == InstructionScope::Agent && rec.agent_targets.is_empty() {
        return Err("agent-scoped instructions require agent_targets".to_string());
    }
    Ok(())
}

pub fn delete_instruction(store: &mut InstructionStoreFile, id: &str) -> Result<(), String> {
    let Some(idx) = store.instructions.iter().position(|r| r.id == id) else {
        return Err(format!("Instruction '{}' not found", id));
    };
    if !store.instructions[idx].managed {
        return Err("Discovered instruction files cannot be deleted from the Instruction Manager; edit or remove the source file in the project.".to_string());
    }
    store.instructions.remove(idx);
    Ok(())
}

pub fn set_instruction_enabled(
    store: &mut InstructionStoreFile,
    id: &str,
    enabled: bool,
) -> Result<InstructionRecord, String> {
    let rec = store
        .instructions
        .iter_mut()
        .find(|r| r.id == id)
        .ok_or_else(|| format!("Instruction '{}' not found", id))?;
    if !rec.managed {
        return Err("Discovered instruction files are read-only references; enable/disable is supported for managed instructions.".to_string());
    }
    rec.enabled = enabled;
    rec.updated_at = Utc::now().to_rfc3339();
    Ok(rec.clone())
}

/// Returns a stable fingerprint (SHA-256 hex) of the project's ARCHIFY.md file,
/// if present and readable. Callers can compare fingerprints across tasks to
/// detect changes to project-level instructions without re-reading the full
/// content.
pub fn archify_md_fingerprint(project_root: &str) -> Option<String> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let path = Path::new(project_root).join("ARCHIFY.md");
    let content = fs::read_to_string(&path).ok()?;
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    Some(format!("{:016x}", hasher.finish()))
}

/// Ensure an ARCHIFY.md file exists in the project root. If it is missing, a
/// minimal default template is created and `true` is returned. If it already
/// exists (or cannot be created), returns `false`. Never overwrites existing
/// user content.
pub fn ensure_archify_md(project_root: &str) -> bool {
    let path = Path::new(project_root).join("ARCHIFY.md");
    if path.exists() {
        return false;
    }
    let template = "# ARCHIFY.md\n\n\
        Project-specific instructions for Archify. This file is automatically\n\
        discovered and applied to planning and execution. Edit freely; Archify\n\
        will never overwrite your changes.\n";
    fs::write(&path, template).is_ok()
}

pub fn discover_project_instruction_files(project_root: &str) -> Vec<InstructionRecord> {
    let root = Path::new(project_root);
    if !root.is_dir() {
        return Vec::new();
    }
    let candidates: Vec<&str> = vec![
        "ARCHIFY.md",
        "CLAUDE.md",
        "AGENTS.md",
        ".cursorrules",
        ".github/copilot-instructions.md",
        ".windsurfrules",
        ".clinerules",
    ];
    let now = Utc::now().to_rfc3339();
    let mut out = Vec::new();
    for rel in candidates {
        let path = root.join(rel);
        if !path.is_file() {
            continue;
        }
        let (content, read_error) = match fs::read_to_string(&path) {
            Ok(c) => {
                let truncated = if c.chars().count() > MAX_INSTRUCTION_CHARS {
                    format!(
                        "{}\n\n[truncated for preview]",
                        c.chars().take(MAX_INSTRUCTION_CHARS).collect::<String>()
                    )
                } else {
                    c
                };
                (truncated, None)
            }
            Err(e) => (String::new(), Some(format!("Could not read file: {}", e))),
        };
        let canon_root = dunce::canonicalize(root)
            .ok()
            .map(|p| p.to_string_lossy().to_string());
        let canon_path = dunce::canonicalize(&path)
            .ok()
            .map(|p| p.to_string_lossy().to_string());
        out.push(InstructionRecord {
            id: format!("discovered::{}", rel.replace(['/', '\\'], "::")),
            title: rel.to_string(),
            content,
            scope: InstructionScope::Project,
            project_root: canon_root,
            project_id: None,
            task_id: None,
            format: InstructionFormat::DiscoveredFile,
            enabled: true,
            agent_targets: Vec::new(),
            priority: 1000,
            tags: vec!["discovered".to_string()],
            created_at: now.clone(),
            updated_at: now.clone(),
            source_path: canon_path,
            managed: false,
            read_error,
        });
    }
    out
}

fn roots_match(a: &str, b: &str) -> bool {
    let na = dunce::canonicalize(a).unwrap_or_else(|_| PathBuf::from(a));
    let nb = dunce::canonicalize(b).unwrap_or_else(|_| PathBuf::from(b));
    na == nb
}

fn targets_match(record_targets: &[String], agent_kind: Option<&str>) -> bool {
    if record_targets.is_empty() {
        return true;
    }
    let Some(kind) = agent_kind else {
        return true;
    };
    let k = kind.trim().to_ascii_lowercase();
    if k.is_empty() || k == "all" {
        return true;
    }
    record_targets.iter().any(|t| t == "all" || *t == k)
}

pub fn resolve_instructions(
    store: &InstructionStoreFile,
    project_root: Option<&str>,
    project_id: Option<&str>,
    task_id: Option<&str>,
    agent_kind: Option<&str>,
) -> AppliedInstructionBlock {
    let mut warnings = Vec::new();
    let mut candidates: Vec<&InstructionRecord> = store
        .instructions
        .iter()
        .filter(|r| r.enabled)
        .filter(|r| match r.scope {
            InstructionScope::Global => true,
            InstructionScope::Project => {
                if let (Some(pr), Some(root)) = (r.project_root.as_deref(), project_root) {
                    if roots_match(pr, root) {
                        return true;
                    }
                }
                if let (Some(rpid), Some(pid)) = (r.project_id.as_deref(), project_id) {
                    return rpid == pid;
                }
                false
            }
            InstructionScope::Task => {
                if let (Some(rt), Some(tid)) = (r.task_id.as_deref(), task_id) {
                    return rt == tid;
                }
                false
            }
            InstructionScope::Agent => {
                // Optional project ownership: an agent-scoped instruction may be
                // global (no project) or tied to a specific project.
                if let Some(pr) = r.project_root.as_deref() {
                    match project_root {
                        Some(root) if roots_match(pr, root) => {}
                        _ => return false,
                    }
                } else if let Some(rpid) = r.project_id.as_deref() {
                    match project_id {
                        Some(pid) if rpid == pid => {}
                        _ => return false,
                    }
                }
                true
            }
        })
        .filter(|r| targets_match(&r.agent_targets, agent_kind))
        .collect();

    let scope_rank = |s: &InstructionScope| match s {
        InstructionScope::Global => 0,
        InstructionScope::Project => 1,
        InstructionScope::Agent => 2,
        InstructionScope::Task => 3,
    };
    candidates.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| scope_rank(&a.scope).cmp(&scope_rank(&b.scope)))
            .then_with(|| {
                a.title
                    .to_ascii_lowercase()
                    .cmp(&b.title.to_ascii_lowercase())
            })
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut seen_titles: HashSet<String> = HashSet::new();
    for r in &candidates {
        let key = format!("{:?}::{}", r.scope, r.title.trim().to_ascii_lowercase());
        if !seen_titles.insert(key) {
            warnings.push(format!(
                "Possible conflict: duplicate instruction title '{}' in the same scope.",
                r.title
            ));
        }
        if r.content.trim().is_empty() {
            warnings.push(format!(
                "Instruction '{}' is empty and will be skipped.",
                r.title
            ));
        }
    }

    let mut refs = Vec::new();
    let mut body = String::new();
    body.push_str("## Applied Instructions (managed, traceable)\n");
    body.push_str("The following enabled instructions were selected for this task. They guide behavior but do not override explicit user requests, safety rules, or approval gates.\n\n");
    let mut used_chars = body.chars().count();
    let mut truncated = false;

    for r in candidates {
        if r.content.trim().is_empty() {
            continue;
        }
        let scope_label = match r.scope {
            InstructionScope::Global => "global",
            InstructionScope::Project => "project",
            InstructionScope::Agent => "agent",
            InstructionScope::Task => "task",
        };
        let source = "managed";
        let header = format!("### [{} | {} | {}]\n", r.title, scope_label, r.id);
        let mut chunk = String::new();
        chunk.push_str(&header);
        chunk.push_str(r.content.trim());
        chunk.push_str("\n\n");
        let chunk_chars = chunk.chars().count();
        if used_chars + chunk_chars > MAX_APPLIED_INSTRUCTION_CHARS {
            truncated = true;
            warnings
                .push("Applied instructions were truncated to fit the prompt budget.".to_string());
            break;
        }
        used_chars += chunk_chars;
        body.push_str(&chunk);
        refs.push(AppliedInstructionRef {
            id: r.id.clone(),
            title: r.title.clone(),
            scope: r.scope.clone(),
            source: source.to_string(),
        });
    }

    if refs.is_empty() {
        body = String::new();
    }
    AppliedInstructionBlock {
        refs,
        text: body,
        warnings,
        truncated,
    }
}

pub fn merge_instruction_context(
    planning_context: Option<String>,
    block: &AppliedInstructionBlock,
) -> Option<String> {
    if block.text.trim().is_empty() {
        return planning_context;
    }
    match planning_context {
        Some(ctx) if !ctx.trim().is_empty() => {
            Some(format!("{}\n\n---\n\n{}", block.text.trim(), ctx))
        }
        _ => Some(block.text.trim().to_string()),
    }
}

pub fn validate_store(store: &InstructionStoreFile) -> Vec<InstructionValidationIssue> {
    let mut issues = Vec::new();
    let mut ids = HashSet::new();
    for r in &store.instructions {
        if !ids.insert(r.id.clone()) {
            issues.push(InstructionValidationIssue {
                id: Some(r.id.clone()),
                level: "error".to_string(),
                message: "Duplicate instruction ID".to_string(),
            });
        }
        if r.managed && r.title.trim().is_empty() {
            issues.push(InstructionValidationIssue {
                id: Some(r.id.clone()),
                level: "error".to_string(),
                message: "Managed instruction has empty title".to_string(),
            });
        }
        if r.scope == InstructionScope::Project
            && r.project_root.as_deref().unwrap_or("").trim().is_empty()
            && r.managed
        {
            issues.push(InstructionValidationIssue {
                id: Some(r.id.clone()),
                level: "error".to_string(),
                message: "Project-scoped instruction missing project_root".to_string(),
            });
        }
        if r.content.chars().count() > MAX_INSTRUCTION_CHARS {
            issues.push(InstructionValidationIssue {
                id: Some(r.id.clone()),
                level: "warning".to_string(),
                message: "Instruction content exceeds recommended size".to_string(),
            });
        }
    }
    issues
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("target")
            .join("scratch");
        let d = base.join(format!(
            "archify_instr_{}_{}_{}",
            label,
            std::process::id(),
            nanos
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn upsert_list_toggle_delete_roundtrip_persists() {
        let dir = temp_dir("roundtrip");
        let path = default_instruction_store_path(&dir);
        let mut store = load_instruction_store(&path);
        let rec = upsert_instruction(
            &mut store,
            InstructionUpsertInput {
                title: Some("Global Rule".to_string()),
                content: Some("Use Rust 2024.".to_string()),
                scope: Some(InstructionScope::Global),
                enabled: Some(true),
                ..Default::default()
            },
        )
        .unwrap();
        save_instruction_store(&path, &store).unwrap();
        let mut reloaded = load_instruction_store(&path);
        assert_eq!(reloaded.instructions.len(), 1);
        let toggled = set_instruction_enabled(&mut reloaded, &rec.id, false).unwrap();
        assert!(!toggled.enabled);
        delete_instruction(&mut reloaded, &rec.id).unwrap();
        assert!(reloaded.instructions.is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn project_scope_requires_root_and_resolves_for_matching_project() {
        let dir = temp_dir("scope");
        let project = dir.join("proj");
        fs::create_dir_all(&project).unwrap();
        let mut store = InstructionStoreFile::default();
        let err = upsert_instruction(
            &mut store,
            InstructionUpsertInput {
                title: Some("Proj".to_string()),
                content: Some("x".to_string()),
                scope: Some(InstructionScope::Project),
                project_root: None,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("project_root"));
        let root = project.to_string_lossy().to_string();
        upsert_instruction(
            &mut store,
            InstructionUpsertInput {
                title: Some("Proj".to_string()),
                content: Some("project-only".to_string()),
                scope: Some(InstructionScope::Project),
                project_root: Some(root.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        let block = resolve_instructions(&store, Some(&root), None, None, Some("planner"));
        assert!(block.text.contains("project-only"));
        let other = resolve_instructions(
            &store,
            Some(&dir.to_string_lossy()),
            None,
            None,
            Some("planner"),
        );
        assert!(other.text.is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn discovered_files_are_read_only_and_not_overwritten() {
        let dir = temp_dir("discover");
        let claude = dir.join("CLAUDE.md");
        fs::write(&claude, "original guidance").unwrap();
        let found = discover_project_instruction_files(&dir.to_string_lossy());
        assert!(
            found
                .iter()
                .any(|r| r.title == "CLAUDE.md" && !r.managed && r.source_path.is_some())
        );
        assert_eq!(fs::read_to_string(&claude).unwrap(), "original guidance");
        let mut store = InstructionStoreFile {
            instructions: found,
            ..Default::default()
        };
        let err = delete_instruction(&mut store, "discovered::CLAUDE.md").unwrap_err();
        assert!(err.contains("cannot be deleted"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn merge_preserves_existing_context_and_application_is_traceable() {
        let mut store = InstructionStoreFile::default();
        upsert_instruction(
            &mut store,
            InstructionUpsertInput {
                title: Some("T".to_string()),
                content: Some("Do X".to_string()),
                scope: Some(InstructionScope::Global),
                ..Default::default()
            },
        )
        .unwrap();
        let block = resolve_instructions(&store, None, None, None, Some("executor"));
        assert_eq!(block.refs.len(), 1);
        assert!(block.text.contains("traceable"));
        let merged =
            merge_instruction_context(Some("existing context".to_string()), &block).unwrap();
        assert!(merged.contains("Do X"));
        assert!(merged.contains("existing context"));
    }

    #[test]
    fn invalid_empty_title_and_malformed_store_handled_gracefully() {
        let mut store = InstructionStoreFile::default();
        let err = upsert_instruction(
            &mut store,
            InstructionUpsertInput {
                title: Some("   ".to_string()),
                content: Some("x".to_string()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(err.contains("title"));
        let dir = temp_dir("malformed");
        let path = default_instruction_store_path(&dir);
        fs::write(&path, "{not json").unwrap();
        let loaded = load_instruction_store(&path);
        assert!(loaded.instructions.is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn precedence_is_priority_then_global_before_project_then_title() {
        let dir = temp_dir("precedence");
        let project = dir.join("proj");
        fs::create_dir_all(&project).unwrap();
        let root = project.to_string_lossy().to_string();
        let mut store = InstructionStoreFile::default();

        for (title, scope, priority) in [
            ("Project later", InstructionScope::Project, 5),
            ("Global later", InstructionScope::Global, 5),
            ("Project first", InstructionScope::Project, 1),
            ("Global first", InstructionScope::Global, 1),
        ] {
            upsert_instruction(
                &mut store,
                InstructionUpsertInput {
                    title: Some(title.to_string()),
                    content: Some(title.to_string()),
                    scope: Some(scope),
                    project_root: Some(root.clone()),
                    priority: Some(priority),
                    ..Default::default()
                },
            )
            .unwrap();
        }

        let block = resolve_instructions(&store, Some(&root), None, None, Some("planner"));
        let titles: Vec<&str> = block.refs.iter().map(|r| r.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "Global first",
                "Project first",
                "Global later",
                "Project later"
            ]
        );

        let positions: Vec<usize> = titles
            .iter()
            .map(|title| block.text.find(title).expect("title in applied text"))
            .collect();
        assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn disabled_and_mismatched_agent_targets_are_not_applied() {
        let mut store = InstructionStoreFile::default();
        upsert_instruction(
            &mut store,
            InstructionUpsertInput {
                title: Some("Planner only".to_string()),
                content: Some("PLANNER RULE".to_string()),
                scope: Some(InstructionScope::Global),
                enabled: Some(true),
                agent_targets: Some(vec!["planner".to_string()]),
                ..Default::default()
            },
        )
        .unwrap();
        upsert_instruction(
            &mut store,
            InstructionUpsertInput {
                title: Some("Executor only".to_string()),
                content: Some("EXECUTOR RULE".to_string()),
                scope: Some(InstructionScope::Global),
                enabled: Some(true),
                agent_targets: Some(vec!["executor".to_string()]),
                ..Default::default()
            },
        )
        .unwrap();
        upsert_instruction(
            &mut store,
            InstructionUpsertInput {
                title: Some("Disabled".to_string()),
                content: Some("DISABLED RULE".to_string()),
                scope: Some(InstructionScope::Global),
                enabled: Some(false),
                ..Default::default()
            },
        )
        .unwrap();

        let planner = resolve_instructions(&store, None, None, None, Some("planner"));
        assert!(planner.text.contains("PLANNER RULE"));
        assert!(!planner.text.contains("EXECUTOR RULE"));
        assert!(!planner.text.contains("DISABLED RULE"));

        let executor = resolve_instructions(&store, None, None, None, Some("executor"));
        assert!(executor.text.contains("EXECUTOR RULE"));
        assert!(!executor.text.contains("PLANNER RULE"));
        assert!(!executor.text.contains("DISABLED RULE"));
    }

    #[test]
    fn switching_scope_to_global_clears_project_linkage() {
        let dir = temp_dir("scope_change");
        let project = dir.join("proj");
        fs::create_dir_all(&project).unwrap();
        let root = dunce::canonicalize(&project)
            .unwrap()
            .to_string_lossy()
            .to_string();
        let mut store = InstructionStoreFile::default();
        let rec = upsert_instruction(
            &mut store,
            InstructionUpsertInput {
                title: Some("Scoped".to_string()),
                content: Some("x".to_string()),
                scope: Some(InstructionScope::Project),
                project_root: Some(root.clone()),
                project_id: Some("project-1".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rec.project_root.as_deref(), Some(root.as_str()));
        assert_eq!(rec.project_id.as_deref(), Some("project-1"));

        let updated = upsert_instruction(
            &mut store,
            InstructionUpsertInput {
                id: Some(rec.id),
                content: Some("y".to_string()),
                scope: Some(InstructionScope::Global),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(updated.scope, InstructionScope::Global);
        assert_eq!(updated.project_root, None);
        assert_eq!(updated.project_id, None);
        assert_eq!(updated.content, "y");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn content_only_edit_survives_a_moved_project_directory() {
        let dir = temp_dir("moved_project");
        let project = dir.join("proj");
        fs::create_dir_all(&project).unwrap();
        let mut store = InstructionStoreFile::default();
        let rec = upsert_instruction(
            &mut store,
            InstructionUpsertInput {
                title: Some("Moved".to_string()),
                content: Some("before".to_string()),
                scope: Some(InstructionScope::Project),
                project_root: Some(project.to_string_lossy().to_string()),
                ..Default::default()
            },
        )
        .unwrap();

        fs::remove_dir_all(&project).unwrap();

        let updated = upsert_instruction(
            &mut store,
            InstructionUpsertInput {
                id: Some(rec.id),
                content: Some("after".to_string()),
                ..Default::default()
            },
        )
        .expect("content-only update should not require the old path to exist");
        assert_eq!(updated.content, "after");
        assert!(updated.project_root.is_some());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn malformed_store_is_quarantined_before_it_can_be_overwritten() {
        let dir = temp_dir("quarantine");
        let path = default_instruction_store_path(&dir);
        fs::write(&path, "{definitely not json").unwrap();

        let mut store = load_instruction_store(&path);
        assert!(store.instructions.is_empty());

        let backup = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .find(|candidate| {
                candidate
                    .file_name()
                    .map(|name| name.to_string_lossy().contains(".corrupt-"))
                    .unwrap_or(false)
            })
            .expect("malformed store should be preserved");
        assert_eq!(fs::read_to_string(&backup).unwrap(), "{definitely not json");

        upsert_instruction(
            &mut store,
            InstructionUpsertInput {
                title: Some("Recovered".to_string()),
                content: Some("works".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        save_instruction_store(&path, &store).unwrap();
        let reloaded = load_instruction_store(&path);
        assert_eq!(reloaded.instructions.len(), 1);
        assert_eq!(reloaded.instructions[0].title, "Recovered");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn archify_md_fingerprint_reflects_content_changes() {
        let dir = temp_dir("fp");
        let path = dir.join("ARCHIFY.md");
        fs::write(&path, "first").unwrap();
        let a = archify_md_fingerprint(&dir.to_string_lossy()).unwrap();
        fs::write(&path, "second").unwrap();
        let b = archify_md_fingerprint(&dir.to_string_lossy()).unwrap();
        assert_ne!(a, b);
        let missing = temp_dir("fp_missing");
        assert!(archify_md_fingerprint(&missing.to_string_lossy()).is_none());
        let _ = fs::remove_dir_all(dir);
        let _ = fs::remove_dir_all(missing);
    }

    #[test]
    fn ensure_archify_md_creates_then_preserves() {
        let dir = temp_dir("ensure");
        assert!(ensure_archify_md(&dir.to_string_lossy()));
        let path = dir.join("ARCHIFY.md");
        assert!(path.exists());
        let original = fs::read_to_string(&path).unwrap();
        // Second call must not overwrite.
        assert!(!ensure_archify_md(&dir.to_string_lossy()));
        assert_eq!(fs::read_to_string(&path).unwrap(), original);
        // Existing content preserved.
        fs::write(&path, "user content").unwrap();
        assert!(!ensure_archify_md(&dir.to_string_lossy()));
        assert_eq!(fs::read_to_string(&path).unwrap(), "user content");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn task_scoped_instruction_roundtrips() {
        let mut store = InstructionStoreFile::default();
        let rec = upsert_instruction(
            &mut store,
            InstructionUpsertInput {
                title: Some("Task rule".to_string()),
                content: Some("do task thing".to_string()),
                scope: Some(InstructionScope::Task),
                task_id: Some("task-123".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(rec.scope, InstructionScope::Task);
        assert_eq!(rec.task_id.as_deref(), Some("task-123"));
        // Resolves only for matching task.
        let block = resolve_instructions(&store, None, None, Some("task-123"), None);
        assert_eq!(block.refs.len(), 1);
        let none = resolve_instructions(&store, None, None, Some("other"), None);
        assert_eq!(none.refs.len(), 0);
        // Global/project scopes still work.
        let _ = resolve_instructions(&store, None, None, None, Some("planner"));
    }
}
