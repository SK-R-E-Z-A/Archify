use crate::models::PromptDelivery;
use crate::providers::resolve_executable;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;
use tokio::time::timeout;
use uuid::Uuid;

const REGISTRY_VERSION: u32 = 1;
const DETECTION_TIMEOUT_SECS: u64 = 5;
const MAX_VERSION_OUTPUT: usize = 4096;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentAvailability {
    Installed,
    Unavailable,
    Unsupported,
    DetectionError,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DetectionMethod {
    Auto,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    Known,
    Custom,
}

/// Registry entry for a CLI coding agent. Agent identity is separate from AI provider identity.
/// No credentials are stored here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisteredCodingAgent {
    pub id: String,
    /// Stable type id for known agents (e.g. "claude", "codex"); custom agents use "custom:<id>".
    pub agent_type: String,
    pub name: String,
    pub kind: AgentKind,
    pub executable: String,
    #[serde(default)]
    pub resolved_path: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    pub detection_method: DetectionMethod,
    pub availability: AgentAvailability,
    #[serde(default)]
    pub status_message: Option<String>,
    #[serde(default)]
    pub default_args: Vec<String>,
    #[serde(default)]
    pub prompt_delivery: PromptDelivery,
    /// Safe version-check args for known agents; never used for unknown/custom executables.
    #[serde(default)]
    pub version_args: Vec<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentRegistryFile {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub agents: Vec<RegisteredCodingAgent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManualAgentInput {
    pub id: Option<String>,
    pub name: Option<String>,
    pub executable: Option<String>,
    #[serde(default)]
    pub default_args: Option<Vec<String>>,
    #[serde(default)]
    pub prompt_delivery: Option<PromptDelivery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectionReport {
    pub agents: Vec<RegisteredCodingAgent>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Extensible descriptor for supported known CLI coding agents.
/// To add a new agent, append a descriptor here; detection/registry logic remains unchanged.
#[derive(Debug, Clone)]
pub struct KnownAgentDescriptor {
    pub agent_type: &'static str,
    pub name: &'static str,
    pub default_executable: &'static str,
    pub default_args: Vec<String>,
    pub prompt_delivery: PromptDelivery,
    pub version_args: Vec<&'static str>,
}

pub fn known_agent_descriptors() -> Vec<KnownAgentDescriptor> {
    // Keep in sync with provider presets conceptually, but registry owns detection metadata.
    vec![
        KnownAgentDescriptor {
            agent_type: "claude",
            name: "Claude Code",
            default_executable: "claude",
            default_args: vec![],
            prompt_delivery: PromptDelivery::PrefixedStdin,
            version_args: vec!["--version"],
        },
        KnownAgentDescriptor {
            agent_type: "codex",
            name: "OpenAI Codex CLI",
            default_executable: "codex",
            default_args: vec!["exec".into(), "-".into()],
            prompt_delivery: PromptDelivery::Stdin,
            version_args: vec!["--version"],
        },
        KnownAgentDescriptor {
            agent_type: "aider",
            name: "Aider",
            default_executable: "aider",
            default_args: vec!["--message".into()],
            prompt_delivery: PromptDelivery::Argument,
            version_args: vec!["--version"],
        },
        KnownAgentDescriptor {
            agent_type: "gemini",
            name: "Gemini CLI",
            default_executable: "gemini",
            default_args: vec![],
            prompt_delivery: PromptDelivery::PrefixedStdin,
            version_args: vec!["--version"],
        },
        KnownAgentDescriptor {
            agent_type: "opencode",
            name: "OpenCode",
            default_executable: "opencode",
            default_args: vec!["run".into()],
            prompt_delivery: PromptDelivery::Argument,
            version_args: vec!["--version"],
        },
        KnownAgentDescriptor {
            agent_type: "cursor-agent",
            name: "Cursor Agent CLI",
            default_executable: "cursor-agent",
            default_args: vec![],
            prompt_delivery: PromptDelivery::PrefixedStdin,
            version_args: vec!["--version"],
        },
        KnownAgentDescriptor {
            agent_type: "copilot",
            name: "GitHub Copilot CLI",
            default_executable: "gh",
            default_args: vec![
                "copilot".into(),
                "suggest".into(),
                "-t".into(),
                "shell".into(),
            ],
            prompt_delivery: PromptDelivery::Argument,
            version_args: vec!["--version"],
        },
        KnownAgentDescriptor {
            agent_type: "qwen-code",
            name: "Qwen Code CLI",
            default_executable: "qwen",
            default_args: vec![],
            prompt_delivery: PromptDelivery::PrefixedStdin,
            version_args: vec!["--version"],
        },
    ]
}

pub fn default_registry_path(base_dir: &Path) -> PathBuf {
    base_dir.join("coding_agents_registry.json")
}

pub fn load_registry(path: &Path) -> AgentRegistryFile {
    if !path.exists() {
        return AgentRegistryFile {
            version: REGISTRY_VERSION,
            agents: Vec::new(),
        };
    }
    match std::fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str::<AgentRegistryFile>(&content) {
            Ok(mut file) => {
                if file.version == 0 {
                    file.version = REGISTRY_VERSION;
                }
                file.agents
                    .retain(|a| !a.id.trim().is_empty() && !a.agent_type.trim().is_empty());
                file
            }
            Err(e) => {
                eprintln!("[agent_registry] malformed {}: {}", path.display(), e);
                AgentRegistryFile {
                    version: REGISTRY_VERSION,
                    agents: Vec::new(),
                }
            }
        },
        Err(e) => {
            eprintln!("[agent_registry] unreadable {}: {}", path.display(), e);
            AgentRegistryFile {
                version: REGISTRY_VERSION,
                agents: Vec::new(),
            }
        }
    }
}

fn write_json_atomic(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content)
        .map_err(|e| format!("Failed to write {}: {}", tmp.display(), e))?;
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(first) => {
            let _ = std::fs::remove_file(path);
            std::fs::rename(&tmp, path).map_err(|second| {
                let _ = std::fs::remove_file(&tmp);
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

pub fn save_registry(path: &Path, file: &AgentRegistryFile) -> Result<(), String> {
    let json = serde_json::to_string_pretty(file)
        .map_err(|e| format!("Failed to serialize registry: {}", e))?;
    write_json_atomic(path, &json)
}

fn normalize_executable_key(exe: &str) -> String {
    exe.trim()
        .trim_matches('"')
        .to_ascii_lowercase()
        .replace('/', "\\")
}

fn validate_manual_executable(executable: &str) -> Result<PathBuf, String> {
    let trimmed = executable.trim().trim_matches('"');
    if trimmed.is_empty() {
        return Err("Executable path or name is required.".to_string());
    }
    let path = Path::new(trimmed);
    let has_sep = trimmed.contains('/') || trimmed.contains('\\');
    if path.is_absolute() || has_sep {
        if !path.exists() {
            return Err(format!("Executable path does not exist: {}", trimmed));
        }
        if !path.is_file() {
            return Err(format!("Executable path is not a file: {}", trimmed));
        }
        return dunce::canonicalize(path)
            .map_err(|e| format!("Failed to resolve executable path '{}': {}", trimmed, e));
    }
    resolve_executable(trimmed)
        .ok_or_else(|| format!("Executable '{}' was not found on PATH.", trimmed))
}

async fn safe_version_check(
    resolved: &Path,
    version_args: &[&str],
) -> (Option<String>, Option<String>) {
    // Only called for known/safe descriptors or explicitly validated manual executables with known args.
    if version_args.is_empty() {
        return (None, None);
    }
    let mut cmd = Command::new(resolved);
    cmd.args(version_args);
    cmd.kill_on_drop(true);
    let result = timeout(Duration::from_secs(DETECTION_TIMEOUT_SECS), cmd.output()).await;
    match result {
        Err(_) => (
            None,
            Some(format!(
                "Version check timed out after {}s.",
                DETECTION_TIMEOUT_SECS
            )),
        ),
        Ok(Err(e)) => (None, Some(format!("Version check failed: {}", e))),
        Ok(Ok(out)) => {
            let mut text = String::new();
            text.push_str(&String::from_utf8_lossy(&out.stdout));
            if !text.trim().is_empty() && !out.stderr.is_empty() {
                text.push('\n');
            }
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            let compact = text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .take(2)
                .collect::<Vec<_>>()
                .join(" ");
            let bounded = if compact.len() > MAX_VERSION_OUTPUT {
                compact[..MAX_VERSION_OUTPUT].to_string()
            } else {
                compact
            };
            if bounded.trim().is_empty() {
                if out.status.success() {
                    (Some("version unavailable".to_string()), None)
                } else {
                    (
                        None,
                        Some(format!("Version check exited with status {}.", out.status)),
                    )
                }
            } else {
                (Some(bounded), None)
            }
        }
    }
}

pub async fn detect_known_agents() -> DetectionReport {
    let mut warnings = Vec::new();
    let mut agents = Vec::new();
    for d in known_agent_descriptors() {
        let now = Utc::now().to_rfc3339();
        let resolved = resolve_executable(d.default_executable);
        let (availability, resolved_path, version, status_message) = match resolved {
            None => (
                AgentAvailability::Unavailable,
                None,
                None,
                Some(format!("{} not found on PATH.", d.default_executable)),
            ),
            Some(path) => {
                let (ver, ver_err) = safe_version_check(&path, &d.version_args).await;
                (
                    AgentAvailability::Installed,
                    Some(path.to_string_lossy().to_string()),
                    ver,
                    ver_err,
                )
            }
        };
        if let AgentAvailability::DetectionError = availability {
            warnings.push(format!("{}: detection error", d.name));
        }
        agents.push(RegisteredCodingAgent {
            id: format!("known::{}", d.agent_type),
            agent_type: d.agent_type.to_string(),
            name: d.name.to_string(),
            kind: AgentKind::Known,
            executable: d.default_executable.to_string(),
            resolved_path,
            version,
            detection_method: DetectionMethod::Auto,
            availability,
            status_message,
            default_args: d.default_args.clone(),
            prompt_delivery: d.prompt_delivery,
            version_args: d.version_args.iter().map(|s| s.to_string()).collect(),
            created_at: now.clone(),
            updated_at: now,
        });
    }
    DetectionReport { agents, warnings }
}

fn is_duplicate(
    file: &AgentRegistryFile,
    agent_type: &str,
    executable: &str,
    ignore_id: Option<&str>,
) -> bool {
    let exe_key = normalize_executable_key(executable);
    file.agents.iter().any(|a| {
        if ignore_id.map(|id| id == a.id).unwrap_or(false) {
            return false;
        }
        a.agent_type.eq_ignore_ascii_case(agent_type)
            || normalize_executable_key(&a.executable) == exe_key
            || a.resolved_path
                .as_deref()
                .map(normalize_executable_key)
                .as_deref()
                == Some(exe_key.as_str())
    })
}

pub async fn upsert_manual_agent(
    path: &Path,
    input: ManualAgentInput,
) -> Result<RegisteredCodingAgent, String> {
    let mut file = load_registry(path);
    let now = Utc::now().to_rfc3339();
    let executable = input.executable.as_deref().unwrap_or("").trim().to_string();
    if executable.is_empty() {
        return Err("Executable path or name is required.".to_string());
    }
    let resolved = validate_manual_executable(&executable)?;
    let resolved_str = resolved.to_string_lossy().to_string();

    if let Some(id) = input.id.as_deref().filter(|s| !s.trim().is_empty()) {
        let idx = file
            .agents
            .iter()
            .position(|a| a.id == id)
            .ok_or_else(|| format!("Registered agent '{}' not found.", id))?;
        if is_duplicate(&file, &file.agents[idx].agent_type, &executable, Some(id)) {
            return Err(
                "Duplicate registration: an agent with the same type or executable already exists."
                    .to_string(),
            );
        }
        let mut rec = file.agents[idx].clone();
        rec.name = input
            .name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(&rec.name)
            .to_string();
        rec.executable = executable;
        rec.resolved_path = Some(resolved_str);
        rec.detection_method = DetectionMethod::Manual;
        rec.availability = AgentAvailability::Installed;
        rec.status_message = None;
        // Do not run arbitrary version checks for custom/manual executables.
        if rec.kind == AgentKind::Custom {
            rec.version = None;
        }
        if let Some(args) = input.default_args {
            rec.default_args = args;
        }
        if let Some(delivery) = input.prompt_delivery {
            rec.prompt_delivery = delivery;
        }
        rec.updated_at = now;
        file.agents[idx] = rec.clone();
        save_registry(path, &file)?;
        return Ok(rec);
    }

    let custom_type = format!(
        "custom:{}",
        normalize_executable_key(&executable)
            .replace('\\', "-")
            .replace(':', "")
    );
    if is_duplicate(&file, &custom_type, &executable, None) {
        return Err(
            "Duplicate registration: an agent with the same type or executable already exists."
                .to_string(),
        );
    }
    let name = input
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            Path::new(&executable)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Custom Agent")
                .to_string()
        });
    let rec = RegisteredCodingAgent {
        id: Uuid::new_v4().to_string(),
        agent_type: custom_type,
        name,
        kind: AgentKind::Custom,
        executable,
        resolved_path: Some(resolved_str),
        version: None,
        detection_method: DetectionMethod::Manual,
        availability: AgentAvailability::Installed,
        status_message: None,
        default_args: input.default_args.unwrap_or_default(),
        prompt_delivery: input.prompt_delivery.unwrap_or_default(),
        version_args: Vec::new(),
        created_at: now.clone(),
        updated_at: now,
    };
    file.agents.push(rec.clone());
    save_registry(path, &file)?;
    Ok(rec)
}

pub fn remove_registered_agent(path: &Path, id: &str) -> Result<(), String> {
    let mut file = load_registry(path);
    let before = file.agents.len();
    file.agents.retain(|a| a.id != id);
    if file.agents.len() == before {
        return Err(format!("Registered agent '{}' not found.", id));
    }
    save_registry(path, &file)
}

pub async fn refresh_registry(path: &Path) -> Result<AgentRegistryFile, String> {
    let mut file = load_registry(path);
    let detected = detect_known_agents().await;
    let now = Utc::now().to_rfc3339();

    // Update/insert known auto-detected agents by agent_type, preserving manual custom entries.
    for det in detected.agents {
        if let Some(existing) = file
            .agents
            .iter_mut()
            .find(|a| a.agent_type == det.agent_type)
        {
            existing.name = det.name;
            existing.executable = det.executable;
            existing.resolved_path = det.resolved_path;
            existing.version = det.version;
            existing.availability = det.availability;
            existing.status_message = det.status_message;
            existing.default_args = det.default_args;
            existing.prompt_delivery = det.prompt_delivery;
            existing.version_args = det.version_args;
            existing.updated_at = now.clone();
            if existing.detection_method != DetectionMethod::Manual {
                existing.detection_method = DetectionMethod::Auto;
            }
        } else {
            file.agents.push(det);
        }
    }

    // Revalidate manual/custom entries safely (path/existence only; no unknown execution).
    for agent in file.agents.iter_mut().filter(|a| {
        matches!(a.kind, AgentKind::Custom) || matches!(a.detection_method, DetectionMethod::Manual)
    }) {
        match validate_manual_executable(&agent.executable) {
            Ok(resolved) => {
                agent.resolved_path = Some(resolved.to_string_lossy().to_string());
                agent.availability = AgentAvailability::Installed;
                agent.status_message = None;
            }
            Err(e) => {
                agent.resolved_path = None;
                agent.availability = AgentAvailability::Unavailable;
                agent.status_message = Some(e);
                agent.version = None;
            }
        }
        agent.updated_at = now.clone();
    }

    // Deduplicate defensively by (agent_type, normalized executable).
    let mut seen: HashSet<String> = HashSet::new();
    file.agents.retain(|a| {
        let key = format!(
            "{}|{}",
            a.agent_type.to_ascii_lowercase(),
            normalize_executable_key(a.resolved_path.as_deref().unwrap_or(&a.executable))
        );
        seen.insert(key)
    });

    file.version = REGISTRY_VERSION;
    save_registry(path, &file)?;
    Ok(file)
}

pub async fn load_or_detect_registry(path: &Path) -> Result<AgentRegistryFile, String> {
    let file = load_registry(path);
    if file.agents.is_empty() {
        return refresh_registry(path).await;
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_load_registry_missing_file_returns_empty() {
        let tmp = std::env::temp_dir().join(format!("test_reg_missing_{}.json", Uuid::new_v4()));
        let file = load_registry(&tmp);
        assert_eq!(file.version, REGISTRY_VERSION);
        assert!(file.agents.is_empty());
    }

    #[tokio::test]
    async fn test_detect_known_agents_returns_descriptors() {
        let report = detect_known_agents().await;
        assert!(!report.agents.is_empty());
        let types: Vec<&str> = report
            .agents
            .iter()
            .map(|a| a.agent_type.as_str())
            .collect();
        assert!(types.contains(&"claude"));
        assert!(types.contains(&"aider"));
        assert!(types.contains(&"gemini"));
    }

    #[tokio::test]
    async fn test_upsert_manual_agent_rejects_empty_executable() {
        let tmp = std::env::temp_dir().join(format!("test_reg_empty_{}.json", Uuid::new_v4()));
        let input = ManualAgentInput {
            id: None,
            name: Some("Test".into()),
            executable: Some("".into()),
            default_args: None,
            prompt_delivery: None,
        };
        let res = upsert_manual_agent(&tmp, input).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("required"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn test_upsert_manual_agent_rejects_nonexistent_executable() {
        let tmp =
            std::env::temp_dir().join(format!("test_reg_nonexistent_{}.json", Uuid::new_v4()));
        let input = ManualAgentInput {
            id: None,
            name: Some("Test".into()),
            executable: Some("this_program_should_never_exist_xyz123".into()),
            default_args: None,
            prompt_delivery: None,
        };
        let res = upsert_manual_agent(&tmp, input).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("not found"));
        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn test_upsert_manual_agent_success_and_duplicate_prevention() {
        let tmp = std::env::temp_dir().join(format!("test_reg_crud_{}.json", Uuid::new_v4()));
        #[cfg(windows)]
        let exe = "cmd.exe";
        #[cfg(not(windows))]
        let exe = "sh";

        let input = ManualAgentInput {
            id: None,
            name: Some("My Shell".into()),
            executable: Some(exe.into()),
            default_args: Some(vec!["/c".into()]),
            prompt_delivery: Some(PromptDelivery::Stdin),
        };

        // First registration succeeds
        let res = upsert_manual_agent(&tmp, input.clone()).await;
        assert!(res.is_ok(), "Failed to register manual agent: {:?}", res);
        let agent = res.unwrap();
        assert_eq!(agent.name, "My Shell");
        assert_eq!(agent.kind, AgentKind::Custom);
        assert_eq!(agent.availability, AgentAvailability::Installed);
        assert!(
            agent.version.is_none(),
            "Custom agent must not run automatic version checks"
        );

        // Duplicate registration is rejected
        let res_dup = upsert_manual_agent(&tmp, input).await;
        assert!(res_dup.is_err());
        assert!(res_dup.unwrap_err().contains("Duplicate"));

        // Remove the agent
        let rem_res = remove_registered_agent(&tmp, &agent.id);
        assert!(rem_res.is_ok());

        // File now has 0 agents
        let file = load_registry(&tmp);
        assert!(file.agents.is_empty());

        let _ = std::fs::remove_file(&tmp);
    }

    #[tokio::test]
    async fn test_refresh_registry_persists_and_retains_custom_agents() {
        let tmp = std::env::temp_dir().join(format!("test_reg_refresh_{}.json", Uuid::new_v4()));
        #[cfg(windows)]
        let exe = "cmd.exe";
        #[cfg(not(windows))]
        let exe = "sh";

        let input = ManualAgentInput {
            id: None,
            name: Some("Custom Agent".into()),
            executable: Some(exe.into()),
            default_args: None,
            prompt_delivery: None,
        };
        let agent = upsert_manual_agent(&tmp, input).await.unwrap();

        // Refresh registry should detect known agents while retaining the custom agent
        let refreshed = refresh_registry(&tmp).await.unwrap();
        assert!(refreshed.agents.iter().any(|a| a.id == agent.id));
        assert!(refreshed.agents.iter().any(|a| a.agent_type == "claude"));

        // Clean up
        let _ = std::fs::remove_file(&tmp);
    }
}
