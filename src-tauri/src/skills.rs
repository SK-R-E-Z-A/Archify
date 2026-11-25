//! Skills Manager backend.
//!
//! A *skill* is a reusable, task-specific procedure stored using the widely
//! used Agent Skills format: a directory containing a `SKILL.md` file with YAML
//! frontmatter (`name`, `description`, optional extra keys) and a Markdown body.
//!
//! This module deliberately owns a standalone registry. It does **not** inject
//! skills into prompts. `resolve_skills_for_agent` is the explicit integration
//! interface for a future coordinated change to prompt/task construction; see
//! `docs/skills-manager-integration.md`.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const MAX_SKILL_CONTENT_CHARS: usize = 64 * 1024;
pub const MAX_SKILL_DESCRIPTION_CHARS: usize = 1024;
pub const MAX_SKILL_MARKDOWN_BYTES: usize = 256 * 1024;
pub const MAX_APPLIED_SKILL_CHARS: usize = 16 * 1024;
pub const MAX_IMPORTED_FILES: usize = 200;
pub const MAX_IMPORTED_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SkillSource {
    /// Created inside Archify.
    Managed,
    /// Copied into the Archify library from an existing `SKILL.md`.
    Imported,
    /// A read-only reference discovered in the open project.
    Discovered,
}

impl Default for SkillSource {
    fn default() -> Self {
        Self::Managed
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SkillValidationIssue {
    #[serde(default)]
    pub skill_id: Option<String>,
    /// `error` blocks saving/applying; `warning` is informational.
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillRecord {
    pub id: String,
    /// Lower-case identifier compatible with the Agent Skills `name` field.
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub category: String,
    /// Empty means "all agents/providers".
    /// Values may include: planner, executor, reviewer, claude, codex, aider,
    /// gemini, opencode, cursor-agent, custom.
    #[serde(default)]
    pub supported_agents: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub source: SkillSource,
    /// Managed/imported records are editable/deletable; discovered records are
    /// read-only references to files in the project.
    #[serde(default = "default_true")]
    pub managed: bool,
    /// Markdown body, stored without the YAML frontmatter delimiters.
    #[serde(default)]
    pub content: String,
    /// Remaining YAML frontmatter (without `name`/`description`) preserved on
    /// round-trip so compatible extra keys are not lost.
    #[serde(default)]
    pub raw_frontmatter: String,
    /// Absolute path to the source `SKILL.md` for imported/discovered records.
    #[serde(default)]
    pub source_path: Option<String>,
    /// Absolute directory containing the managed `SKILL.md`.
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub read_error: Option<String>,
    #[serde(default)]
    pub validation_issues: Vec<SkillValidationIssue>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillStoreFile {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub skills: Vec<SkillRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillUpsertInput {
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub supported_agents: Option<Vec<String>>,
    pub tags: Option<Vec<String>>,
    pub enabled: Option<bool>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedSkillRef {
    pub id: String,
    pub name: String,
    pub category: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppliedSkillBlock {
    pub refs: Vec<AppliedSkillRef>,
    pub text: String,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub truncated: bool,
}

pub fn default_skill_store_path(base_dir: &Path) -> PathBuf {
    base_dir.join("skills.json")
}

pub fn default_skill_library_dir(base_dir: &Path) -> PathBuf {
    base_dir.join("skills")
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

fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|e| format!("Failed to write {}: {}", tmp.display(), e))?;
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

pub fn load_skill_store(path: &Path) -> SkillStoreFile {
    if !path.exists() {
        return SkillStoreFile {
            version: 1,
            skills: Vec::new(),
        };
    }
    match fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<SkillStoreFile>(&content) {
            Ok(mut store) => {
                if store.version == 0 {
                    store.version = 1;
                }
                store.skills.retain(|s| !s.id.trim().is_empty());
                store
            }
            Err(e) => {
                eprintln!("[skills] malformed store {}: {}", path.display(), e);
                SkillStoreFile {
                    version: 1,
                    skills: Vec::new(),
                }
            }
        },
        Err(e) => {
            eprintln!("[skills] unreadable store {}: {}", path.display(), e);
            SkillStoreFile {
                version: 1,
                skills: Vec::new(),
            }
        }
    }
}

pub fn save_skill_store(path: &Path, store: &SkillStoreFile) -> Result<(), String> {
    write_json_atomic(path, store)
}

// ---------------------------------------------------------------------------
// SKILL.md parsing / rendering
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct ParsedSkillMarkdown {
    pub frontmatter: String,
    pub body: String,
    pub fields: BTreeMap<String, String>,
}

pub fn parse_skill_markdown(raw: &str) -> Result<ParsedSkillMarkdown, String> {
    let without_bom = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let normalized = without_bom.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines: Vec<&str> = normalized.lines().collect();
    while lines.first().map(|l| l.trim().is_empty()).unwrap_or(false) {
        lines.remove(0);
    }
    if lines.is_empty() {
        return Err("SKILL.md is empty".to_string());
    }
    if lines[0].trim() != "---" {
        return Err(
            "SKILL.md must begin with a YAML frontmatter block delimited by ---".to_string(),
        );
    }
    let end = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| l.trim() == "---")
        .map(|(i, _)| i)
        .ok_or_else(|| "SKILL.md frontmatter is missing its closing ---".to_string())?;
    let frontmatter = lines[1..end].join("\n");
    let body = lines[end + 1..].join("\n");
    let fields = parse_frontmatter_fields(&frontmatter);
    Ok(ParsedSkillMarkdown {
        frontmatter,
        body: body.trim().to_string(),
        fields,
    })
}

fn parse_frontmatter_fields(frontmatter: &str) -> BTreeMap<String, String> {
    let lines: Vec<&str> = frontmatter.lines().collect();
    let mut fields = BTreeMap::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim().is_empty() || line.starts_with(' ') || line.starts_with('\t') {
            i += 1;
            continue;
        }
        let Some(colon) = line.find(':') else {
            i += 1;
            continue;
        };
        let key = line[..colon].trim().to_ascii_lowercase();
        if key.is_empty() {
            i += 1;
            continue;
        }
        let value = line[colon + 1..].trim();
        if matches!(value, "|" | "|-" | "|+" | ">" | ">-" | ">+") {
            let folded = value.starts_with('>');
            let mut collected = Vec::new();
            i += 1;
            while i < lines.len() {
                let l = lines[i];
                if l.trim().is_empty() {
                    collected.push(String::new());
                    i += 1;
                    continue;
                }
                if !(l.starts_with(' ') || l.starts_with('\t')) {
                    break;
                }
                collected.push(l.trim_start().to_string());
                i += 1;
            }
            while collected.last().map(|s| s.is_empty()).unwrap_or(false) {
                collected.pop();
            }
            let joined = if folded {
                collected.join(" ")
            } else {
                collected.join("\n")
            };
            fields.insert(key, joined);
            continue;
        }
        // Nested mappings/lists are preserved in `raw_frontmatter`; only scalar
        // top-level values are surfaced here.
        fields.insert(key, unquote_scalar(value));
        i += 1;
    }
    fields
}

fn unquote_scalar(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
        let inner = &trimmed[1..trimmed.len() - 1];
        let mut out = String::with_capacity(inner.len());
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c == '\\' {
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            } else {
                out.push(c);
            }
        }
        return out;
    }
    if trimmed.len() >= 2 && trimmed.starts_with('\'') && trimmed.ends_with('\'') {
        return trimmed[1..trimmed.len() - 1].replace("''", "'");
    }
    trimmed.to_string()
}

/// Remove top-level `keys` (and their indented continuation lines) while
/// preserving every other frontmatter line exactly.
fn frontmatter_without(frontmatter: &str, keys: &[&str]) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut skipping = false;
    for line in frontmatter.lines() {
        let top_level =
            !line.starts_with(' ') && !line.starts_with('\t') && !line.trim().is_empty();
        if top_level {
            let key = line
                .split(':')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            skipping = keys.contains(&key.as_str());
        }
        if !skipping {
            out.push(line);
        }
    }
    out.join("\n").trim().to_string()
}

fn yaml_scalar(value: &str) -> String {
    let safe = !value.is_empty()
        && value
            .chars()
            .next()
            .map(|c| c.is_ascii_alphanumeric())
            .unwrap_or(false)
        && !value.starts_with(' ')
        && !value.ends_with(' ')
        && !value
            .chars()
            .any(|c| c == ':' || c == '#' || c == '\n' || c == '\r' || c == '"' || c == '\'');
    if safe {
        value.to_string()
    } else {
        format!(
            "\"{}\"",
            value
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
        )
    }
}

pub fn render_skill_markdown(record: &SkillRecord) -> String {
    let extras = frontmatter_without(&record.raw_frontmatter, &["name", "description"]);
    let mut frontmatter = String::new();
    frontmatter.push_str(&format!("name: {}\n", yaml_scalar(&record.name)));
    frontmatter.push_str(&format!(
        "description: {}\n",
        yaml_scalar(&record.description)
    ));
    if !extras.is_empty() {
        frontmatter.push('\n');
        frontmatter.push_str(&extras);
        frontmatter.push('\n');
    }
    format!("---\n{}---\n\n{}\n", frontmatter, record.content.trim())
}

// ---------------------------------------------------------------------------
// Validation / normalization
// ---------------------------------------------------------------------------

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

fn normalize_single_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn normalize_name(raw: &str) -> String {
    let mut out = String::new();
    let mut last_sep = false;
    for c in raw.trim().chars() {
        let c = c.to_ascii_lowercase();
        if c.is_ascii_alphanumeric() || c == '.' || c == '_' {
            out.push(c);
            last_sep = false;
        } else if (c.is_whitespace() || c == '-' || c == '/') && !last_sep && !out.is_empty() {
            out.push('-');
            last_sep = true;
        }
    }
    out.trim_matches(['-', '.']).chars().take(100).collect()
}

fn normalize_list(values: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for value in values {
        let norm = value.trim().to_ascii_lowercase();
        if norm.is_empty() || norm.len() > 64 {
            continue;
        }
        if seen.insert(norm.clone()) {
            out.push(norm);
        }
    }
    out
}

fn normalize_category(raw: &str) -> String {
    let value = normalize_single_line(raw);
    if value.is_empty() {
        "uncategorized".to_string()
    } else {
        value.chars().take(64).collect()
    }
}

pub fn validate_record(record: &SkillRecord) -> Vec<SkillValidationIssue> {
    let mut issues = Vec::new();
    let mut push = |level: &str, message: String| {
        issues.push(SkillValidationIssue {
            skill_id: Some(record.id.clone()),
            level: level.to_string(),
            message,
        });
    };

    if record.name.trim().is_empty() {
        push("error", "Skill name is required.".to_string());
    } else if record.name.chars().count() > 100 {
        push("error", "Skill name exceeds 100 characters.".to_string());
    } else if record
        .name
        .chars()
        .any(|c| !(c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'))
    {
        push(
            "error",
            "Skill name may only contain lowercase letters, numbers, '-', '_' and '.'.".to_string(),
        );
    }

    if record.description.trim().is_empty() {
        push("error", "Skill description is required.".to_string());
    } else if record.description.chars().count() > MAX_SKILL_DESCRIPTION_CHARS {
        push(
            "error",
            format!(
                "Skill description exceeds {} characters.",
                MAX_SKILL_DESCRIPTION_CHARS
            ),
        );
    }

    if record.content.trim().is_empty() {
        push(
            "warning",
            "Skill has no procedure content and will not be applied.".to_string(),
        );
    } else if record.content.chars().count() > MAX_SKILL_CONTENT_CHARS {
        push(
            "error",
            format!(
                "Skill content exceeds {} characters.",
                MAX_SKILL_CONTENT_CHARS
            ),
        );
    }
    if record.category.chars().count() > 64 {
        push("error", "Skill category exceeds 64 characters.".to_string());
    }
    if record
        .supported_agents
        .iter()
        .any(|a| a.chars().count() > 64 || a.chars().any(|c| c.is_control()))
    {
        push(
            "error",
            "Supported agents contain an invalid value.".to_string(),
        );
    }
    if !record.managed && record.source_path.is_none() {
        push(
            "warning",
            "Discovered skill has no source path; it cannot be imported.".to_string(),
        );
    }
    issues
}

fn first_error(issues: &[SkillValidationIssue]) -> Option<String> {
    issues
        .iter()
        .find(|i| i.level == "error")
        .map(|i| i.message.clone())
}

pub fn refresh_validation(store: &mut SkillStoreFile, library_dir: &Path) {
    for record in &mut store.skills {
        record.validation_issues = validate_record(record);
        if record.managed {
            let file = record
                .directory
                .as_deref()
                .map(|dir| Path::new(dir).join("SKILL.md"))
                .unwrap_or_else(|| library_dir.join("__missing__").join("SKILL.md"));
            if !file.is_file() {
                record.read_error = Some(
                    "Skill file is missing; edit and save this skill to restore it.".to_string(),
                );
                record.validation_issues.push(SkillValidationIssue {
                    skill_id: Some(record.id.clone()),
                    level: "error".to_string(),
                    message: "Skill file is missing from the library.".to_string(),
                });
            } else {
                record.read_error = None;
            }
        }
    }
}

pub fn validate_skill_store(
    store: &SkillStoreFile,
    library_dir: &Path,
) -> Vec<SkillValidationIssue> {
    let mut issues = Vec::new();
    let mut names = HashSet::new();
    let mut ids = HashSet::new();
    for record in &store.skills {
        if !ids.insert(record.id.clone()) {
            issues.push(SkillValidationIssue {
                skill_id: Some(record.id.clone()),
                level: "error".to_string(),
                message: "Duplicate skill ID.".to_string(),
            });
        }
        if record.managed {
            let key = record.name.to_ascii_lowercase();
            if !key.is_empty() && !names.insert(key) {
                issues.push(SkillValidationIssue {
                    skill_id: Some(record.id.clone()),
                    level: "error".to_string(),
                    message: format!("Duplicate skill name '{}'.", record.name),
                });
            }
        }
        issues.extend(validate_record(record));
        if record.managed {
            let exists = record
                .directory
                .as_deref()
                .map(|dir| Path::new(dir).join("SKILL.md").is_file())
                .unwrap_or(false);
            if !exists {
                issues.push(SkillValidationIssue {
                    skill_id: Some(record.id.clone()),
                    level: "error".to_string(),
                    message: "Managed skill file is missing from the library.".to_string(),
                });
            }
        }
    }
    let _ = library_dir;
    issues
}

// ---------------------------------------------------------------------------
// Storage helpers
// ---------------------------------------------------------------------------

fn paths_equal(a: &Path, b: &Path) -> bool {
    let canon = |p: &Path| dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    canon(a) == canon(b)
}

fn path_is_within(child: &Path, parent: &Path) -> bool {
    let canon = |p: &Path| dunce::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let child = canon(child);
    let parent = canon(parent);
    child != parent && child.starts_with(&parent)
}

fn ensure_unique_name(
    store: &SkillStoreFile,
    name: &str,
    except_id: Option<&str>,
) -> Result<(), String> {
    if name.is_empty() {
        return Err("Skill name is required.".to_string());
    }
    let collision = store.skills.iter().any(|r| {
        r.managed && r.name.eq_ignore_ascii_case(name) && Some(r.id.as_str()) != except_id
    });
    if collision {
        Err(format!("A skill named '{}' already exists.", name))
    } else {
        Ok(())
    }
}

fn write_managed_skill(library_dir: &Path, record: &mut SkillRecord) -> Result<(), String> {
    let slug = if record.name.is_empty() {
        "skill".to_string()
    } else {
        record.name.clone()
    };
    let desired = library_dir.join(&slug);
    let rendered = render_skill_markdown(record);
    if rendered.len() > MAX_SKILL_MARKDOWN_BYTES {
        return Err(format!(
            "Rendered SKILL.md exceeds {} bytes.",
            MAX_SKILL_MARKDOWN_BYTES
        ));
    }
    fs::create_dir_all(&desired)
        .map_err(|e| format!("Failed to create {}: {}", desired.display(), e))?;
    write_bytes_atomic(&desired.join("SKILL.md"), rendered.as_bytes())?;

    if let Some(old) = record.directory.as_deref() {
        let old_path = PathBuf::from(old);
        if !paths_equal(&old_path, &desired)
            && path_is_within(&old_path, library_dir)
            && old_path.exists()
        {
            let _ = fs::remove_dir_all(&old_path);
        }
    }
    record.directory = Some(desired.to_string_lossy().to_string());
    Ok(())
}

fn copy_auxiliary_files(src: &Path, dst: &Path) -> Result<(), String> {
    let mut stack = vec![src.to_path_buf()];
    let mut files = 0usize;
    let mut bytes = 0u64;
    while let Some(dir) = stack.pop() {
        let entries =
            fs::read_dir(&dir).map_err(|e| format!("Failed to read {}: {}", dir.display(), e))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("Failed to read directory entry: {}", e))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|e| format!("Failed to inspect {}: {}", path.display(), e))?;
            if file_type.is_symlink() {
                continue;
            }
            let rel = path
                .strip_prefix(src)
                .map_err(|e| format!("Failed to compute relative path: {}", e))?;
            if rel == Path::new("SKILL.md") {
                continue;
            }
            let target = dst.join(rel);
            if file_type.is_dir() {
                fs::create_dir_all(&target)
                    .map_err(|e| format!("Failed to create {}: {}", target.display(), e))?;
                stack.push(path);
                continue;
            }
            if !file_type.is_file() {
                continue;
            }
            files += 1;
            let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            bytes += size;
            if files > MAX_IMPORTED_FILES || bytes > MAX_IMPORTED_BYTES {
                return Err(format!(
                    "Skill directory is too large to import (limit: {} files / {} bytes).",
                    MAX_IMPORTED_FILES, MAX_IMPORTED_BYTES
                ));
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
            }
            fs::copy(&path, &target).map_err(|e| {
                format!(
                    "Failed to copy {} to {}: {}",
                    path.display(),
                    target.display(),
                    e
                )
            })?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CRUD
// ---------------------------------------------------------------------------

pub fn upsert_skill(
    store: &mut SkillStoreFile,
    library_dir: &Path,
    input: SkillUpsertInput,
) -> Result<SkillRecord, String> {
    let now = Utc::now().to_rfc3339();

    if let Some(id) = input.id.as_ref().filter(|s| !s.trim().is_empty()) {
        let idx = store
            .skills
            .iter()
            .position(|r| r.id == *id && r.managed)
            .ok_or_else(|| format!("Managed skill '{}' not found", id))?;
        let mut record = store.skills[idx].clone();

        if let Some(name) = input.name {
            record.name = normalize_name(&name);
        }
        if let Some(description) = input.description {
            record.description = sanitize_text(
                "description",
                &normalize_single_line(&description),
                MAX_SKILL_DESCRIPTION_CHARS,
            )?;
        }
        if let Some(category) = input.category {
            record.category = normalize_category(&category);
        }
        if let Some(agents) = input.supported_agents {
            record.supported_agents = normalize_list(agents);
        }
        if let Some(tags) = input.tags {
            record.tags = normalize_list(tags);
        }
        if let Some(enabled) = input.enabled {
            record.enabled = enabled;
        }
        if let Some(content) = input.content {
            record.content = sanitize_text("content", &content, MAX_SKILL_CONTENT_CHARS)?;
        }
        if record.name.is_empty() {
            return Err("Skill name is required.".to_string());
        }
        if record.description.is_empty() {
            return Err("Skill description is required.".to_string());
        }
        record.category = normalize_category(&record.category);

        ensure_unique_name(store, &record.name, Some(&record.id))?;
        record.validation_issues = validate_record(&record);
        if let Some(error) = first_error(&record.validation_issues) {
            return Err(error);
        }
        record.updated_at = now;
        write_managed_skill(library_dir, &mut record)?;
        record.validation_issues = validate_record(&record);
        record.read_error = None;
        store.skills[idx] = record.clone();
        return Ok(record);
    }

    let name = normalize_name(input.name.as_deref().unwrap_or(""));
    if name.is_empty() {
        return Err("Skill name is required.".to_string());
    }
    ensure_unique_name(store, &name, None)?;
    let description = sanitize_text(
        "description",
        &normalize_single_line(input.description.as_deref().unwrap_or("")),
        MAX_SKILL_DESCRIPTION_CHARS,
    )?;
    if description.is_empty() {
        return Err("Skill description is required.".to_string());
    }
    let content = sanitize_text(
        "content",
        input.content.as_deref().unwrap_or(""),
        MAX_SKILL_CONTENT_CHARS,
    )?;

    let mut record = SkillRecord {
        id: Uuid::new_v4().to_string(),
        name,
        description,
        category: normalize_category(input.category.as_deref().unwrap_or("")),
        supported_agents: normalize_list(input.supported_agents.unwrap_or_default()),
        tags: normalize_list(input.tags.unwrap_or_default()),
        enabled: input.enabled.unwrap_or(true),
        source: SkillSource::Managed,
        managed: true,
        content,
        raw_frontmatter: String::new(),
        source_path: None,
        directory: None,
        created_at: now.clone(),
        updated_at: now,
        read_error: None,
        validation_issues: Vec::new(),
    };
    record.validation_issues = validate_record(&record);
    if let Some(error) = first_error(&record.validation_issues) {
        return Err(error);
    }
    write_managed_skill(library_dir, &mut record)?;
    record.validation_issues = validate_record(&record);
    store.skills.push(record.clone());
    Ok(record)
}

pub fn set_skill_enabled(
    store: &mut SkillStoreFile,
    id: &str,
    enabled: bool,
) -> Result<SkillRecord, String> {
    let record = store
        .skills
        .iter_mut()
        .find(|r| r.id == id)
        .ok_or_else(|| format!("Skill '{}' not found", id))?;
    if !record.managed {
        return Err(
            "Discovered skills are read-only references; import one to enable or disable it."
                .to_string(),
        );
    }
    record.enabled = enabled;
    record.updated_at = Utc::now().to_rfc3339();
    Ok(record.clone())
}

pub fn delete_skill(
    store: &mut SkillStoreFile,
    library_dir: &Path,
    id: &str,
) -> Result<(), String> {
    let idx = store
        .skills
        .iter()
        .position(|r| r.id == id)
        .ok_or_else(|| format!("Skill '{}' not found", id))?;
    let record = &store.skills[idx];
    if !record.managed {
        return Err(
            "Discovered skills cannot be deleted from the Skills Manager; remove the source SKILL.md in the project instead."
                .to_string(),
        );
    }
    if let Some(dir) = record.directory.as_deref() {
        let path = PathBuf::from(dir);
        if path.exists() {
            if !path_is_within(&path, library_dir) {
                return Err(format!(
                    "Refusing to delete '{}' because it is outside the managed skill library.",
                    path.display()
                ));
            }
            fs::remove_dir_all(&path)
                .map_err(|e| format!("Failed to remove {}: {}", path.display(), e))?;
        }
    }
    store.skills.remove(idx);
    Ok(())
}

// ---------------------------------------------------------------------------
// Import / discovery
// ---------------------------------------------------------------------------

fn limited_read_to_string(path: &Path) -> Result<String, String> {
    let metadata =
        fs::metadata(path).map_err(|e| format!("Failed to inspect {}: {}", path.display(), e))?;
    if metadata.len() > MAX_SKILL_MARKDOWN_BYTES as u64 {
        return Err(format!(
            "{} exceeds the {} byte SKILL.md limit.",
            path.display(),
            MAX_SKILL_MARKDOWN_BYTES
        ));
    }
    fs::read_to_string(path).map_err(|e| format!("Failed to read {}: {}", path.display(), e))
}

pub fn import_skill(
    store: &mut SkillStoreFile,
    library_dir: &Path,
    source_path: &str,
) -> Result<SkillRecord, String> {
    let trimmed = source_path.trim();
    if trimmed.is_empty() {
        return Err("Import path is required.".to_string());
    }
    let source = dunce::canonicalize(trimmed)
        .map_err(|e| format!("Invalid skill path '{}': {}", trimmed, e))?;
    // Only copy auxiliary files when an explicit skill directory is imported.
    // A direct SKILL.md path imports just that file to avoid accidentally
    // copying an entire `.agents/skills` root.
    let (skill_file, source_dir) = if source.is_dir() {
        (source.join("SKILL.md"), Some(source.clone()))
    } else {
        (source.clone(), None)
    };
    if !skill_file.is_file() {
        return Err(format!("No SKILL.md found at {}.", skill_file.display()));
    }
    let source_key = skill_file.to_string_lossy().to_string();
    if store
        .skills
        .iter()
        .any(|r| r.managed && r.source_path.as_deref() == Some(source_key.as_str()))
    {
        return Err(format!("'{}' has already been imported.", source_key));
    }

    let raw = limited_read_to_string(&skill_file)?;
    let parsed = parse_skill_markdown(&raw)?;
    let fallback_name = source
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = normalize_name(
        parsed
            .fields
            .get("name")
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(&fallback_name),
    );
    if name.is_empty() {
        return Err("Skill name is required (frontmatter `name` or directory name).".to_string());
    }
    let description = normalize_single_line(
        parsed
            .fields
            .get("description")
            .map(String::as_str)
            .unwrap_or(""),
    );
    if description.is_empty() {
        return Err(format!(
            "Skill '{}' is missing the required frontmatter `description`.",
            skill_file.display()
        ));
    }
    ensure_unique_name(store, &name, None)?;

    let now = Utc::now().to_rfc3339();
    let mut record = SkillRecord {
        id: Uuid::new_v4().to_string(),
        name,
        description,
        category: normalize_category(
            parsed
                .fields
                .get("category")
                .map(String::as_str)
                .unwrap_or(""),
        ),
        supported_agents: normalize_list(
            parsed
                .fields
                .get("agents")
                .map(|value| value.split(',').map(|s| s.to_string()).collect())
                .unwrap_or_default(),
        ),
        tags: normalize_list(
            parsed
                .fields
                .get("tags")
                .map(|value| value.split(',').map(|s| s.to_string()).collect())
                .unwrap_or_default(),
        ),
        enabled: true,
        source: SkillSource::Imported,
        managed: true,
        content: parsed.body,
        raw_frontmatter: parsed.frontmatter,
        source_path: Some(source_key),
        directory: None,
        created_at: now.clone(),
        updated_at: now,
        read_error: None,
        validation_issues: Vec::new(),
    };
    record.validation_issues = validate_record(&record);
    if let Some(error) = first_error(&record.validation_issues) {
        return Err(format!("Invalid skill '{}': {}", record.name, error));
    }

    let desired = library_dir.join(&record.name);
    if let Some(dir) = source_dir.as_deref() {
        if !paths_equal(dir, &desired) {
            fs::create_dir_all(&desired)
                .map_err(|e| format!("Failed to create {}: {}", desired.display(), e))?;
            if let Err(e) = copy_auxiliary_files(dir, &desired) {
                let _ = fs::remove_dir_all(&desired);
                return Err(e);
            }
        }
    }
    write_managed_skill(library_dir, &mut record)?;
    record.validation_issues = validate_record(&record);
    store.skills.push(record.clone());
    Ok(record)
}

pub fn discover_project_skills(project_root: &str) -> Vec<SkillRecord> {
    let root = Path::new(project_root);
    if !root.is_dir() {
        return Vec::new();
    }
    let now = Utc::now().to_rfc3339();
    let mut out = Vec::new();
    for relative in [".agents/skills", ".claude/skills"] {
        let skills_dir = root.join(relative);
        if !skills_dir.is_dir() {
            continue;
        }
        let mut entries: Vec<PathBuf> = match fs::read_dir(&skills_dir) {
            Ok(entries) => entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect(),
            Err(e) => {
                eprintln!("[skills] could not read {}: {}", skills_dir.display(), e);
                continue;
            }
        };
        entries.sort_by_key(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default()
        });
        for dir in entries {
            let skill_file = dir.join("SKILL.md");
            if !skill_file.is_file() {
                continue;
            }
            let dir_name = dir
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let canonical_dir = dunce::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
            let canonical_file =
                dunce::canonicalize(&skill_file).unwrap_or_else(|_| skill_file.clone());
            let mut record = SkillRecord {
                id: format!(
                    "discovered::{}",
                    canonical_file.to_string_lossy().to_string()
                ),
                name: dir_name.clone(),
                description: String::new(),
                category: String::new(),
                supported_agents: Vec::new(),
                tags: Vec::new(),
                enabled: true,
                source: SkillSource::Discovered,
                managed: false,
                content: String::new(),
                raw_frontmatter: String::new(),
                source_path: Some(canonical_file.to_string_lossy().to_string()),
                directory: Some(canonical_dir.to_string_lossy().to_string()),
                created_at: now.clone(),
                updated_at: now.clone(),
                read_error: None,
                validation_issues: Vec::new(),
            };
            match limited_read_to_string(&skill_file) {
                Ok(raw) => match parse_skill_markdown(&raw) {
                    Ok(parsed) => {
                        if let Some(name) =
                            parsed.fields.get("name").filter(|n| !n.trim().is_empty())
                        {
                            record.name = normalize_name(name);
                            if !record.name.is_empty() && record.name != normalize_name(&dir_name) {
                                record.validation_issues.push(SkillValidationIssue {
                                    skill_id: Some(record.id.clone()),
                                    level: "warning".to_string(),
                                    message: format!(
                                        "Frontmatter name '{}' differs from directory '{}'.",
                                        record.name, dir_name
                                    ),
                                });
                            }
                        }
                        record.description = normalize_single_line(
                            parsed
                                .fields
                                .get("description")
                                .map(String::as_str)
                                .unwrap_or(""),
                        );
                        record.category = normalize_category(
                            parsed
                                .fields
                                .get("category")
                                .map(String::as_str)
                                .unwrap_or(""),
                        );
                        record.tags = normalize_list(
                            parsed
                                .fields
                                .get("tags")
                                .map(|value| value.split(',').map(|s| s.to_string()).collect())
                                .unwrap_or_default(),
                        );
                        record.supported_agents = normalize_list(
                            parsed
                                .fields
                                .get("agents")
                                .map(|value| value.split(',').map(|s| s.to_string()).collect())
                                .unwrap_or_default(),
                        );
                        record.content = parsed.body;
                        record.raw_frontmatter = parsed.frontmatter;
                        record.validation_issues.extend(validate_record(&record));
                    }
                    Err(e) => {
                        record.read_error = Some(e.clone());
                        record.validation_issues.push(SkillValidationIssue {
                            skill_id: Some(record.id.clone()),
                            level: "error".to_string(),
                            message: e,
                        });
                    }
                },
                Err(e) => {
                    record.read_error = Some(e.clone());
                    record.validation_issues.push(SkillValidationIssue {
                        skill_id: Some(record.id.clone()),
                        level: "error".to_string(),
                        message: e,
                    });
                }
            }
            out.push(record);
        }
    }
    out.sort_by(|a, b| {
        a.category
            .to_ascii_lowercase()
            .cmp(&b.category.to_ascii_lowercase())
            .then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
            .then_with(|| a.id.cmp(&b.id))
    });
    out
}

// ---------------------------------------------------------------------------
// Integration interface (deliberately not called by orchestrator.rs)
// ---------------------------------------------------------------------------

fn targets_match(record_targets: &[String], agent_kind: Option<&str>) -> bool {
    if record_targets.is_empty() {
        return true;
    }
    let Some(kind) = agent_kind else {
        return true;
    };
    let kind = kind.trim().to_ascii_lowercase();
    if kind.is_empty() || kind == "all" {
        return true;
    }
    record_targets
        .iter()
        .any(|target| target == "all" || *target == kind)
}

/// Resolve an explicit, user-selected list of skills into a bounded prompt
/// block. Skills are never applied implicitly: an empty `selected_ids` always
/// produces an empty block.
///
/// This is a stable integration point for task/session orchestration. It is not
pub fn resolve_skills_for_agent(
    store: &SkillStoreFile,
    selected_ids: &[String],
    agent_kind: Option<&str>,
) -> AppliedSkillBlock {
    let mut warnings = Vec::new();
    let mut refs = Vec::new();
    let mut body = String::new();
    let mut truncated = false;

    if selected_ids.is_empty() {
        return AppliedSkillBlock {
            refs,
            text: body,
            warnings,
            truncated,
        };
    }

    body.push_str("## Selected Skills (explicitly chosen for this task)\n");
    body.push_str(
        "The following skills were selected for this task. Follow a skill's procedure when its trigger conditions apply. Skills guide how to perform work; they do not override instructions, explicit user requests, safety rules, or approval gates.\n\n",
    );
    let mut used_chars = body.chars().count();
    let mut seen = HashSet::new();

    for id in selected_ids {
        if !seen.insert(id.clone()) {
            continue;
        }
        let Some(record) = store.skills.iter().find(|r| r.id == *id && r.managed) else {
            warnings.push(format!("Selected skill '{}' was not found.", id));
            continue;
        };
        if !record.enabled {
            warnings.push(format!(
                "Selected skill '{}' is disabled and was skipped.",
                record.name
            ));
            continue;
        }
        if !targets_match(&record.supported_agents, agent_kind) {
            warnings.push(format!(
                "Selected skill '{}' does not target agent '{}' and was skipped.",
                record.name,
                agent_kind.unwrap_or("all")
            ));
            continue;
        }
        let issues = validate_record(record);
        if let Some(error) = first_error(&issues) {
            warnings.push(format!(
                "Selected skill '{}' is invalid and was skipped: {}",
                record.name, error
            ));
            continue;
        }
        if record.content.trim().is_empty() {
            warnings.push(format!(
                "Selected skill '{}' has empty content and was skipped.",
                record.name
            ));
            continue;
        }
        let chunk = format!(
            "### [{} | {} | {}]\n{}\n\n",
            record.name,
            if record.category.is_empty() {
                "uncategorized"
            } else {
                &record.category
            },
            record.id,
            record.content.trim()
        );
        let chunk_chars = chunk.chars().count();
        if used_chars + chunk_chars > MAX_APPLIED_SKILL_CHARS {
            truncated = true;
            warnings
                .push("Selected skills were truncated to fit the skill prompt budget.".to_string());
            break;
        }
        used_chars += chunk_chars;
        body.push_str(&chunk);
        refs.push(AppliedSkillRef {
            id: record.id.clone(),
            name: record.name.clone(),
            category: record.category.clone(),
            source: match record.source {
                SkillSource::Managed => "managed",
                SkillSource::Imported => "imported",
                SkillSource::Discovered => "discovered",
            }
            .to_string(),
        });
    }

    if refs.is_empty() {
        body.clear();
    }
    AppliedSkillBlock {
        refs,
        text: body,
        warnings,
        truncated,
    }
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
        let dir = std::env::temp_dir().join(format!(
            "archify_skills_{}_{}_{}",
            label,
            std::process::id(),
            nanos
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn create_skill(
        store: &mut SkillStoreFile,
        library: &Path,
        name: &str,
        content: &str,
    ) -> SkillRecord {
        upsert_skill(
            store,
            library,
            SkillUpsertInput {
                name: Some(name.to_string()),
                description: Some(format!("{} description", name)),
                content: Some(content.to_string()),
                category: Some("engineering".to_string()),
                ..Default::default()
            },
        )
        .unwrap()
    }

    #[test]
    fn create_persist_toggle_delete_roundtrip() {
        let dir = temp_dir("roundtrip");
        let library = default_skill_library_dir(&dir);
        let path = default_skill_store_path(&dir);
        let mut store = load_skill_store(&path);
        let record = create_skill(
            &mut store,
            &library,
            "Test Skill",
            "# Procedure\nDo the thing.",
        );
        assert_eq!(record.name, "test-skill");
        assert!(record.managed);
        assert!(
            Path::new(record.directory.as_ref().unwrap())
                .join("SKILL.md")
                .is_file()
        );
        save_skill_store(&path, &store).unwrap();

        let mut reloaded = load_skill_store(&path);
        assert_eq!(reloaded.skills.len(), 1);
        refresh_validation(&mut reloaded, &library);
        assert!(
            reloaded.skills[0]
                .validation_issues
                .iter()
                .all(|i| i.level != "error")
        );

        let toggled = set_skill_enabled(&mut reloaded, &record.id, false).unwrap();
        assert!(!toggled.enabled);
        delete_skill(&mut reloaded, &library, &record.id).unwrap();
        assert!(reloaded.skills.is_empty());
        assert!(!Path::new(record.directory.as_ref().unwrap()).exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn frontmatter_roundtrip_preserves_extra_keys() {
        let dir = temp_dir("frontmatter");
        let library = default_skill_library_dir(&dir);
        let source = dir.join("source");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: Original\nmetadata:\n  author: Test\nallowed-tools: Read\n---\n\n# Demo\nBody\n",
        )
        .unwrap();

        let mut store = SkillStoreFile::default();
        let imported = import_skill(&mut store, &library, &source.to_string_lossy()).unwrap();
        assert_eq!(imported.source, SkillSource::Imported);
        let rendered =
            fs::read_to_string(Path::new(imported.directory.as_ref().unwrap()).join("SKILL.md"))
                .unwrap();
        assert!(rendered.contains("metadata:\n  author: Test"));
        assert!(rendered.contains("allowed-tools: Read"));
        assert!(rendered.contains("description: Original"));

        let updated = upsert_skill(
            &mut store,
            &library,
            SkillUpsertInput {
                id: Some(imported.id.clone()),
                description: Some("Updated description".to_string()),
                content: Some("New body".to_string()),
                ..Default::default()
            },
        )
        .unwrap();
        let rendered =
            fs::read_to_string(Path::new(updated.directory.as_ref().unwrap()).join("SKILL.md"))
                .unwrap();
        assert!(rendered.contains("description: Updated description"));
        assert!(rendered.contains("metadata:\n  author: Test"));
        assert!(rendered.contains("New body"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_and_duplicate_skills_are_rejected() {
        let dir = temp_dir("invalid");
        let library = default_skill_library_dir(&dir);
        let mut store = SkillStoreFile::default();
        let error = upsert_skill(
            &mut store,
            &library,
            SkillUpsertInput {
                name: Some("no-description".to_string()),
                description: Some("   ".to_string()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(error.contains("description"));

        create_skill(&mut store, &library, "duplicate", "body");
        let error = upsert_skill(
            &mut store,
            &library,
            SkillUpsertInput {
                name: Some("Duplicate".to_string()),
                description: Some("desc".to_string()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(error.contains("already exists"));

        let error = upsert_skill(
            &mut store,
            &library,
            SkillUpsertInput {
                name: Some("too-long".to_string()),
                description: Some("d".to_string()),
                content: Some("x".repeat(MAX_SKILL_CONTENT_CHARS + 1)),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(error.contains("exceeds"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn import_copies_auxiliary_files_and_rejects_duplicates() {
        let dir = temp_dir("import");
        let library = default_skill_library_dir(&dir);
        let source = dir
            .join("project")
            .join(".agents")
            .join("skills")
            .join("demo");
        fs::create_dir_all(source.join("scripts")).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\n---\n\nRun the script.\n",
        )
        .unwrap();
        fs::write(source.join("scripts").join("run.sh"), "echo hi").unwrap();

        let mut store = SkillStoreFile::default();
        let record = import_skill(&mut store, &library, &source.to_string_lossy()).unwrap();
        let managed_dir = PathBuf::from(record.directory.clone().unwrap());
        assert!(managed_dir.join("SKILL.md").is_file());
        assert!(managed_dir.join("scripts").join("run.sh").is_file());

        let error = import_skill(&mut store, &library, &source.to_string_lossy()).unwrap_err();
        assert!(error.contains("already been imported"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn discovered_skills_are_read_only() {
        let dir = temp_dir("discover");
        let source = dir.join(".agents").join("skills").join("demo");
        fs::create_dir_all(&source).unwrap();
        fs::write(
            source.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo skill\n---\n\nBody\n",
        )
        .unwrap();
        let found = discover_project_skills(&dir.to_string_lossy());
        assert_eq!(found.len(), 1);
        assert!(!found[0].managed);
        assert_eq!(found[0].source, SkillSource::Discovered);
        assert!(found[0].source_path.is_some());

        let mut store = SkillStoreFile {
            version: 1,
            skills: found,
        };
        let discovered_id = store.skills[0].id.clone();
        let error = delete_skill(&mut store, &dir, &discovered_id).unwrap_err();
        assert!(error.contains("cannot be deleted"));
        assert!(source.join("SKILL.md").is_file());

        let imported =
            import_skill(&mut store, &dir.join("library"), &source.to_string_lossy()).unwrap();
        assert!(imported.managed);
        assert_eq!(imported.source, SkillSource::Imported);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn resolution_requires_explicit_selection_and_honors_enabled_and_agent() {
        let dir = temp_dir("resolve");
        let library = default_skill_library_dir(&dir);
        let mut store = SkillStoreFile::default();
        let general = create_skill(&mut store, &library, "general", "General procedure");
        let mut targeted = create_skill(&mut store, &library, "targeted", "Targeted procedure");
        targeted.supported_agents = vec!["executor".to_string()];
        store.skills[1] = targeted.clone();
        let mut disabled = create_skill(&mut store, &library, "disabled", "Disabled procedure");
        disabled.enabled = false;
        store.skills[2] = disabled.clone();
        refresh_validation(&mut store, &library);

        let empty = resolve_skills_for_agent(&store, &[], Some("planner"));
        assert!(empty.text.is_empty());
        assert!(empty.refs.is_empty());

        let plan = resolve_skills_for_agent(
            &store,
            &[general.id.clone(), disabled.id.clone()],
            Some("planner"),
        );
        assert_eq!(plan.refs.len(), 1);
        assert!(plan.text.contains("General procedure"));
        assert!(plan.warnings.iter().any(|w| w.contains("disabled")));

        let exec = resolve_skills_for_agent(&store, &[targeted.id.clone()], Some("executor"));
        assert_eq!(exec.refs.len(), 1);
        let planner = resolve_skills_for_agent(&store, &[targeted.id.clone()], Some("planner"));
        assert!(planner.refs.is_empty());
        assert!(
            planner
                .warnings
                .iter()
                .any(|w| w.contains("does not target"))
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn malformed_store_and_frontmatter_are_reported_not_overwritten() {
        let dir = temp_dir("malformed");
        let path = default_skill_store_path(&dir);
        fs::write(&path, "{not json").unwrap();
        assert!(load_skill_store(&path).skills.is_empty());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{not json");

        let source = dir.join(".agents").join("skills").join("broken");
        fs::create_dir_all(&source).unwrap();
        fs::write(source.join("SKILL.md"), "# no frontmatter\n").unwrap();
        let found = discover_project_skills(&dir.to_string_lossy());
        assert_eq!(found.len(), 1);
        assert!(found[0].read_error.is_some());
        assert!(
            found[0]
                .validation_issues
                .iter()
                .any(|i| i.level == "error" && i.message.contains("frontmatter"))
        );
        let _ = fs::remove_dir_all(dir);
    }
}
