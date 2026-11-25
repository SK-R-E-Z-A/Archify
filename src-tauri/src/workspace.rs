//! Workspace layer: projects, provider profiles, task runtimes, scheduler, and
//! independent Prompt/CLI sessions.
//!
//! The single-task `Orchestrator` remains the execution engine. The workspace
//! owns one isolated `Orchestrator` per task so several tasks can plan, execute,
//! and be approved independently.

use crate::credentials::{CredentialStore, CredentialStoreError, default_credential_store};
use crate::model_selection::{
    AiResponsibility, ModelInfo, ModelPreferences, ModelProvider, ModelSelection, ModelSelector,
    ResponsibilityModelPreference,
};
use crate::models::{
    AvailabilityCheck, CliAgentPreset, CreateSessionOptions, LocalModelPreset, ProjectRecord,
    ProjectSummary, PromptDelivery, ProviderConfig, ProviderConfigInput, ProviderProfile,
    ProviderStatus, SessionKind, SessionMessage, SessionRecord, StartTaskOptions, TaskRecord,
    TaskState, WorkspaceSettings, WorkspaceSnapshot,
};
use crate::orchestrator::{
    self, Orchestrator, SharedOrchestrator, TaskCreationOptions, read_task_history,
};
use crate::providers::CodingExecutor;
use crate::providers::cloud_http::CloudHttpPlannerReviewer;
use crate::providers::presets::{cli_agent_presets, find_cli_preset, local_model_presets};
use crate::providers::process_cli::ProcessCliExecutor;
use crate::skills::{
    default_skill_library_dir, default_skill_store_path, load_skill_store, refresh_validation,
    resolve_skills_for_agent,
};
use crate::token_usage::{
    TokenUsage, TokenUsageRecord, TokenUsageStats, TokenUsageTracker, UsageContext,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use tauri::{AppHandle, Emitter};
use tokio::sync::{Mutex, watch};
use uuid::Uuid;

// Task-specific configuration overrides applied with highest precedence in the
// configuration hierarchy (Application defaults → Global → Project → Task).
#[derive(Clone, Default)]
pub struct TaskConfigOverrides {
    pub cli_executable: Option<String>,
    pub cli_args: Option<Vec<String>>,
    pub prompt_delivery: Option<PromptDelivery>,
}

impl From<&crate::models::StartTaskOptions> for TaskConfigOverrides {
    fn from(options: &crate::models::StartTaskOptions) -> Self {
        Self {
            cli_executable: options.cli_executable.clone(),
            cli_args: options.cli_args.clone(),
            prompt_delivery: options.prompt_delivery,
        }
    }
}

use crate::monitoring::MonitoringHub;
use crate::stall_watchdog;
// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

const WORKSPACE_FILE_VERSION: u32 = 2;

#[derive(Serialize, Deserialize, Default)]
struct WorkspaceFile {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    projects: Vec<ProjectRecord>,
    #[serde(default)]
    provider_profiles: Vec<ProviderProfile>,
    #[serde(default)]
    settings: WorkspaceSettings,
}

/// Migrate a loaded workspace file to the current schema version.
/// Older files without a `version` field deserialize to version 0 and are
/// upgraded in place.
fn migrate_workspace_file(file: &mut WorkspaceFile) {
    match file.version {
        0 | 1 => {
            // Version 0/1 (pre-migration): archived defaults to false via serde.
            // The `version` field is set to current on save.
            file.version = WORKSPACE_FILE_VERSION;
        }
        WORKSPACE_FILE_VERSION => {}
        other => {
            // Unknown future version: keep as-is and log. Forward compatibility
            // means we don't reject data written by a newer version.
            eprintln!(
                "[workspace] unknown workspace file version {}, proceeding with caution",
                other
            );
        }
    }
}

fn credential_store_error(error: CredentialStoreError) -> String {
    error.safe_message().to_string()
}

fn with_saved_credential_status(mut status: ProviderStatus, has_saved_key: bool) -> ProviderStatus {
    status.has_api_key |= has_saved_key;
    status.cloud_configured |= has_saved_key && status.cloud_endpoint.is_some();
    status
}

fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| format!("Failed to serialize {}: {}", path.display(), e))?;
    let tmp = path.with_extension("tmp");
    use std::io::Write;
    let write_result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(json.as_bytes())?;
        file.sync_all()
    })();
    if let Err(error) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Failed to write {}: {}", tmp.display(), error));
    }

    replace_file_with(&tmp, path, |from, to| std::fs::rename(from, to))
}

fn replace_file_with(
    tmp: &Path,
    path: &Path,
    mut rename: impl FnMut(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), String> {
    match rename(tmp, path) {
        Ok(()) => Ok(()),
        Err(first) => {
            if !path.exists() {
                let _ = std::fs::remove_file(tmp);
                return Err(format!("Failed to replace {}: {}", path.display(), first));
            }
            if path.is_dir() {
                let _ = std::fs::remove_file(tmp);
                return Err(format!(
                    "Refusing to replace directory {}: {}",
                    path.display(),
                    first
                ));
            }

            let backup = path.with_extension("bak");
            if backup.exists() {
                let _ = std::fs::remove_file(tmp);
                return Err(format!(
                    "Failed to replace {}: {} (preserving existing backup {})",
                    path.display(),
                    first,
                    backup.display()
                ));
            }
            if let Err(error) = rename(path, &backup) {
                let _ = std::fs::remove_file(tmp);
                return Err(format!(
                    "Failed to preserve {} before replacement: {} (original: {})",
                    path.display(),
                    error,
                    first
                ));
            }

            match rename(tmp, path) {
                Ok(()) => {
                    let _ = std::fs::remove_file(backup);
                    Ok(())
                }
                Err(replacement_error) => {
                    let restore = rename(&backup, path);
                    let _ = std::fs::remove_file(tmp);
                    match restore {
                        Ok(()) => Err(format!(
                            "Failed to replace {}: {} (original: {})",
                            path.display(),
                            replacement_error,
                            first
                        )),
                        Err(restore_error) => Err(format!(
                            "Failed to replace {}: {}; last valid data remains at {} (restore failed: {})",
                            path.display(),
                            replacement_error,
                            backup.display(),
                            restore_error
                        )),
                    }
                }
            }
        }
    }
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> Option<T> {
    let read = |candidate: &Path| {
        let content = std::fs::read_to_string(candidate).ok()?;
        serde_json::from_str(&content).ok()
    };
    read(path).or_else(|| read(&path.with_extension("bak")))
}

fn invalid_task_history_records(dir: &Path) -> Vec<String> {
    let mut diagnostics = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return diagnostics;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<unknown>");
        let has_valid_backup = || {
            let backup = path.with_extension("bak");
            let content = std::fs::read_to_string(backup).ok()?;
            let record = serde_json::from_str::<TaskRecord>(&content).ok()?;
            (!record.id.is_empty() && !record.user_task.trim().is_empty()).then_some(())
        };
        match std::fs::read_to_string(&path) {
            Ok(content) => match serde_json::from_str::<TaskRecord>(&content) {
                Ok(record) if !record.id.is_empty() && !record.user_task.trim().is_empty() => {}
                Ok(_) => diagnostics.push(format!(
                    "Task history record {} is incomplete and was skipped.",
                    name
                )),
                Err(error) => {
                    if has_valid_backup().is_some() {
                        diagnostics.push(format!(
                            "Task history primary {} is invalid; a backup is available for recovery: {}",
                            name, error
                        ));
                    } else {
                        diagnostics.push(format!(
                            "Task history record {} could not be parsed and was skipped: {}",
                            name, error
                        ));
                    }
                }
            },
            Err(error) => {
                if has_valid_backup().is_some() {
                    diagnostics.push(format!(
                        "Task history primary {} could not be read; a backup is available for recovery: {}",
                        name, error
                    ));
                } else {
                    diagnostics.push(format!(
                        "Task history record {} could not be read and was skipped: {}",
                        name, error
                    ));
                }
            }
        }
    }
    diagnostics
}

fn restore_task_baseline(orchestrator: &mut Orchestrator, record: &TaskRecord) {
    let Some(baseline) = record.task_baseline.as_ref() else {
        return;
    };
    orchestrator
        .task_baselines
        .entry(record.id.clone())
        .or_insert_with(|| {
            baseline
                .iter()
                .map(|(path, snapshot)| {
                    (
                        path.clone(),
                        crate::verification::FileSnapshot {
                            mtime_secs: snapshot.mtime_secs,
                            size: snapshot.size,
                            hash: snapshot.hash.clone(),
                        },
                    )
                })
                .collect()
        });
}

#[cfg(test)]
mod persistence_tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn failed_atomic_replacement_restores_the_original_file() {
        let dir = std::env::temp_dir().join(format!("workspace_atomic_replace_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("workspace.json");
        let tmp = path.with_extension("tmp");
        std::fs::write(&path, "last valid data").unwrap();
        std::fs::write(&tmp, "replacement").unwrap();

        let mut replace_attempts = 0;
        let result = replace_file_with(&tmp, &path, |from, to| {
            if from == tmp && to == path {
                replace_attempts += 1;
                if replace_attempts <= 2 {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "injected replacement failure",
                    ));
                }
            }
            std::fs::rename(from, to)
        });

        assert!(result.is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "last valid data");
        assert!(!tmp.exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn workspace_json_reads_backup_when_primary_is_missing_or_invalid() {
        let dir = std::env::temp_dir().join(format!("workspace_backup_read_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("workspace.json");
        let backup = path.with_extension("bak");
        let expected = serde_json::json!({"revision": 7});
        std::fs::write(&backup, serde_json::to_vec(&expected).unwrap()).unwrap();

        assert_eq!(read_json::<Value>(&path), Some(expected.clone()));
        std::fs::write(&path, "{ invalid").unwrap();
        assert_eq!(read_json::<Value>(&path), Some(expected));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn task_history_diagnostics_report_bad_records_without_hiding_valid_ones() {
        let dir =
            std::env::temp_dir().join(format!("workspace_history_diagnostics_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let valid = serde_json::json!({
            "id": "valid",
            "user_task": "task",
            "project_root": ".",
            "state": "completed",
            "plan": null,
            "iteration": 1,
            "max_iterations": 3,
            "iterations": [],
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "error": null
        });
        std::fs::write(dir.join("valid.json"), serde_json::to_vec(&valid).unwrap()).unwrap();
        std::fs::write(dir.join("broken.json"), "{ invalid").unwrap();

        let diagnostics = invalid_task_history_records(&dir);
        let history = read_task_history(&dir);

        assert_eq!(history.len(), 1);
        assert!(history.contains_key("valid"));
        assert_eq!(diagnostics.len(), 1);
        assert!(diagnostics[0].contains("broken.json"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn automatic_restart_recovery_requires_a_saved_plan() {
        let dir = std::env::temp_dir().join(format!(
            "workspace_recovery_without_plan_{}",
            Uuid::new_v4()
        ));
        let history_dir = dir.join("task_history");
        std::fs::create_dir_all(&history_dir).unwrap();
        std::fs::write(
            dir.join("workspace.json"),
            serde_json::to_vec(&serde_json::json!({
                "settings": {
                    "auto_retry_interrupted": true,
                    "max_recovery_attempts": 2
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            history_dir.join("interrupted.json"),
            serde_json::to_vec(&serde_json::json!({
                "id": "interrupted",
                "user_task": "task without a plan",
                "project_root": ".",
                "state": "executing",
                "plan": null,
                "task_graph": null,
                "iteration": 0,
                "max_iterations": 3,
                "iterations": [],
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "error": null,
                "recovery_attempts": 0
            }))
            .unwrap(),
        )
        .unwrap();

        let workspace = Workspace::new_with_paths(None, dir.clone());
        let recovered = workspace
            .task_status("interrupted")
            .await
            .unwrap()
            .expect("interrupted record should remain visible");

        assert_eq!(recovered.state, TaskState::Cancelled);
        assert_eq!(recovered.recovery_attempts, 0);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn automatic_restart_retry_restores_the_original_baseline_after_queueing() {
        let dir = std::env::temp_dir().join(format!("workspace_retry_baseline_{}", Uuid::new_v4()));
        let history_dir = dir.join("task_history");
        std::fs::create_dir_all(&history_dir).unwrap();
        std::fs::write(
            dir.join("workspace.json"),
            serde_json::to_vec(&serde_json::json!({
                "settings": {
                    "auto_retry_interrupted": true,
                    "max_recovery_attempts": 2
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            history_dir.join("interrupted.json"),
            serde_json::to_vec(&serde_json::json!({
                "id": "interrupted",
                "user_task": "resume with original baseline",
                "project_root": ".",
                "state": "executing",
                "plan": {
                    "summary": "saved plan",
                    "steps": ["step"],
                    "acceptance_criteria": []
                },
                "task_graph": null,
                "iteration": 1,
                "max_iterations": 3,
                "iterations": [],
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "error": null,
                "recovery_attempts": 0,
                "task_baseline": {
                    "before.txt": {
                        "mtime_secs": 123,
                        "size": 6,
                        "hash": "original-hash"
                    }
                }
            }))
            .unwrap(),
        )
        .unwrap();

        let workspace = Workspace::new_with_paths(None, dir.clone());
        let core = workspace.core.lock().await;
        let runtime = core.runtimes.get("interrupted").unwrap();
        let orchestrator = runtime.orchestrator.lock().await;

        assert_eq!(
            orchestrator.current_task.as_ref().unwrap().state,
            TaskState::Queued
        );
        assert_eq!(
            orchestrator.task_baselines["interrupted"]["before.txt"]
                .hash
                .as_deref(),
            Some("original-hash")
        );
        drop(orchestrator);
        drop(core);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn automatic_restart_recovery_stops_at_the_configured_attempt_limit() {
        let dir = std::env::temp_dir().join(format!("workspace_recovery_limit_{}", Uuid::new_v4()));
        let history_dir = dir.join("task_history");
        std::fs::create_dir_all(&history_dir).unwrap();
        std::fs::write(
            dir.join("workspace.json"),
            serde_json::to_vec(&serde_json::json!({
                "settings": {
                    "auto_retry_interrupted": true,
                    "max_recovery_attempts": 1
                }
            }))
            .unwrap(),
        )
        .unwrap();
        std::fs::write(
            history_dir.join("exhausted.json"),
            serde_json::to_vec(&serde_json::json!({
                "id": "exhausted",
                "user_task": "task at recovery limit",
                "project_root": ".",
                "state": "executing",
                "plan": {
                    "summary": "saved plan",
                    "steps": ["step"],
                    "acceptance_criteria": []
                },
                "task_graph": null,
                "iteration": 0,
                "max_iterations": 3,
                "iterations": [],
                "created_at": "2026-01-01T00:00:00Z",
                "updated_at": "2026-01-01T00:00:00Z",
                "error": null,
                "recovery_attempts": 1
            }))
            .unwrap(),
        )
        .unwrap();

        let workspace = Workspace::new_with_paths(None, dir.clone());
        let recovered = workspace
            .task_status("exhausted")
            .await
            .unwrap()
            .expect("interrupted record should remain visible");

        assert_eq!(recovered.state, TaskState::Cancelled);
        assert_eq!(recovered.recovery_attempts, 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn interrupted_task_baseline_survives_history_reload() {
        let dir =
            std::env::temp_dir().join(format!("workspace_persisted_baseline_{}", Uuid::new_v4()));
        let history_dir = dir.join("task_history");
        let config_path = dir.join("provider_config.json");
        std::fs::create_dir_all(&history_dir).unwrap();
        let record: TaskRecord = serde_json::from_value(serde_json::json!({
            "id": "baseline-task",
            "user_task": "persist baseline",
            "project_root": ".",
            "state": "executing",
            "plan": null,
            "task_graph": null,
            "iteration": 0,
            "max_iterations": 3,
            "iterations": [],
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "error": null
        }))
        .unwrap();
        let mut orchestrator = Orchestrator::with_task(
            None,
            history_dir.clone(),
            config_path.clone(),
            ProviderConfig::default(),
            record.clone(),
        );
        orchestrator.task_baselines.insert(
            "baseline-task".to_string(),
            [(
                "existing.txt".to_string(),
                crate::verification::FileSnapshot {
                    mtime_secs: 123,
                    size: 6,
                    hash: Some("baseline-hash".to_string()),
                },
            )]
            .into_iter()
            .collect(),
        );
        orchestrator.save_task_to_disk(&record);

        let restored = read_task_history(&history_dir);
        assert!(restored["baseline-task"].task_baseline.is_some());
        let reattached = Orchestrator::with_task(
            None,
            history_dir,
            config_path,
            ProviderConfig::default(),
            restored.get("baseline-task").unwrap().clone(),
        );
        let baseline = reattached.task_baselines.get("baseline-task").unwrap();

        assert_eq!(baseline["existing.txt"].size, 6);
        assert_eq!(
            baseline["existing.txt"].hash.as_deref(),
            Some("baseline-hash")
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn retry_task_preserves_task_model_id() {
        let dir = std::env::temp_dir().join(format!("workspace_retry_model_{}", Uuid::new_v4()));
        let history_dir = dir.join("task_history");
        std::fs::create_dir_all(&history_dir).unwrap();

        // Workspace settings: no auto-retry (we're testing explicit retry).
        std::fs::write(
            dir.join("workspace.json"),
            serde_json::to_vec(&serde_json::json!({
                "settings": {
                    "auto_retry_interrupted": false,
                    "max_recovery_attempts": 1
                }
            }))
            .unwrap(),
        )
        .unwrap();

        // A completed task with a saved plan and a task-level model override.
        let task_record = serde_json::json!({
            "id": "completed-task",
            "user_task": "a completed task",
            "project_root": ".",
            "state": "completed",
            "plan": {
                "summary": "test plan",
                "steps": ["step"],
                "acceptance_criteria": []
            },
            "task_graph": {
                "components": [{
                    "id": "step-1",
                    "description": "step",
                    "state": "completed",
                    "depends_on": [],
                    "acceptance_criteria": [],
                    "error": null
                }]
            },
            "iteration": 1,
            "max_iterations": 3,
            "iterations": [],
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "error": null,
            "task_model_id": "test-model-42"
        });
        std::fs::write(
            history_dir.join("completed-task.json"),
            serde_json::to_vec(&task_record).unwrap(),
        )
        .unwrap();

        let workspace = Workspace::new_with_paths(None, dir.clone());

        let result = workspace.retry_task("completed-task".to_string()).await;
        assert!(
            result.is_ok(),
            "retry_task should succeed: {:?}",
            result.err()
        );
        let retried = result.unwrap();

        // The task-level model override must be preserved on retry.
        assert_eq!(
            retried.task_model_id.as_deref(),
            Some("test-model-42"),
            "task_model_id should be preserved on retry"
        );

        // The retried task should be a fresh record (new id, iteration 0).
        assert_ne!(retried.id, "completed-task");
        // Resume sets iteration to 1 before execution begins.
        assert_eq!(retried.iteration, 1);
        assert_eq!(retried.recovery_attempts, 1);

        let _ = std::fs::remove_dir_all(dir);
    }
}

// ---------------------------------------------------------------------------
// Runtime state
// ---------------------------------------------------------------------------

pub struct TaskRuntime {
    pub orchestrator: SharedOrchestrator,
    pub project_id: Option<String>,
    pub priority: i32,
}

/// Runtime-only cancellation handle for the CLI turn currently running in a
/// session. The turn id prevents a late cleanup from removing the handle of a
/// newer turn that reused the same session id.
struct CliTurnHandle {
    turn_id: String,
    cancel_tx: watch::Sender<bool>,
}

pub struct WorkspaceCore {
    pub active_project_id: Option<String>,
    pub projects: Vec<ProjectRecord>,
    pub provider_profiles: Vec<ProviderProfile>,
    pub settings: WorkspaceSettings,
    pub sessions: Vec<SessionRecord>,
    pub runtimes: HashMap<String, TaskRuntime>,
    /// Sessions with a handoff currently being created. Runtime-only.
    pub handoffs_in_progress: HashSet<String>,
    /// Active CLI turn cancellation handles keyed by session id. Runtime-only.
    cli_turns: HashMap<String, CliTurnHandle>,
    /// Active prompt (HTTP streaming) turn cancellation handles keyed by session id. Runtime-only.
    prompt_turns: HashMap<String, CliTurnHandle>,
    pub history_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub workspace_path: PathBuf,
    pub config_path: PathBuf,
    pub app_handle: Option<AppHandle>,
    credential_store: Arc<dyn CredentialStore>,
    /// Model selection preferences and state
    pub model_selector: ModelSelector,
    /// Token usage tracker shared with active orchestrators so CLI-reported
    /// usage reaches the same aggregate view as provider-reported usage.
    pub token_usage_tracker: Arc<StdMutex<TokenUsageTracker>>,
    /// Shared monitoring hub for lifecycle and execution events. All tasks
    /// share one event stream with deduplication and history.
    pub monitoring_hub: MonitoringHub,
    /// Task IDs whose results have already been written to a source session,
    /// to avoid duplicate session messages.
    pub reported_task_results: HashSet<String>,
    /// Last observed task state per task, for state-change notification sync.
    pub last_task_states: HashMap<String, TaskState>,
    /// Persisted notification records.
    pub notification_path: PathBuf,
    pub notifications: Vec<crate::models::NotificationRecord>,
    pub notification_dedup: HashSet<String>,
    /// Persisted activity records.
    pub activity_path: PathBuf,
    pub activity: Vec<crate::models::ActivityRecord>,
    pub activity_dedup: HashSet<String>,
    /// Last monitoring sequence processed into notifications/activity.
    pub last_monitoring_sequence: u64,
}

impl WorkspaceCore {
    fn save(&self) {
        let file = WorkspaceFile {
            version: WORKSPACE_FILE_VERSION,
            projects: self.projects.clone(),
            provider_profiles: self.provider_profiles.clone(),
            settings: self.settings.clone(),
        };
        if let Err(e) = write_json_atomic(&self.workspace_path, &file) {
            self.emit_log(&format!("workspace: {}", e));
        }
    }

    fn save_session(&self, session: &SessionRecord) {
        if let Err(e) = std::fs::create_dir_all(&self.sessions_dir) {
            self.emit_log(&format!("sessions: cannot create directory: {}", e));
            return;
        }
        let mut persisted = session.clone();
        persisted.busy = false;
        let path = self.sessions_dir.join(format!("{}.json", session.id));
        if let Err(e) = write_json_atomic(&path, &persisted) {
            self.emit_log(&format!("sessions: {}", e));
        }
    }

    fn delete_session_file(&self, session_id: &str) {
        let path = self.sessions_dir.join(format!("{}.json", session_id));
        let _ = std::fs::remove_file(path);
    }

    /// Removes the cancellation handle only if it still belongs to the turn
    /// that just finished. This keeps natural completion and cancellation from
    /// deleting each other's state.
    fn finish_cli_turn(&mut self, session_id: &str, turn_id: &str) {
        let is_current = self
            .cli_turns
            .get(session_id)
            .map(|handle| handle.turn_id == turn_id)
            .unwrap_or(false);
        if is_current {
            self.cli_turns.remove(session_id);
        }
    }

    fn emit(&self, event: &str, payload: impl Serialize + Clone) {
        if let Some(ref handle) = self.app_handle {
            let _ = handle.emit(event, payload);
        }
    }

    fn emit_log(&self, message: &str) {
        if let Some(ref handle) = self.app_handle {
            let _ = handle.emit("orchestration_log", message);
        }
    }

    fn emit_workspace(&self) {
        self.emit("workspace_update", self.snapshot());
    }

    fn emit_session(&self, session: &SessionRecord) {
        self.emit("session_update", session.clone());
    }

    pub fn snapshot(&self) -> WorkspaceSnapshot {
        let mut active: Vec<String> = Vec::new();
        for runtime in self.runtimes.values() {
            if let Ok(guard) = runtime.orchestrator.try_lock() {
                if let Some(ref task) = guard.current_task {
                    if !task.state.is_terminal() {
                        active.push(task.id.clone());
                    }
                }
            } else {
                // Locked runtimes are actively updating, therefore active.
                active.push(String::new());
            }
        }
        active.retain(|id| !id.is_empty());

        let mut queued: Vec<TaskRecord> = Vec::new();
        for runtime in self.runtimes.values() {
            if let Ok(guard) = runtime.orchestrator.try_lock() {
                if let Some(ref task) = guard.current_task {
                    if task.state == TaskState::Queued {
                        queued.push(task.clone());
                    }
                }
            }
        }
        queued.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then_with(|| a.queued_at.cmp(&b.queued_at))
        });

        WorkspaceSnapshot {
            active_project_id: self.active_project_id.clone(),
            projects: self.projects.clone(),
            provider_profiles: self.provider_profiles.clone(),
            settings: self.settings.clone(),
            active_task_ids: active,
            queued_task_ids: queued.iter().map(|task| task.id.clone()).collect(),
        }
    }

    fn project(&self, project_id: &str) -> Option<&ProjectRecord> {
        self.projects
            .iter()
            .find(|project| project.id == project_id)
    }

    fn profile(&self, profile_id: &str) -> Option<&ProviderProfile> {
        self.provider_profiles
            .iter()
            .find(|profile| profile.id == profile_id)
    }

    /// Resolve the provider profile that a project/task should use.
    fn resolve_profile(
        &self,
        project_id: Option<&str>,
        profile_id: Option<&str>,
    ) -> Option<&ProviderProfile> {
        if let Some(id) = profile_id {
            if let Some(profile) = self.profile(id) {
                return Some(profile);
            }
        }
        if let Some(project_id) = project_id {
            if let Some(project) = self.project(project_id) {
                if let Some(ref id) = project.provider_profile_id {
                    if let Some(profile) = self.profile(id) {
                        return Some(profile);
                    }
                }
            }
            if let Some(profile) = self.provider_profiles.iter().find(|profile| {
                profile.project_id.as_deref() == Some(project_id) && profile.is_default
            }) {
                return Some(profile);
            }
        }
        if let Some(profile) = self
            .provider_profiles
            .iter()
            .find(|profile| profile.project_id.is_none() && profile.is_default)
        {
            return Some(profile);
        }
        self.provider_profiles
            .iter()
            .find(|profile| profile.project_id.is_none())
    }

    fn resolve_config(&self, project_id: Option<&str>, profile_id: Option<&str>) -> ProviderConfig {
        self.resolve_profile(project_id, profile_id)
            .map(|profile| profile.config.clone())
            .unwrap_or_default()
    }

    /// Resolve configuration using the full precedence hierarchy:
    ///
    /// Application defaults → Global user settings → Project settings →
    /// Task-specific overrides.
    ///
    /// `task_overrides` are task-specific settings from `StartTaskOptions`
    /// (cli_executable, cli_args, prompt_delivery) that take highest
    /// precedence. Returns the merged config and the resolved profile id.
    fn resolve_config_with_precedence(
        &self,
        project_id: Option<&str>,
        profile_id: Option<&str>,
        task_overrides: &TaskConfigOverrides,
    ) -> (ProviderConfig, Option<String>) {
        // Level 1: Application defaults
        let mut config = ProviderConfig::default();

        // Level 2: Global user settings (default profile with no project_id)
        // Level 3: Project settings (project's profile or project-scoped default)
        let resolved_profile = self.resolve_profile(project_id, profile_id);
        if let Some(profile) = resolved_profile {
            config = profile.config.clone();
        }

        // Level 4: Task-specific overrides (highest precedence)
        if let Some(ref exe) = task_overrides.cli_executable {
            let trimmed = exe.trim().to_string();
            if !trimmed.is_empty() {
                config.cli_executable = Some(trimmed);
                config.executor_type = "process_cli".to_string();
            }
        }
        if let Some(ref args) = task_overrides.cli_args {
            config.cli_args = if args.is_empty() {
                None
            } else {
                Some(args.clone())
            };
        }
        if let Some(delivery) = task_overrides.prompt_delivery {
            config.prompt_delivery = delivery;
        }

        let profile_id = resolved_profile.map(|p| p.id.clone());
        (config, profile_id)
    }

    fn profile_id_for(&self, project_id: Option<&str>, profile_id: Option<&str>) -> Option<String> {
        self.resolve_profile(project_id, profile_id)
            .map(|profile| profile.id.clone())
    }

    /// Legacy tasks created before the workspace existed have no project id.
    /// Treat them as belonging to the project with a matching root.
    fn task_belongs_to_project(&self, task: &TaskRecord, project_id: &str) -> bool {
        if task.project_id.as_deref() == Some(project_id) {
            return true;
        }
        if task.project_id.is_some() {
            return false;
        }
        self.project(project_id)
            .map(|project| project.root.eq_ignore_ascii_case(&task.project_root))
            .unwrap_or(false)
    }

    fn find_project_for_root(&self, root: &str) -> Option<&ProjectRecord> {
        self.projects
            .iter()
            .find(|project| project.root.eq_ignore_ascii_case(root))
    }
}

// ---------------------------------------------------------------------------
// Workspace
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct Workspace {
    pub core: Arc<Mutex<WorkspaceCore>>,
}

impl Workspace {
    pub async fn select_project(&self, project_id: String) -> Result<ProjectRecord, String> {
        let mut core = self.core.lock().await;
        if core.project(&project_id).is_none() {
            return Err("Project not found".to_string());
        }
        core.active_project_id = Some(project_id.clone());
        if let Some(project) = core.projects.iter_mut().find(|p| p.id == project_id) {
            project.last_opened_at = chrono::Utc::now().to_rfc3339();
        }
        let result = project_id.clone();
        core.save();
        core.emit_workspace();
        Ok(core
            .project(&result)
            .cloned()
            .ok_or_else(|| "Project not found".to_string())?)
    }

    pub fn new(app_handle: Option<AppHandle>) -> Self {
        let base = if let Some(ref handle) = app_handle {
            use tauri::Manager;
            handle
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
        } else {
            std::env::temp_dir()
                .join("archify-workspace")
                .join(Uuid::new_v4().to_string())
        };
        Self::new_with_credential_store(app_handle, base, default_credential_store())
    }

    pub fn new_with_paths(app_handle: Option<AppHandle>, base_dir: PathBuf) -> Self {
        Self::new_with_credential_store(app_handle, base_dir, default_credential_store())
    }

    pub fn new_with_credential_store(
        app_handle: Option<AppHandle>,
        base_dir: PathBuf,
        credential_store: Arc<dyn CredentialStore>,
    ) -> Self {
        let workspace_path = base_dir.join("workspace.json");
        let history_dir = base_dir.join("task_history");
        let sessions_dir = base_dir.join("sessions");
        let config_path = base_dir.join("provider_config.json");
        let notification_path = base_dir.join("notifications.json");
        let activity_path = base_dir.join("activity.json");

        // Aggregate token usage is loaded once and shared with every active
        // orchestrator, so CLI-reported executions contribute to the same
        // persistent statistics as provider calls.
        let mut loaded_usage = TokenUsageTracker::new(base_dir.join("token_usage.json"));
        if let Err(e) = loaded_usage.load() {
            if let Some(handle) = app_handle.as_ref() {
                let _ = handle.emit("orchestration_log", format!("token_usage: {}", e));
            }
        }
        let token_usage_tracker = Arc::new(StdMutex::new(loaded_usage));

        // Shared monitoring hub for lifecycle and execution events. All tasks
        // share one event stream with deduplication and history.
        let monitoring_hub = MonitoringHub::from_app_handle(app_handle.clone());
        let _ = monitoring_hub.set_persist_path(Some(history_dir.join("monitoring_events.jsonl")));

        let mut file: WorkspaceFile = read_json(&workspace_path).unwrap_or_default();
        migrate_workspace_file(&mut file);
        let mut projects = std::mem::take(&mut file.projects);
        let mut provider_profiles = std::mem::take(&mut file.provider_profiles);
        let settings = file.settings;

        let _ = std::fs::create_dir_all(&history_dir);
        let _ = std::fs::create_dir_all(&sessions_dir);

        // Migrate the single-task app: reuse the legacy provider config as the
        // default profile and discover existing task roots as projects.
        if provider_profiles.is_empty() {
            let legacy: Option<ProviderConfig> = read_json(&config_path);
            let now = chrono::Utc::now().to_rfc3339();
            provider_profiles.push(ProviderProfile {
                id: Uuid::new_v4().to_string(),
                name: "Default".to_string(),
                project_id: None,
                is_default: true,
                config: legacy.unwrap_or_default(),
                created_at: now.clone(),
                updated_at: now,
            });
        }

        for diagnostic in invalid_task_history_records(&history_dir) {
            if let Some(handle) = app_handle.as_ref() {
                let _ = handle.emit("orchestration_log", diagnostic);
            }
        }
        let history = read_task_history(&history_dir);
        if projects.is_empty() {
            let now = chrono::Utc::now().to_rfc3339();
            let mut seen: Vec<String> = Vec::new();
            for record in history.values() {
                if record.project_root.trim().is_empty()
                    || seen
                        .iter()
                        .any(|root| root.eq_ignore_ascii_case(&record.project_root))
                {
                    continue;
                }
                seen.push(record.project_root.clone());
                projects.push(ProjectRecord {
                    id: Uuid::new_v4().to_string(),
                    name: project_name_from_root(&record.project_root),
                    root: record.project_root.clone(),
                    provider_profile_id: None,
                    created_at: now.clone(),
                    last_opened_at: now.clone(),
                    pinned: false,
                    archived: false,
                    description: None,
                    tags: Vec::new(),
                });
            }
        }

        let sessions = load_sessions(&sessions_dir);
        let loaded_notifications: Vec<crate::models::NotificationRecord> =
            read_json(&notification_path).unwrap_or_default();
        let loaded_activity: Vec<crate::models::ActivityRecord> =
            read_json(&activity_path).unwrap_or_default();
        let mut notification_dedup: HashSet<String> = HashSet::new();
        for n in &loaded_notifications {
            if let Some(k) = &n.dedup_key {
                notification_dedup.insert(k.clone());
            }
        }
        let mut activity_dedup: HashSet<String> = HashSet::new();
        for a in &loaded_activity {
            if let Some(k) = &a.dedup_key {
                activity_dedup.insert(k.clone());
            }
        }

        // Crash recovery: reattach queued/awaiting tasks and decide what happens
        // to tasks that were mid-flight when the app stopped.
        let mut runtimes: HashMap<String, TaskRuntime> = HashMap::new();
        for (_, mut record) in history {
            match record.state {
                TaskState::Queued | TaskState::AwaitingApproval => {
                    let config = find_profile_config(
                        &provider_profiles,
                        &projects,
                        record.project_id.as_deref(),
                        record.provider_profile_id.as_deref(),
                    );
                    let mut orchestrator = Orchestrator::with_task(
                        app_handle.clone(),
                        history_dir.clone(),
                        config_path.clone(),
                        config,
                        record.clone(),
                    );
                    orchestrator.set_token_usage_tracker(Some(token_usage_tracker.clone()));
                    orchestrator.set_monitoring_hub(monitoring_hub.clone());
                    restore_task_baseline(&mut orchestrator, &record);
                    let orchestrator = Arc::new(Mutex::new(orchestrator));
                    runtimes.insert(
                        record.id.clone(),
                        TaskRuntime {
                            orchestrator,
                            project_id: record.project_id.clone(),
                            priority: record.priority,
                        },
                    );
                }
                state if state.is_interrupted_by_restart() => {
                    let can_retry = settings.auto_retry_interrupted
                        && record.plan.is_some()
                        && record.recovery_attempts < settings.max_recovery_attempts;
                    if can_retry {
                        record.recovery_attempts += 1;
                        if let Some(ref mut graph) = record.task_graph {
                            graph.reset_in_flight();
                        }
                        record.state = TaskState::Queued;
                        record.queued_at = Some(chrono::Utc::now().to_rfc3339());
                        record.updated_at = chrono::Utc::now().to_rfc3339();
                        record.error = Some(
                            "Recovered automatically after an application restart.".to_string(),
                        );
                    } else {
                        record.state = TaskState::Cancelled;
                        record.updated_at = chrono::Utc::now().to_rfc3339();
                        record.error = Some(
                            "Task was interrupted by an application restart. Retry it from the \
                             task history to resume."
                                .to_string(),
                        );
                    }
                    let writer =
                        Orchestrator::new_isolated(None, history_dir.clone(), config_path.clone());
                    writer.save_task_to_disk(&record);

                    if can_retry {
                        let config = find_profile_config(
                            &provider_profiles,
                            &projects,
                            record.project_id.as_deref(),
                            record.provider_profile_id.as_deref(),
                        );
                        let mut orchestrator = Orchestrator::with_task(
                            app_handle.clone(),
                            history_dir.clone(),
                            config_path.clone(),
                            config,
                            record.clone(),
                        );
                        orchestrator.set_token_usage_tracker(Some(token_usage_tracker.clone()));
                        orchestrator.set_monitoring_hub(monitoring_hub.clone());
                        restore_task_baseline(&mut orchestrator, &record);
                        let orchestrator = Arc::new(Mutex::new(orchestrator));
                        runtimes.insert(
                            record.id.clone(),
                            TaskRuntime {
                                orchestrator,
                                project_id: record.project_id.clone(),
                                priority: record.priority,
                            },
                        );
                    }
                }
                _ => {}
            }
        }

        // Initialize model selector with built-in providers
        let mut model_selector = ModelSelector::new();
        for provider in crate::model_selection::create_builtin_model_providers() {
            let provider: &dyn ModelProvider = &provider;
            model_selector.add_provider_models(
                provider.provider_id().to_string(),
                provider.available_models(),
            );
        }

        let core = WorkspaceCore {
            active_project_id: None,
            projects,
            provider_profiles,
            settings,
            sessions,
            runtimes,
            handoffs_in_progress: HashSet::new(),
            cli_turns: HashMap::new(),
            prompt_turns: HashMap::new(),
            history_dir,
            sessions_dir,
            workspace_path,
            config_path,
            app_handle,
            credential_store,
            model_selector,
            token_usage_tracker,
            monitoring_hub,
            reported_task_results: HashSet::new(),
            last_task_states: HashMap::new(),
            notification_path,
            notifications: loaded_notifications,
            notification_dedup,
            activity_path,
            activity: loaded_activity,
            activity_dedup,
            last_monitoring_sequence: 0,
        };

        if !core.workspace_path.exists() {
            core.save();
        }

        Self {
            core: Arc::new(Mutex::new(core)),
        }
    }

    // -----------------------------------------------------------------------
    // Workspace snapshot / settings
    // -----------------------------------------------------------------------

    pub async fn snapshot(&self) -> WorkspaceSnapshot {
        self.core.lock().await.snapshot()
    }

    pub async fn update_settings(
        &self,
        settings: WorkspaceSettings,
    ) -> Result<WorkspaceSnapshot, String> {
        let mut core = self.core.lock().await;
        core.settings = WorkspaceSettings {
            max_concurrent_tasks: settings.max_concurrent_tasks.clamp(1, 16),
            max_concurrent_sessions: settings.max_concurrent_sessions.clamp(1, 32),
            scheduler_paused: settings.scheduler_paused,
            auto_retry_interrupted: settings.auto_retry_interrupted,
            max_recovery_attempts: settings.max_recovery_attempts.min(10),
        };
        core.save();
        let snapshot = core.snapshot();
        core.emit_workspace();
        Ok(snapshot)
    }

    // -----------------------------------------------------------------------
    // Projects
    // -----------------------------------------------------------------------

    pub async fn add_project(
        &self,
        root: String,
        name: Option<String>,
    ) -> Result<ProjectRecord, String> {
        let root_path = Path::new(&root);
        if !root_path.exists() {
            return Err(format!("Project root does not exist: {}", root));
        }
        let canonical = dunce::canonicalize(root_path)
            .map_err(|e| format!("Failed to canonicalize project root: {}", e))?;
        if !canonical.is_dir() {
            return Err(format!("Project root is not a directory: {}", root));
        }
        let canonical_str = canonical.to_string_lossy().to_string();

        let mut core = self.core.lock().await;
        if core.find_project_for_root(&canonical_str).is_some() {
            return Err("That folder is already added as a project.".to_string());
        }
        let now = chrono::Utc::now().to_rfc3339();
        let project = ProjectRecord {
            id: Uuid::new_v4().to_string(),
            name: name
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| project_name_from_root(&canonical_str)),
            root: canonical_str,
            provider_profile_id: None,
            created_at: now.clone(),
            last_opened_at: now,
            pinned: false,
            archived: false,
            description: None,
            tags: Vec::new(),
        };
        core.projects.push(project.clone());
        core.save();
        core.emit_workspace();
        Ok(project)
    }

    pub async fn update_project(
        &self,
        project_id: String,
        name: Option<String>,
        provider_profile_id: Option<String>,
        pinned: Option<bool>,
        description: Option<String>,
        tags: Option<Vec<String>>,
    ) -> Result<ProjectRecord, String> {
        let mut core = self.core.lock().await;
        if let Some(ref id) = provider_profile_id {
            if !id.is_empty() && core.profile(id).is_none() {
                return Err("Provider profile not found".to_string());
            }
        }
        let project = core
            .projects
            .iter_mut()
            .find(|project| project.id == project_id)
            .ok_or_else(|| "Project not found".to_string())?;
        if let Some(name) = name {
            let trimmed = name.trim().to_string();
            if !trimmed.is_empty() {
                project.name = trimmed;
            }
        }
        if let Some(profile_id) = provider_profile_id {
            project.provider_profile_id = if profile_id.is_empty() {
                None
            } else {
                Some(profile_id)
            };
        }
        if let Some(pinned) = pinned {
            project.pinned = pinned;
        }
        if let Some(description_value) = description {
            project.description = if description_value.trim().is_empty() {
                None
            } else {
                Some(description_value.trim().to_string())
            };
        }
        if let Some(tags_value) = tags {
            project.tags = tags_value;
        }
        project.last_opened_at = chrono::Utc::now().to_rfc3339();
        let updated = project.clone();
        core.save();
        core.emit_workspace();
        Ok(updated)
    }

    pub async fn remove_project(&self, project_id: String) -> Result<(), String> {
        let mut core = self.core.lock().await;
        if core.project(&project_id).is_none() {
            return Err("Project not found".to_string());
        }
        let has_active = core.runtimes.values().any(|runtime| {
            if runtime.project_id.as_deref() != Some(project_id.as_str()) {
                return false;
            }
            match runtime.orchestrator.try_lock() {
                Ok(guard) => guard
                    .current_task
                    .as_ref()
                    .map(|task| !task.state.is_terminal())
                    .unwrap_or(false),
                Err(_) => true,
            }
        });
        if has_active {
            return Err(
                "Cancel or complete this project's tasks before removing the project.".to_string(),
            );
        }
        let profile_ids: Vec<String> = core
            .provider_profiles
            .iter()
            .filter(|profile| profile.project_id.as_deref() == Some(project_id.as_str()))
            .map(|profile| profile.id.clone())
            .collect();
        let saved_credentials: Vec<(String, Option<String>)> = profile_ids
            .iter()
            .map(|id| {
                core.credential_store
                    .retrieve(id)
                    .map(|secret| (id.clone(), secret))
                    .map_err(credential_store_error)
            })
            .collect::<Result<_, _>>()?;
        let mut deleted_credentials: Vec<(String, String)> = Vec::new();
        for (profile_id, secret) in &saved_credentials {
            if let Err(error) = core.credential_store.delete(profile_id) {
                for (deleted_id, deleted_secret) in deleted_credentials.iter().rev() {
                    if let Err(restore_error) =
                        core.credential_store.store(deleted_id, deleted_secret)
                    {
                        core.emit_log(restore_error.safe_message());
                    }
                }
                return Err(credential_store_error(error));
            }
            if let Some(secret) = secret {
                deleted_credentials.push((profile_id.clone(), secret.clone()));
            }
        }
        let before = core.projects.len();
        core.projects.retain(|project| project.id != project_id);
        if core.projects.len() == before {
            return Err("Project not found".to_string());
        }
        // Project-scoped profiles go with the project.
        core.provider_profiles
            .retain(|profile| profile.project_id.as_deref() != Some(project_id.as_str()));
        core.save();
        core.emit_workspace();
        drop(core);
        self.clear_cached_profile_credentials(&profile_ids).await;
        Ok(())
    }

    /// Detect registered projects whose root directory no longer exists on disk.
    /// Returns the ids of projects whose root is missing. This does not mutate
    /// state; callers decide whether to re-point or remove them.
    pub async fn detect_missing_project_roots(&self) -> Vec<String> {
        let core = self.core.lock().await;
        core.projects
            .iter()
            .filter(|p| !std::path::Path::new(&p.root).exists())
            .map(|p| p.id.clone())
            .collect()
    }

    // -----------------------------------------------------------------------
    // Project lifecycle helpers
    // -----------------------------------------------------------------------

    /// Alias for `add_project`, kept for callers that prefer a "create" verb.
    pub async fn create_project(
        &self,
        root: String,
        name: Option<String>,
    ) -> Result<ProjectRecord, String> {
        // Create the directory if it does not yet exist, so a brand-new project
        // folder can be registered. Existing directories are reused.
        let path = Path::new(&root);
        if !path.exists() {
            std::fs::create_dir_all(path)
                .map_err(|e| format!("Failed to create project directory: {}", e))?;
        }
        self.add_project(root, name).await
    }

    pub async fn register_project(
        &self,
        root: String,
        name: Option<String>,
    ) -> Result<ProjectRecord, String> {
        self.add_project(root, name).await
    }

    pub async fn new_project(
        &self,
        root: String,
        name: Option<String>,
    ) -> Result<ProjectRecord, String> {
        self.create_project(root, name).await
    }

    pub async fn get_project(&self, project_id: String) -> Result<Option<ProjectRecord>, String> {
        let core = self.core.lock().await;
        Ok(core.project(&project_id).cloned())
    }

    pub async fn project(&self, project_id: String) -> Option<ProjectRecord> {
        let core = self.core.lock().await;
        core.project(&project_id).cloned()
    }

    pub async fn find_project(&self, project_id: String) -> Option<ProjectRecord> {
        self.project(project_id).await
    }

    pub async fn list_projects(&self) -> Result<Vec<ProjectRecord>, String> {
        let core = self.core.lock().await;
        Ok(core.projects.clone())
    }

    pub async fn list_all_projects(&self) -> Vec<ProjectRecord> {
        let core = self.core.lock().await;
        core.projects.clone()
    }

    pub async fn list_active_projects(&self) -> Vec<ProjectRecord> {
        let core = self.core.lock().await;
        core.projects
            .iter()
            .filter(|p| !p.archived)
            .cloned()
            .collect()
    }

    pub async fn list_archived_projects(&self) -> Vec<ProjectRecord> {
        let core = self.core.lock().await;
        core.projects
            .iter()
            .filter(|p| p.archived)
            .cloned()
            .collect()
    }

    pub async fn search_projects(&self, query: String) -> Vec<ProjectRecord> {
        let needle = query.to_lowercase();
        let core = self.core.lock().await;
        core.projects
            .iter()
            .filter(|p| {
                needle.is_empty()
                    || p.name.to_lowercase().contains(&needle)
                    || p.root.to_lowercase().contains(&needle)
                    || p.id.to_lowercase().contains(&needle)
            })
            .cloned()
            .collect()
    }

    pub async fn filter_projects(&self, query: String) -> Vec<ProjectRecord> {
        self.search_projects(query).await
    }

    /// Archive or unarchive a project registration. Archived projects are kept
    /// for history but cannot receive new tasks.
    pub async fn archive_project(
        &self,
        project_id: String,
        archived: bool,
    ) -> Result<ProjectRecord, String> {
        self.set_project_archived(project_id, archived).await
    }

    pub async fn unarchive_project(&self, project_id: String) -> Result<ProjectRecord, String> {
        self.set_project_archived(project_id, false).await
    }

    pub async fn set_project_archived(
        &self,
        project_id: String,
        archived: bool,
    ) -> Result<ProjectRecord, String> {
        let mut core = self.core.lock().await;
        // Prevent archiving while tasks are active.
        if archived {
            let has_active = core.runtimes.values().any(|runtime| {
                if runtime.project_id.as_deref() != Some(project_id.as_str()) {
                    return false;
                }
                match runtime.orchestrator.try_lock() {
                    Ok(guard) => guard
                        .current_task
                        .as_ref()
                        .map(|task| !task.state.is_terminal())
                        .unwrap_or(false),
                    Err(_) => true,
                }
            });
            if has_active {
                return Err("Cancel or complete this project's tasks before archiving.".to_string());
            }
        }
        let project = core
            .projects
            .iter_mut()
            .find(|p| p.id == project_id)
            .ok_or_else(|| "Project not found".to_string())?;
        project.archived = archived;
        project.last_opened_at = chrono::Utc::now().to_rfc3339();
        let updated = project.clone();
        core.save();
        core.emit_workspace();
        Ok(updated)
    }

    pub async fn update_project_root(
        &self,
        project_id: String,
        new_root: String,
    ) -> Result<ProjectRecord, String> {
        let new_path = Path::new(&new_root);
        if !new_path.exists() {
            return Err(format!("Project root does not exist: {}", new_root));
        }
        let canonical = dunce::canonicalize(new_path)
            .map_err(|e| format!("Failed to canonicalize project root: {}", e))?;
        if !canonical.is_dir() {
            return Err(format!("Project root is not a directory: {}", new_root));
        }
        let canonical_str = canonical.to_string_lossy().to_string();

        let mut core = self.core.lock().await;
        // Duplicate detection against other projects.
        if core
            .projects
            .iter()
            .any(|p| p.id != project_id && p.root.eq_ignore_ascii_case(&canonical_str))
        {
            return Err("That folder is already added as a project.".to_string());
        }
        // Prevent moving while tasks are active.
        let has_active = core.runtimes.values().any(|runtime| {
            if runtime.project_id.as_deref() != Some(project_id.as_str()) {
                return false;
            }
            match runtime.orchestrator.try_lock() {
                Ok(guard) => guard
                    .current_task
                    .as_ref()
                    .map(|task| !task.state.is_terminal())
                    .unwrap_or(false),
                Err(_) => true,
            }
        });
        if has_active {
            return Err(
                "Cancel or complete this project's tasks before changing its root.".to_string(),
            );
        }
        let project = core
            .projects
            .iter_mut()
            .find(|p| p.id == project_id)
            .ok_or_else(|| "Project not found".to_string())?;
        project.root = canonical_str;
        project.last_opened_at = chrono::Utc::now().to_rfc3339();
        let updated = project.clone();
        core.save();
        core.emit_workspace();
        Ok(updated)
    }

    pub async fn change_project_root(
        &self,
        project_id: String,
        new_root: String,
    ) -> Result<ProjectRecord, String> {
        self.update_project_root(project_id, new_root).await
    }

    pub async fn move_project(
        &self,
        project_id: String,
        new_root: String,
    ) -> Result<ProjectRecord, String> {
        self.update_project_root(project_id, new_root).await
    }

    pub async fn set_project_root(
        &self,
        project_id: String,
        new_root: String,
    ) -> Result<ProjectRecord, String> {
        self.update_project_root(project_id, new_root).await
    }

    /// Update project display name only; never moves the directory.
    pub async fn rename_project(
        &self,
        project_id: String,
        name: String,
    ) -> Result<ProjectRecord, String> {
        self.update_project(project_id, Some(name), None, None, None, None)
            .await
    }

    pub async fn update_project_metadata(
        &self,
        project_id: String,
        name: Option<String>,
        pinned: Option<bool>,
        provider_profile_id: Option<String>,
    ) -> Result<ProjectRecord, String> {
        self.update_project(project_id, name, provider_profile_id, pinned, None, None)
            .await
    }

    /// Validation helpers.
    pub async fn project_exists(&self, project_id: &str) -> bool {
        let core = self.core.lock().await;
        core.project(project_id).is_some()
    }

    pub async fn validate_project(&self, project_id: &str) -> Result<(), String> {
        let core = self.core.lock().await;
        if core.project(project_id).is_none() {
            return Err("Project not found".to_string());
        }
        Ok(())
    }

    pub async fn validate_project_root(&self, root: &str) -> Result<(), String> {
        let path = Path::new(root);
        if !path.exists() {
            return Err(format!("Project root does not exist: {}", root));
        }
        let canonical = dunce::canonicalize(path)
            .map_err(|e| format!("Failed to canonicalize project root: {}", e))?;
        if !canonical.is_dir() {
            return Err(format!("Project root is not a directory: {}", root));
        }
        Ok(())
    }

    pub async fn is_project_archived(&self, project_id: &str) -> bool {
        let core = self.core.lock().await;
        core.project(project_id)
            .map(|p| p.archived)
            .unwrap_or(false)
    }

    pub async fn project_health(&self, project_id: &str) -> String {
        let core = self.core.lock().await;
        match core.project(project_id) {
            None => "missing".to_string(),
            Some(p) => {
                if !Path::new(&p.root).exists() {
                    "unavailable".to_string()
                } else if p.archived {
                    "archived".to_string()
                } else {
                    "healthy".to_string()
                }
            }
        }
    }

    pub async fn project_status(&self, project_id: &str) -> String {
        self.project_health(project_id).await
    }

    /// Ownership validation for cross-project references.
    pub async fn validate_task_project(
        &self,
        task_id: &str,
        project_id: &str,
    ) -> Result<(), String> {
        let core = self.core.lock().await;
        // Find task
        let task = if let Some(runtime) = core.runtimes.get(task_id) {
            runtime
                .orchestrator
                .try_lock()
                .ok()
                .and_then(|g| g.current_task.clone())
        } else {
            None
        };
        let task = match task {
            Some(t) => Some(t),
            None => read_task_history(&core.history_dir).get(task_id).cloned(),
        };
        let Some(task) = task else {
            return Err("Task not found".to_string());
        };
        if let Some(tid) = task.project_id.as_deref() {
            if tid != project_id {
                return Err("Task does not belong to this project".to_string());
            }
        } else {
            // Legacy: match by root
            if let Some(project) = core.project(project_id) {
                if !task.project_root.eq_ignore_ascii_case(&project.root) {
                    return Err("Task does not belong to this project".to_string());
                }
            } else {
                return Err("Project not found".to_string());
            }
        }
        Ok(())
    }

    pub async fn task_belongs_to_project_pub(&self, task_id: &str, project_id: &str) -> bool {
        self.validate_task_project(task_id, project_id)
            .await
            .is_ok()
    }

    pub async fn task_belongs_to_project(&self, task_id: &str, project_id: &str) -> bool {
        self.task_belongs_to_project_pub(task_id, project_id).await
    }

    pub async fn validate_cross_project_reference(
        &self,
        task_id: &str,
        project_id: &str,
    ) -> Result<(), String> {
        self.validate_task_project(task_id, project_id).await
    }

    pub async fn ensure_task_belongs_to_project(
        &self,
        task_id: &str,
        project_id: &str,
    ) -> Result<(), String> {
        self.validate_task_project(task_id, project_id).await
    }

    pub async fn task_project_id(&self, task_id: &str) -> Option<String> {
        let core = self.core.lock().await;
        let task = if let Some(runtime) = core.runtimes.get(task_id) {
            runtime
                .orchestrator
                .try_lock()
                .ok()
                .and_then(|g| g.current_task.clone())
        } else {
            None
        };
        let task = task.or_else(|| read_task_history(&core.history_dir).get(task_id).cloned());
        task.and_then(|t| t.project_id)
    }

    /// Prevent archived projects from receiving tasks: enforced in start_task.
    /// This helper checks.
    pub async fn ensure_project_accepts_tasks(&self, project_id: &str) -> Result<(), String> {
        let core = self.core.lock().await;
        let project = core
            .project(project_id)
            .ok_or_else(|| "Project not found".to_string())?;
        if project.archived {
            return Err("Archived projects cannot receive new tasks.".to_string());
        }
        if !Path::new(&project.root).exists() {
            return Err("Project directory is unavailable.".to_string());
        }
        Ok(())
    }

    // Same-project concurrency policy helpers.
    pub async fn active_task_count_for_project(&self, project_id: &str) -> usize {
        let core = self.core.lock().await;
        core.runtimes
            .values()
            .filter(|rt| rt.project_id.as_deref() == Some(project_id))
            .filter(|rt| {
                rt.orchestrator
                    .try_lock()
                    .ok()
                    .map(|g| {
                        g.current_task
                            .as_ref()
                            .map(|t| t.state.occupies_execution_slot())
                            .unwrap_or(false)
                    })
                    .unwrap_or(true)
            })
            .count()
    }

    pub async fn can_start_task_in_project(&self, project_id: &str) -> Result<(), String> {
        // Allow at most one actively executing task per project.
        if self.active_task_count_for_project(project_id).await > 0 {
            return Err("Another task is already running in this project.".to_string());
        }
        Ok(())
    }

    pub async fn project_concurrency_status(&self, project_id: &str) -> String {
        if self.active_task_count_for_project(project_id).await > 0 {
            "busy".to_string()
        } else {
            "idle".to_string()
        }
    }

    pub async fn check_same_project_conflict(&self, project_id: &str) -> Result<(), String> {
        self.can_start_task_in_project(project_id).await
    }

    pub async fn same_project_conflict(&self, project_id: &str) -> Result<(), String> {
        self.can_start_task_in_project(project_id).await
    }

    // Dashboard / summary.
    pub async fn project_summary(&self, project_id: String) -> Result<ProjectSummary, String> {
        self.get_project_summary(project_id).await
    }

    pub async fn get_project_summary(&self, project_id: String) -> Result<ProjectSummary, String> {
        self.get_project_dashboard(project_id).await
    }

    pub async fn project_dashboard(&self, project_id: String) -> Result<ProjectSummary, String> {
        self.get_project_dashboard(project_id).await
    }

    pub async fn get_project_dashboard(
        &self,
        project_id: String,
    ) -> Result<ProjectSummary, String> {
        let (core, tasks) = {
            let core = self.core.lock().await;
            let tasks: Vec<TaskRecord> = read_task_history(&core.history_dir)
                .into_values()
                .filter(|t| core.task_belongs_to_project(t, &project_id))
                .collect();
            (core.project(&project_id).cloned(), tasks)
        };
        let project = core.ok_or_else(|| "Project not found".to_string())?;
        let token_stats = {
            let core = self.core.lock().await;
            let tracker = core
                .token_usage_tracker
                .lock()
                .map_err(|_| "lock".to_string())?;
            tracker.get_stats_by_project(&project_id)
        };

        let mut active = 0usize;
        let mut queued = 0usize;
        let mut completed = 0usize;
        let mut failed = 0usize;
        let mut cancelled = 0usize;
        let mut latest: Option<TaskRecord> = None;
        let mut last_activity: Option<String> = None;
        for t in &tasks {
            match t.state {
                TaskState::Queued => queued += 1,
                TaskState::Planning
                | TaskState::Executing
                | TaskState::Verifying
                | TaskState::Reviewing
                | TaskState::Retrying
                | TaskState::AwaitingApproval => active += 1,
                TaskState::Completed => completed += 1,
                TaskState::Failed => failed += 1,
                TaskState::Cancelled => cancelled += 1,
                _ => {}
            }
            if latest
                .as_ref()
                .map(|l| t.updated_at > l.updated_at)
                .unwrap_or(true)
            {
                latest = Some(t.clone());
            }
            if last_activity
                .as_ref()
                .map(|l| t.updated_at > *l)
                .unwrap_or(true)
            {
                last_activity = Some(t.updated_at.clone());
            }
        }
        // Also count runtime non-terminal not in history? runtime tasks are in history mostly. Count runtime active slots too.
        let runtime_active = {
            let core = self.core.lock().await;
            core.runtimes
                .values()
                .filter(|rt| rt.project_id.as_deref() == Some(project_id.as_str()))
                .filter(|rt| {
                    rt.orchestrator
                        .try_lock()
                        .ok()
                        .map(|g| {
                            g.current_task
                                .as_ref()
                                .map(|t| !t.state.is_terminal())
                                .unwrap_or(false)
                        })
                        .unwrap_or(true)
                })
                .count()
        };
        if runtime_active > active {
            active = runtime_active;
        }

        let recent: Vec<TaskRecord> = {
            let mut v = tasks.clone();
            v.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
            v.truncate(10);
            v
        };

        Ok(ProjectSummary {
            project_id: project.id.clone(),
            name: project.name.clone(),
            root: project.root.clone(),
            archived: project.archived,
            status: if !Path::new(&project.root).exists() {
                "unavailable".to_string()
            } else if project.archived {
                "archived".to_string()
            } else {
                "active".to_string()
            },
            health: if Path::new(&project.root).exists() {
                "healthy".to_string()
            } else {
                "unavailable".to_string()
            },
            directory_available: Path::new(&project.root).exists(),
            active_task_count: active,
            queued_task_count: queued,
            completed_task_count: completed,
            failed_task_count: failed,
            cancelled_task_count: cancelled,
            total_task_count: tasks.len(),
            latest_task_status: latest
                .as_ref()
                .map(|t| format!("{:?}", t.state).to_lowercase()),
            latest_task_id: latest.as_ref().map(|t| t.id.clone()),
            last_activity_at: last_activity,
            recent_tasks: recent,
            token_usage: token_stats,
        })
    }

    pub async fn get_project_health(&self, project_id: String) -> String {
        self.project_health(&project_id).await
    }

    pub async fn list_project_tasks(&self, project_id: String) -> Result<Vec<TaskRecord>, String> {
        self.list_tasks(Some(project_id)).await
    }

    pub async fn tasks_for_project(&self, project_id: String) -> Result<Vec<TaskRecord>, String> {
        self.list_tasks(Some(project_id)).await
    }

    // -----------------------------------------------------------------------
    // Provider profiles
    // -----------------------------------------------------------------------

    pub async fn create_provider_profile(
        &self,
        name: String,
        project_id: Option<String>,
        is_default: bool,
        input: ProviderConfigInput,
    ) -> Result<ProviderProfile, String> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("Profile name cannot be empty".to_string());
        }
        let mut config = normalize_profile_config(&ProviderConfig::default(), input)?;

        let mut core = self.core.lock().await;
        if let Some(ref project_id) = project_id {
            if core.project(project_id).is_none() {
                return Err("Project not found".to_string());
            }
        }
        let profile_id = Uuid::new_v4().to_string();
        if let Some(secret) = config.api_key.take() {
            core.credential_store
                .store(&profile_id, &secret)
                .map_err(credential_store_error)?;
        }
        if is_default {
            for profile in core.provider_profiles.iter_mut() {
                if profile.project_id == project_id {
                    profile.is_default = false;
                }
            }
        }
        let now = chrono::Utc::now().to_rfc3339();
        let profile = ProviderProfile {
            id: profile_id,
            name,
            project_id,
            is_default,
            config,
            created_at: now.clone(),
            updated_at: now,
        };
        core.provider_profiles.push(profile.clone());
        core.save();
        core.emit_workspace();
        Ok(profile)
    }

    pub async fn update_provider_profile(
        &self,
        profile_id: String,
        name: Option<String>,
        is_default: Option<bool>,
        input: ProviderConfigInput,
    ) -> Result<ProviderProfile, String> {
        let mut core = self.core.lock().await;
        let existing = core
            .profile(&profile_id)
            .cloned()
            .ok_or_else(|| "Provider profile not found".to_string())?;
        let mut config = normalize_profile_config(&existing.config, input)?;
        if let Some(secret) = config.api_key.take() {
            core.credential_store
                .store(&profile_id, &secret)
                .map_err(credential_store_error)?;
        }

        if let Some(true) = is_default {
            let scope = existing.project_id.clone();
            for profile in core.provider_profiles.iter_mut() {
                if profile.project_id == scope {
                    profile.is_default = false;
                }
            }
        }
        let profile = core
            .provider_profiles
            .iter_mut()
            .find(|profile| profile.id == profile_id)
            .ok_or_else(|| "Provider profile not found".to_string())?;
        if let Some(name) = name {
            let trimmed = name.trim().to_string();
            if !trimmed.is_empty() {
                profile.name = trimmed;
            }
        }
        if let Some(is_default) = is_default {
            profile.is_default = is_default;
        }
        profile.config = config;
        profile.updated_at = chrono::Utc::now().to_rfc3339();
        let updated = profile.clone();
        core.save();
        core.emit_workspace();
        Ok(updated)
    }

    pub async fn delete_provider_profile(&self, profile_id: String) -> Result<(), String> {
        let mut core = self.core.lock().await;
        if core.profile(&profile_id).is_none() {
            return Err("Provider profile not found".to_string());
        }
        core.credential_store
            .delete(&profile_id)
            .map_err(credential_store_error)?;
        core.provider_profiles
            .retain(|profile| profile.id != profile_id);
        for project in core.projects.iter_mut() {
            if project.provider_profile_id.as_deref() == Some(profile_id.as_str()) {
                project.provider_profile_id = None;
            }
        }
        core.save();
        core.emit_workspace();
        drop(core);
        self.clear_cached_profile_credentials(std::slice::from_ref(&profile_id))
            .await;
        Ok(())
    }

    pub async fn delete_provider_credential(&self, profile_id: String) -> Result<(), String> {
        {
            let core = self.core.lock().await;
            if core.profile(&profile_id).is_none() {
                return Err("Provider profile not found".to_string());
            }
            core.credential_store
                .delete(&profile_id)
                .map_err(credential_store_error)?;
        }
        self.clear_cached_profile_credentials(std::slice::from_ref(&profile_id))
            .await;
        Ok(())
    }

    async fn clear_cached_profile_credentials(&self, profile_ids: &[String]) {
        if profile_ids.is_empty() {
            return;
        }
        let runtimes: Vec<SharedOrchestrator> = {
            let core = self.core.lock().await;
            core.runtimes
                .values()
                .map(|runtime| runtime.orchestrator.clone())
                .collect()
        };
        for runtime in runtimes {
            let mut orchestrator = runtime.lock().await;
            let belongs_to_removed_profile = orchestrator
                .current_task
                .as_ref()
                .and_then(|task| task.provider_profile_id.as_ref())
                .is_some_and(|id| profile_ids.contains(id));
            if belongs_to_removed_profile {
                orchestrator.config.api_key = None;
                if let Some(task) = orchestrator.current_task.as_mut() {
                    if let Some(snapshot) = task.execution_config.as_mut() {
                        snapshot.api_key = None;
                    }
                }
            }
            for task in orchestrator.task_history.values_mut() {
                if task
                    .provider_profile_id
                    .as_ref()
                    .is_some_and(|id| profile_ids.contains(id))
                {
                    if let Some(snapshot) = task.execution_config.as_mut() {
                        snapshot.api_key = None;
                    }
                }
            }
        }
    }

    /// Provider status for the default global profile (kept for the legacy
    /// single-provider settings view).
    pub async fn default_provider_status(&self) -> Result<ProviderStatus, String> {
        let core = self.core.lock().await;
        let profile = core.resolve_profile(None, None);
        let profile_id = profile.map(|profile| profile.id.as_str());
        let config = profile
            .map(|profile| profile.config.clone())
            .unwrap_or_default();
        let has_saved_key = profile_id
            .map(|id| core.credential_store.contains(id))
            .transpose()
            .map_err(credential_store_error)?
            .unwrap_or(false);
        let orch =
            Orchestrator::new_isolated(None, core.history_dir.clone(), core.config_path.clone());
        let mut orch = orch;
        orch.config = config;
        Ok(with_saved_credential_status(
            orch.get_provider_status(),
            has_saved_key,
        ))
    }

    pub async fn provider_status_for(
        &self,
        profile_id: Option<String>,
    ) -> Result<ProviderStatus, String> {
        let core = self.core.lock().await;
        let profile = profile_id
            .as_deref()
            .and_then(|id| core.profile(id))
            .or_else(|| core.resolve_profile(None, None));
        let has_saved_key = profile
            .map(|profile| core.credential_store.contains(&profile.id))
            .transpose()
            .map_err(credential_store_error)?
            .unwrap_or(false);
        let config = profile
            .map(|profile| profile.config.clone())
            .unwrap_or_default();
        let mut orch =
            Orchestrator::new_isolated(None, core.history_dir.clone(), core.config_path.clone());
        orch.config = config;
        Ok(with_saved_credential_status(
            orch.get_provider_status(),
            has_saved_key,
        ))
    }

    /// Provider status for the profile a session actually resolves to. This is
    /// what the Prompt session UI uses to show whether a credential is
    /// currently available; it never exposes the key itself.
    pub async fn session_provider_status(
        &self,
        session_id: String,
    ) -> Result<ProviderStatus, String> {
        let core = self.core.lock().await;
        let session = core
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| "Session not found".to_string())?;
        let profile = core.resolve_profile(
            session.project_id.as_deref(),
            session.provider_profile_id.as_deref(),
        );
        let has_saved_key = profile
            .map(|profile| core.credential_store.contains(&profile.id))
            .transpose()
            .map_err(credential_store_error)?
            .unwrap_or(false);
        let config = profile
            .map(|profile| profile.config.clone())
            .unwrap_or_default();
        let mut orch =
            Orchestrator::new_isolated(None, core.history_dir.clone(), core.config_path.clone());
        orch.config = config;
        Ok(with_saved_credential_status(
            orch.get_provider_status(),
            has_saved_key,
        ))
    }

    async fn retrieve_profile_credential(
        &self,
        profile_id: Option<&str>,
        config: &ProviderConfig,
    ) -> Result<Option<String>, String> {
        if let Some(key) = config.api_key.as_ref().filter(|key| !key.trim().is_empty()) {
            return Ok(Some(key.clone()));
        }
        if config.planner_type != "cloud_http" {
            return Ok(None);
        }
        let Some(profile_id) = profile_id else {
            return Ok(None);
        };
        let store = self.core.lock().await.credential_store.clone();
        store.retrieve(profile_id).map_err(credential_store_error)
    }

    async fn hydrate_task_credential(
        &self,
        orchestrator: &SharedOrchestrator,
    ) -> Result<(), String> {
        let (profile_id, config) = {
            let guard = orchestrator.lock().await;
            let profile_id = guard
                .current_task
                .as_ref()
                .and_then(|task| task.provider_profile_id.as_deref())
                .map(str::to_string);
            (profile_id, guard.config.clone())
        };
        let key = self
            .retrieve_profile_credential(profile_id.as_deref(), &config)
            .await?;
        let mut guard = orchestrator.lock().await;
        guard.config.api_key = key.clone();
        if let Some(task) = guard.current_task.as_mut() {
            if let Some(snapshot) = task.execution_config.as_mut() {
                snapshot.api_key = key;
            }
        }
        Ok(())
    }

    pub async fn discover_models(
        &self,
        endpoint: String,
        api_key: Option<String>,
    ) -> Result<Vec<String>, String> {
        CloudHttpPlannerReviewer::list_models(&endpoint, api_key.as_deref()).await
    }

    pub fn cli_presets(&self) -> Vec<CliAgentPreset> {
        cli_agent_presets()
    }

    pub fn local_model_presets(&self) -> Vec<LocalModelPreset> {
        local_model_presets()
    }

    pub fn check_cli_availability(&self, executable: String) -> Result<AvailabilityCheck, String> {
        use crate::providers::resolve_executable;
        let path = resolve_executable(&executable);
        match path {
            Some(p) => Ok(AvailabilityCheck {
                ok: true,
                path: Some(p.to_string_lossy().to_string()),
                error: None,
            }),
            None => Ok(AvailabilityCheck {
                ok: false,
                path: None,
                error: Some(format!(
                    "Executable '{}' not found on PATH or as a file path.",
                    executable
                )),
            }),
        }
    }

    // -----------------------------------------------------------------------
    // Tasks
    // -----------------------------------------------------------------------

    pub async fn start_task(
        &self,
        user_task: String,
        project_root: String,
        options: StartTaskOptions,
    ) -> Result<TaskRecord, String> {
        self.start_task_with_context(user_task, project_root, options, None, None)
            .await
    }

    /// Shared task-creation path. The extra arguments are only supplied by the
    /// Prompt-session handoff; direct `start_task` callers keep their previous
    /// behavior.
    async fn start_task_with_context(
        &self,
        user_task: String,
        project_root: String,
        options: StartTaskOptions,
        planning_context: Option<String>,
        source_session_id: Option<String>,
    ) -> Result<TaskRecord, String> {
        let (mut profile_config, profile_id, project_id, project_root, should_queue) = {
            let core = self.core.lock().await;

            // Resolve or auto-create the project for this root.
            let canonical = match dunce::canonicalize(Path::new(&project_root)) {
                Ok(path) => path.to_string_lossy().to_string(),
                Err(e) => {
                    return Err(format!("Failed to canonicalize project root: {}", e));
                }
            };
            let project = options
                .project_id
                .as_deref()
                .and_then(|id| core.project(id))
                .or_else(|| core.find_project_for_root(&canonical));
            if let Some(project) = project {
                if project.archived {
                    return Err("Archived projects cannot receive new tasks.".to_string());
                }
                if !std::path::Path::new(&project.root).exists() {
                    return Err("Project directory is unavailable.".to_string());
                }
            }
            let project_id = project.map(|project| project.id.clone());
            let resolved_root = project
                .map(|project| project.root.clone())
                .unwrap_or_else(|| canonical.clone());

            let profile = core.resolve_profile(
                project_id.as_deref(),
                options.provider_profile_id.as_deref(),
            );
            let profile_id = profile.map(|profile| profile.id.clone());
            let (config, _) = core.resolve_config_with_precedence(
                project_id.as_deref(),
                options.provider_profile_id.as_deref(),
                &TaskConfigOverrides::from(&options),
            );

            let active_slots = count_active_slots(&core);
            let should_queue = core.settings.scheduler_paused
                || active_slots >= core.settings.max_concurrent_tasks as usize;

            (config, profile_id, project_id, resolved_root, should_queue)
        };
        if !should_queue {
            profile_config.api_key = self
                .retrieve_profile_credential(profile_id.as_deref(), &profile_config)
                .await?;
        }

        let history_dir = { self.core.lock().await.history_dir.clone() };
        let config_path = { self.core.lock().await.config_path.clone() };
        let instructions_path = config_path
            .parent()
            .map(crate::instructions::default_instruction_store_path)
            .unwrap_or_else(|| std::path::PathBuf::from("instructions.json"));
        let app_handle = { self.core.lock().await.app_handle.clone() };

        // Apply managed instructions to planning context (predictable + traceable).
        // Also merge discovered project instruction files (ARCHIFY.md, CLAUDE.md,
        // AGENTS.md, ...) so file-based project instructions reach the planner,
        // not only the executor. Discovered files are read-only references and are
        // never written back.
        let instruction_block = {
            let mut store = crate::instructions::load_instruction_store(&instructions_path);
            let discovered = crate::instructions::discover_project_instruction_files(&project_root);
            store.instructions.extend(discovered);
            crate::instructions::resolve_instructions(
                &store,
                Some(&project_root),
                project_id.as_deref(),
                None,
                Some("planner"),
            )
        };
        let mut planning_context =
            crate::instructions::merge_instruction_context(planning_context, &instruction_block);

        // Apply user-selected skills to planning context, after instructions.
        // Skills are injected only when explicitly selected by the user; they do not
        // apply implicitly or to all tasks. Skills targeting the "planner" agent kind
        // are merged into the planning context; executor skills are handled later
        // in the orchestration loop.
        if !options.selected_skill_ids.is_empty() {
            let base_dir = config_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."));
            let skill_store_path = default_skill_store_path(base_dir);
            let library_dir = default_skill_library_dir(base_dir);
            let mut store = load_skill_store(&skill_store_path);
            refresh_validation(&mut store, &library_dir);

            for id in &options.selected_skill_ids {
                let Some(record) = store.skills.iter().find(|s| s.id == *id && s.managed) else {
                    return Err(format!(
                        "Selected skill '{}' not found in managed skills.",
                        id
                    ));
                };
                if !record.enabled {
                    return Err(format!("Selected skill '{}' is disabled.", record.name));
                }
                if record.validation_issues.iter().any(|i| i.level == "error") {
                    return Err(format!(
                        "Selected skill '{}' has validation errors and cannot be applied.",
                        record.name
                    ));
                }
            }

            let applied =
                resolve_skills_for_agent(&store, &options.selected_skill_ids, Some("planner"));
            if !applied.text.trim().is_empty() {
                let skill_context = applied.text.trim().to_string();
                planning_context = Some(match planning_context {
                    Some(existing) if !existing.trim().is_empty() => {
                        format!("{}\n\n---\n\n{}", existing.trim(), skill_context)
                    }
                    _ => skill_context,
                });
            }
            if !applied.refs.is_empty() {
                if let Some(h) = app_handle.as_ref() {
                    let names: Vec<_> = applied
                        .refs
                        .iter()
                        .map(|r| format!("{} ({})", r.name, r.id))
                        .collect();
                    let _ = h.emit(
                        "app_log",
                        format!(
                            "Skills applied to planning ({} skills): {}",
                            names.len(),
                            names.join(", ")
                        ),
                    );
                }
            }
            for w in &applied.warnings {
                if let Some(h) = app_handle.as_ref() {
                    let _ = h.emit("app_log", format!("Skill warning: {}", w));
                }
            }
            if applied.truncated {
                if let Some(h) = app_handle.as_ref() {
                    let _ = h.emit(
                        "app_log",
                        "Warning: Selected skills truncated to fit prompt budget.",
                    );
                }
            }
        }
        if !instruction_block.refs.is_empty() {
            let titles: Vec<_> = instruction_block
                .refs
                .iter()
                .map(|r| r.title.as_str())
                .collect();
            if let Some(h) = app_handle.as_ref() {
                let _ = h.emit(
                    "app_log",
                    format!(
                        "Instructions applied ({}): {}",
                        titles.len(),
                        titles.join(", ")
                    ),
                );
            }
            for w in &instruction_block.warnings {
                if let Some(h) = app_handle.as_ref() {
                    let _ = h.emit("app_log", format!("Instruction warning: {}", w));
                }
            }
        }

        let orchestrator = Arc::new(Mutex::new(Orchestrator::new_isolated(
            app_handle,
            history_dir,
            config_path,
        )));
        let monitoring_hub = { self.core.lock().await.monitoring_hub.clone() };
        orchestrator.lock().await.set_monitoring_hub(monitoring_hub);
        let usage_tracker = self.core.lock().await.token_usage_tracker.clone();
        {
            let mut orch = orchestrator.lock().await;
            orch.config = profile_config;
            // Pass workspace model preferences so select_model() can resolve
            // default / responsibility / auto selections during planning.
            let prefs = self.core.lock().await.model_selector.preferences().clone();
            orch.set_model_preferences(Some(prefs), None);
            orch.set_token_usage_tracker(Some(usage_tracker));
        }

        let task = orchestrator::create_task(
            orchestrator.clone(),
            user_task,
            project_root,
            TaskCreationOptions {
                project_id: project_id.clone(),
                provider_profile_id: profile_id,
                agent_name: None,
                priority: options.priority.unwrap_or(0),
                initial_state: Some(if should_queue {
                    TaskState::Queued
                } else {
                    TaskState::Planning
                }),
                planning_context,
                source_session_id,
                selected_skill_ids: options.selected_skill_ids.clone(),
            },
        )
        .await?;

        {
            let mut core = self.core.lock().await;
            core.runtimes.insert(
                task.id.clone(),
                TaskRuntime {
                    orchestrator: orchestrator.clone(),
                    project_id,
                    priority: task.priority,
                },
            );
            core.emit_workspace();
        }

        if task.state == TaskState::Planning {
            orchestrator::begin_planning(orchestrator.clone(), task.id.clone()).await?;
        }

        self.task_status(&task.id)
            .await?
            .ok_or_else(|| "Task disappeared right after creation".to_string())
    }

    /// Explicitly create a task from an existing Prompt AI conversation.
    ///
    /// The conversation is supplied to the existing planner as additional
    /// context. The session is never modified, and the task still goes through
    /// the normal plan review and approval flow before any CLI work starts.
    pub async fn start_task_from_session(
        &self,
        session_id: String,
        objective: Option<String>,
    ) -> Result<TaskRecord, String> {
        let session_id = session_id.trim().to_string();
        if session_id.is_empty() {
            return Err("Choose a Prompt AI session before planning the conversation.".to_string());
        }

        {
            let mut core = self.core.lock().await;
            if !core.handoffs_in_progress.insert(session_id.clone()) {
                return Err(
                    "A task is already being created from this conversation. Wait for it to finish."
                        .to_string(),
                );
            }
        }

        let result = self
            .start_task_from_session_inner(&session_id, objective)
            .await;

        self.core
            .lock()
            .await
            .handoffs_in_progress
            .remove(&session_id);
        result
    }

    async fn start_task_from_session_inner(
        &self,
        session_id: &str,
        objective: Option<String>,
    ) -> Result<TaskRecord, String> {
        let (session, project) = {
            let core = self.core.lock().await;
            let session = core
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .cloned()
                .ok_or_else(|| {
                    "Prompt session not found. It may have been deleted; open Sessions and choose another conversation."
                        .to_string()
                })?;
            if session.kind != SessionKind::Prompt {
                return Err(
                    "Only Prompt AI conversations can be turned into a task. This session is a CLI coding-agent session."
                        .to_string(),
                );
            }
            if session.busy {
                return Err(
                    "This Prompt session is still waiting for a response. Wait for it to finish before planning the conversation."
                        .to_string(),
                );
            }
            let project_id = session.project_id.as_deref().ok_or_else(|| {
                "This Prompt session is not linked to a project. Create a Prompt session with a project selected so the planner knows where to work."
                    .to_string()
            })?;
            let project = core.project(project_id).cloned().ok_or_else(|| {
                "The project linked to this Prompt session no longer exists. Create a new Prompt session for an existing project."
                    .to_string()
            })?;
            (session, project)
        };

        // Reject a second handoff while a task from this conversation is still
        // active. This complements the in-flight guard above for repeated clicks
        // after the first request has returned.
        if let Some(existing) = self.active_task_for_session(&session.id).await {
            return Err(format!(
                "A task (id={}) is already in progress from this conversation. Finish or cancel it before planning the conversation again.",
                existing
            ));
        }

        let objective = objective
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .or_else(|| latest_user_objective(&session))
            .ok_or_else(|| {
                "This conversation has no user requirements to plan yet. Send at least one message describing what you want before planning."
                    .to_string()
            })?;

        let planning_context = build_session_planning_context(&session).ok_or_else(|| {
            "This conversation has no usable messages to plan. Send a message describing the objective before planning."
                .to_string()
        })?;

        let options = StartTaskOptions {
            project_id: Some(project.id.clone()),
            provider_profile_id: session.provider_profile_id.clone(),
            ..Default::default()
        };

        self.start_task_with_context(
            objective,
            project.root.clone(),
            options,
            Some(planning_context),
            Some(session.id.clone()),
        )
        .await
    }

    async fn active_task_for_session(&self, session_id: &str) -> Option<String> {
        let core = self.core.lock().await;
        for runtime in core.runtimes.values() {
            if let Ok(guard) = runtime.orchestrator.try_lock() {
                if let Some(task) = guard.current_task.as_ref() {
                    if task.source_session_id.as_deref() == Some(session_id)
                        && !task.state.is_terminal()
                    {
                        return Some(task.id.clone());
                    }
                }
            }
        }
        read_task_history(&core.history_dir)
            .into_values()
            .find(|task| {
                task.source_session_id.as_deref() == Some(session_id) && !task.state.is_terminal()
            })
            .map(|task| task.id)
    }

    /// Most recently updated non-terminal task across every project.
    pub async fn current_task(&self) -> Option<TaskRecord> {
        let core = self.core.lock().await;
        let mut best: Option<TaskRecord> = None;
        for runtime in core.runtimes.values() {
            if let Ok(guard) = runtime.orchestrator.try_lock() {
                if let Some(ref task) = guard.current_task {
                    if !task.state.is_terminal() {
                        let is_newer = best
                            .as_ref()
                            .map(|prev| task.updated_at > prev.updated_at)
                            .unwrap_or(true);
                        if is_newer {
                            best = Some(task.clone());
                        }
                    }
                }
            }
        }
        best
    }

    pub async fn task_status(&self, task_id: &str) -> Result<Option<TaskRecord>, String> {
        let runtime = {
            let core = self.core.lock().await;
            core.runtimes
                .get(task_id)
                .map(|runtime| runtime.orchestrator.clone())
        };
        if let Some(orchestrator) = runtime {
            let guard = orchestrator.lock().await;
            return Ok(guard
                .current_task
                .clone()
                .filter(|task| task.id == task_id)
                .or_else(|| guard.task_history.get(task_id).cloned()));
        }
        let core = self.core.lock().await;
        Ok(read_task_history(&core.history_dir).remove(task_id))
    }

    pub async fn list_tasks(&self, project_id: Option<String>) -> Result<Vec<TaskRecord>, String> {
        let core = self.core.lock().await;
        let mut tasks: Vec<TaskRecord> =
            read_task_history(&core.history_dir).into_values().collect();
        if let Some(project_id) = project_id {
            tasks.retain(|task| core.task_belongs_to_project(task, &project_id));
        }
        tasks.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(tasks)
    }

    // -----------------------------------------------------------------------
    // Notifications
    // -----------------------------------------------------------------------

    pub async fn record_notification(
        &self,
        mut notification: crate::models::NotificationRecord,
    ) -> bool {
        let mut core = self.core.lock().await;
        if notification.id.is_empty() {
            notification.id = uuid::Uuid::new_v4().to_string();
        }
        if notification.created_at.is_empty() {
            notification.created_at = chrono::Utc::now().to_rfc3339();
        }
        if let Some(key) = &notification.dedup_key {
            if core.notification_dedup.contains(key) {
                return false;
            }
            core.notification_dedup.insert(key.clone());
        }
        core.notifications.push(notification.clone());
        // Retention: keep newest 2000 by timestamp.
        if core.notifications.len() > 2000 {
            core.notifications
                .sort_by(|a, b| a.created_at.cmp(&b.created_at));
            let drop = core.notifications.len() - 2000;
            core.notifications.drain(0..drop);
        }
        let _ = write_json_atomic(&core.notification_path, &core.notifications);
        if let Some(h) = core.app_handle.clone() {
            let _ = h.emit("notification_created", notification);
        }
        true
    }

    pub async fn list_notifications(
        &self,
        filter: crate::models::NotificationFilter,
    ) -> Result<Vec<crate::models::NotificationRecord>, String> {
        let core = self.core.lock().await;
        if let Some(pid) = &filter.project_id {
            if !core.projects.iter().any(|p| &p.id == pid) {
                return Err("Project not found".to_string());
            }
        }
        let mut out: Vec<_> = core
            .notifications
            .iter()
            .filter(|n| {
                if let Some(pid) = &filter.project_id {
                    if n.project_id.as_ref() != Some(pid) {
                        return false;
                    }
                }
                if let Some(tid) = &filter.task_id {
                    if n.task_id.as_ref() != Some(tid) {
                        return false;
                    }
                }
                if let Some(cat) = &filter.category {
                    if &n.category != cat {
                        return false;
                    }
                }
                if let Some(sev) = &filter.severity {
                    if &n.severity != sev {
                        return false;
                    }
                }
                if filter.unread_only && n.read {
                    return false;
                }
                true
            })
            .cloned()
            .collect();
        // newest first, stable tie-break by id desc
        out.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });
        let offset = filter.offset.unwrap_or(0);
        let limit = filter.limit.unwrap_or(out.len().max(1));
        if offset >= out.len() {
            return Ok(Vec::new());
        }
        Ok(out.into_iter().skip(offset).take(limit).collect())
    }

    pub async fn notification_unread_count(
        &self,
        project_id: Option<String>,
    ) -> Result<usize, String> {
        let core = self.core.lock().await;
        if let Some(pid) = &project_id {
            if !core.projects.iter().any(|p| &p.id == pid) {
                return Err("Project not found".to_string());
            }
        }
        Ok(core
            .notifications
            .iter()
            .filter(|n| !n.read && (project_id.is_none() || n.project_id == project_id))
            .count())
    }

    pub async fn mark_notification_read(&self, id: String, read: bool) -> Result<(), String> {
        let mut core = self.core.lock().await;
        let Some(n) = core.notifications.iter_mut().find(|n| n.id == id) else {
            return Err("Notification not found".to_string());
        };
        n.read = read;
        let _ = write_json_atomic(&core.notification_path, &core.notifications);
        Ok(())
    }

    pub async fn mark_all_notifications_read(
        &self,
        project_id: Option<String>,
    ) -> Result<usize, String> {
        let mut core = self.core.lock().await;
        if let Some(pid) = &project_id {
            if !core.projects.iter().any(|p| &p.id == pid) {
                return Err("Project not found".to_string());
            }
        }
        let mut count = 0usize;
        for n in core.notifications.iter_mut() {
            if !n.read && (project_id.is_none() || n.project_id == project_id) {
                n.read = true;
                count += 1;
            }
        }
        let _ = write_json_atomic(&core.notification_path, &core.notifications);
        Ok(count)
    }

    pub async fn delete_notification(&self, id: String) -> Result<(), String> {
        let mut core = self.core.lock().await;
        let before = core.notifications.len();
        core.notifications.retain(|n| n.id != id);
        if core.notifications.len() == before {
            return Err("Notification not found".to_string());
        }
        // Remove dedup keys that no longer have a notification.
        let keys: HashSet<String> = core
            .notifications
            .iter()
            .filter_map(|n| n.dedup_key.clone())
            .collect();
        core.notification_dedup.retain(|k| keys.contains(k));
        let _ = write_json_atomic(&core.notification_path, &core.notifications);
        Ok(())
    }

    pub async fn clear_notifications(&self, project_id: Option<String>) -> Result<usize, String> {
        let mut core = self.core.lock().await;
        if let Some(pid) = &project_id {
            if !core.projects.iter().any(|p| &p.id == pid) {
                return Err("Project not found".to_string());
            }
        }
        let before = core.notifications.len();
        core.notifications.retain(|n| {
            if let Some(pid) = &project_id {
                n.project_id.as_ref() != Some(pid)
            } else {
                false
            }
        });
        // if no project, clear all
        if project_id.is_none() {
            core.notifications.clear();
            core.notification_dedup.clear();
        } else {
            let keys: HashSet<String> = core
                .notifications
                .iter()
                .filter_map(|n| n.dedup_key.clone())
                .collect();
            core.notification_dedup.retain(|k| keys.contains(k));
        }
        let cleared = before - core.notifications.len();
        let _ = write_json_atomic(&core.notification_path, &core.notifications);
        Ok(cleared)
    }

    // -----------------------------------------------------------------------
    // Activity
    // -----------------------------------------------------------------------

    pub async fn record_activity(&self, mut activity: crate::models::ActivityRecord) -> bool {
        let mut core = self.core.lock().await;
        if activity.id.is_empty() {
            activity.id = uuid::Uuid::new_v4().to_string();
        }
        if activity.timestamp.is_empty() {
            activity.timestamp = chrono::Utc::now().to_rfc3339();
        }
        if activity.outcome.is_empty() {
            activity.outcome = "info".to_string();
        }
        if let Some(key) = &activity.dedup_key {
            if core.activity_dedup.contains(key) {
                return false;
            }
            core.activity_dedup.insert(key.clone());
        }
        core.activity.push(activity.clone());
        if core.activity.len() > 5000 {
            core.activity.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
            let drop = core.activity.len() - 5000;
            core.activity.drain(0..drop);
        }
        let _ = write_json_atomic(&core.activity_path, &core.activity);
        if let Some(h) = core.app_handle.clone() {
            let _ = h.emit("activity_created", activity);
        }
        true
    }

    pub async fn list_activity(
        &self,
        filter: crate::models::ActivityFilter,
    ) -> Result<Vec<crate::models::ActivityRecord>, String> {
        let core = self.core.lock().await;
        if let Some(pid) = &filter.project_id {
            if !core.projects.iter().any(|p| &p.id == pid) {
                return Err("Project not found".to_string());
            }
        }
        let mut out: Vec<_> = core
            .activity
            .iter()
            .filter(|a| {
                if let Some(pid) = &filter.project_id {
                    if a.project_id.as_ref() != Some(pid) {
                        return false;
                    }
                }
                if let Some(tid) = &filter.task_id {
                    if a.task_id.as_ref() != Some(tid) {
                        return false;
                    }
                }
                if let Some(et) = &filter.event_type {
                    if &a.event_type != et {
                        return false;
                    }
                }
                if let Some(outcome) = &filter.outcome {
                    if &a.outcome != outcome {
                        return false;
                    }
                }
                if let Some(since) = &filter.since {
                    if a.timestamp < *since {
                        return false;
                    }
                }
                if let Some(until) = &filter.until {
                    if a.timestamp > *until {
                        return false;
                    }
                }
                if let Some(q) = &filter.search {
                    let ql = q.to_lowercase();
                    if !a.summary.to_lowercase().contains(&ql)
                        && !a.event_type.to_lowercase().contains(&ql)
                    {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();
        out.sort_by(|a, b| b.timestamp.cmp(&a.timestamp).then_with(|| b.id.cmp(&a.id)));
        let offset = filter.offset.unwrap_or(0);
        let limit = filter.limit.unwrap_or(out.len().max(1));
        if offset >= out.len() {
            return Ok(Vec::new());
        }
        Ok(out.into_iter().skip(offset).take(limit).collect())
    }

    // -----------------------------------------------------------------------
    // Reports
    // -----------------------------------------------------------------------

    pub async fn task_report(&self, task_id: String) -> Result<crate::models::TaskReport, String> {
        let core = self.core.lock().await;
        let task = read_task_history(&core.history_dir)
            .get(&task_id)
            .cloned()
            .ok_or_else(|| "Task not found".to_string())?;
        // project ownership validation: task's project must exist if specified
        let project = task
            .project_id
            .as_ref()
            .and_then(|pid| core.projects.iter().find(|p| &p.id == pid));
        let token_stats = {
            let tracker = core.token_usage_tracker.lock().unwrap();
            tracker.get_stats_by_task(&task_id)
        };
        let has_known = token_stats.request_count > token_stats.unknown_request_count;
        let token_input = if has_known {
            Some(token_stats.total_input_tokens)
        } else {
            None
        };
        let token_output = if has_known {
            Some(token_stats.total_output_tokens)
        } else {
            None
        };
        let token_total = if has_known {
            Some(token_stats.total_tokens)
        } else {
            None
        };

        let mut verification_results = Vec::new();
        let mut review_results = Vec::new();
        let mut corrections = 0u32;
        let mut tests_commands = Vec::new();
        for it in &task.iterations {
            verification_results.extend(it.verification_results.clone());
            if let Some(r) = &it.reviewer_response {
                review_results.push(r.clone());
                if r.verdict != crate::models::ReviewerVerdict::Complete {
                    corrections += 1;
                }
            }
            for v in &it.verification_results {
                if !v.command.trim().is_empty() && !tests_commands.contains(&v.command) {
                    tests_commands.push(v.command.clone());
                }
            }
        }
        let retries = if task.iteration > 1 {
            task.iteration - 1
        } else {
            0
        };
        let files_changed = task.task_level_changes.clone().unwrap_or_else(|| {
            let mut agg = crate::models::ChangedFiles::default();
            for it in &task.iterations {
                agg.created.extend(it.changed_files.created.clone());
                agg.modified.extend(it.changed_files.modified.clone());
                agg.deleted.extend(it.changed_files.deleted.clone());
            }
            // dedup
            agg.created.sort();
            agg.created.dedup();
            agg.modified.sort();
            agg.modified.dedup();
            agg.deleted.sort();
            agg.deleted.dedup();
            agg
        });
        let agents = task.agent_name.clone().into_iter().collect();
        let models: Vec<String> = task
            .execution_config
            .as_ref()
            .and_then(|c| c.cloud_model.clone())
            .into_iter()
            .collect();
        let mut remaining_issues = Vec::new();
        if let Some(last) = task.iterations.last() {
            if let Some(r) = &last.reviewer_response {
                remaining_issues.extend(r.unresolved_issues.clone());
            }
        }
        if let Some(err) = &task.error {
            remaining_issues.push(err.clone());
        }
        let final_outcome = match task.state {
            crate::models::TaskState::Completed => "completed".to_string(),
            crate::models::TaskState::Failed => "failed".to_string(),
            crate::models::TaskState::Cancelled => "cancelled".to_string(),
            _ => format!("{:?}", task.state).to_lowercase(),
        };
        let elapsed_ms = if let (Ok(created), Ok(updated)) = (
            chrono::DateTime::parse_from_rfc3339(&task.created_at),
            chrono::DateTime::parse_from_rfc3339(&task.updated_at),
        ) {
            let diff = updated.signed_duration_since(created).num_milliseconds();
            if diff >= 0 { Some(diff as u64) } else { None }
        } else {
            None
        };
        let mut known_facts = Vec::new();
        known_facts.push("Status derived from persisted task record.".to_string());
        if task.task_level_changes.is_none() {
            known_facts.push(
                "File changes aggregated from iterations (no task-level baseline).".to_string(),
            );
        }
        let mut unavailable = Vec::new();
        if token_total.is_none() {
            unavailable.push("Token usage unavailable".to_string());
        }
        if elapsed_ms.is_none() {
            unavailable.push("Elapsed time unavailable".to_string());
        }
        let report = crate::models::TaskReport {
            task_id: task.id.clone(),
            project_id: task.project_id.clone(),
            project_name: project.map(|p| p.name.clone()),
            original_request: task.user_task.clone(),
            status: format!("{:?}", task.state).to_lowercase(),
            created_at: task.created_at.clone(),
            updated_at: task.updated_at.clone(),
            elapsed_ms,
            planning_summary: task.plan.as_ref().map(|p| p.summary.clone()),
            agents,
            models,
            execution_attempts: task.iterations.len() as u32,
            verification_results,
            review_results,
            corrections,
            retries,
            files_changed,
            tests_and_commands: tests_commands,
            final_outcome,
            remaining_issues,
            token_input,
            token_output,
            token_total,
            known_facts,
            unavailable,
        };
        Ok(report)
    }

    pub async fn project_report(
        &self,
        project_id: String,
    ) -> Result<crate::models::ProjectReport, String> {
        let core = self.core.lock().await;
        if !core.projects.iter().any(|p| p.id == project_id) {
            return Err("Project not found".to_string());
        }
        let project = core
            .projects
            .iter()
            .find(|p| p.id == project_id)
            .unwrap()
            .clone();
        let tasks: Vec<TaskRecord> = read_task_history(&core.history_dir)
            .into_values()
            .filter(|t| t.project_id.as_deref() == Some(&project_id))
            .collect();
        let mut total = 0usize;
        let mut completed = 0usize;
        let mut failed = 0usize;
        let mut cancelled = 0usize;
        let mut queued = 0usize;
        let mut active = 0usize;
        let mut attempts = 0u32;
        let mut retries = 0u32;
        let mut vpassed = 0usize;
        let mut vfailed = 0usize;
        let mut recent_failures = Vec::new();
        let mut recent_tasks = Vec::new();
        for t in &tasks {
            total += 1;
            match t.state {
                crate::models::TaskState::Completed => completed += 1,
                crate::models::TaskState::Failed => {
                    failed += 1;
                    recent_failures.push(t.id.clone());
                }
                crate::models::TaskState::Cancelled => cancelled += 1,
                crate::models::TaskState::Queued => queued += 1,
                _ if t.state.is_active() => active += 1,
                _ => {}
            }
            attempts += t.iterations.len() as u32;
            if t.iteration > 1 {
                retries += t.iteration - 1;
            }
            for it in &t.iterations {
                for v in &it.verification_results {
                    match v.status {
                        crate::models::VerificationStatus::Passed => vpassed += 1,
                        crate::models::VerificationStatus::Failed => vfailed += 1,
                        _ => {}
                    }
                }
            }
        }
        let mut sorted = tasks.clone();
        sorted.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        for t in sorted.iter().take(5) {
            recent_tasks.push(t.id.clone());
        }
        recent_failures.truncate(5);
        let completion_rate = if total > 0 {
            completed as f64 / total as f64
        } else {
            0.0
        };
        let token_stats = {
            let tracker = core.token_usage_tracker.lock().unwrap();
            tracker.get_stats_by_project(&project_id)
        };
        let has_known = token_stats.request_count > token_stats.unknown_request_count;
        let ti = token_stats.total_input_tokens;
        let to = token_stats.total_output_tokens;
        let tt = token_stats.total_tokens;
        Ok(crate::models::ProjectReport {
            project_id,
            name: project.name,
            total_tasks: total,
            completed_tasks: completed,
            failed_tasks: failed,
            cancelled_tasks: cancelled,
            queued_tasks: queued,
            active_tasks: active,
            completion_rate,
            execution_attempts: attempts,
            retries,
            verification_passed: vpassed,
            verification_failed: vfailed,
            token_input: if has_known { Some(ti) } else { None },
            token_output: if has_known { Some(to) } else { None },
            token_total: if has_known { Some(tt) } else { None },
            recent_failures,
            recent_tasks,
        })
    }

    // -----------------------------------------------------------------------
    // Monitoring processing
    // -----------------------------------------------------------------------

    pub async fn process_monitoring_events(&self) {
        // Collect new recorded events first.
        let (hub, last_seq) = {
            let core = self.core.lock().await;
            (core.monitoring_hub.clone(), core.last_monitoring_sequence)
        };
        let events = hub.history_since(last_seq);
        if events.is_empty() {
            return;
        }
        for ev in events {
            // Update last processed seq.
            {
                let mut core = self.core.lock().await;
                if ev.sequence > core.last_monitoring_sequence {
                    core.last_monitoring_sequence = ev.sequence;
                }
            }
            // Determine task id
            let task_id = ev.task_id.clone();
            // Look up project/task
            let project_id = {
                let core = self.core.lock().await;
                let task = read_task_history(&core.history_dir).get(&task_id).cloned();
                task.as_ref().and_then(|t| t.project_id.clone())
            };
            // Parse payload
            let payload = ev.payload.clone();
            // Try ExecutionEvent mapping by key
            if let Some(obj) = payload.as_object() {
                // ExecutionEvent externally tagged: key is variant name snake_case
                for (key, val) in obj {
                    let dedup = format!("mon:{}:{}", ev.sequence, key);
                    match key.as_str() {
                        "task_state_transitioned" => {
                            let from = val.get("from").and_then(|v| v.as_str()).unwrap_or("");
                            let to = val.get("to").and_then(|v| v.as_str()).unwrap_or("");
                            let iter = val
                                .get("iteration")
                                .and_then(|v| v.as_u64())
                                .map(|x| x as u32);
                            let summary =
                                format!("Task {} transitioned {} -> {}", task_id, from, to);
                            let outcome = match to {
                                "completed" => "success",
                                "failed" => "failure",
                                "cancelled" => "cancelled",
                                _ => "info",
                            };
                            // notification for meaningful transitions
                            let notify = matches!(
                                to,
                                "planning"
                                    | "executing"
                                    | "verifying"
                                    | "reviewing"
                                    | "retrying"
                                    | "completed"
                                    | "failed"
                                    | "cancelled"
                                    | "awaiting_approval"
                            );
                            if notify {
                                let category = match to {
                                    "completed" => "task_completed",
                                    "failed" => "task_failed",
                                    "cancelled" => "task_cancelled",
                                    "retrying" => "retry_started",
                                    "awaiting_approval" => "task_blocked",
                                    _ => "task_progress",
                                };
                                let severity = match to {
                                    "completed" => "success",
                                    "failed" => "error",
                                    "cancelled" => "warning",
                                    _ => "info",
                                };
                                let _ = self
                                    .record_notification(crate::models::NotificationRecord {
                                        id: String::new(),
                                        created_at: String::new(),
                                        category: category.to_string(),
                                        severity: severity.to_string(),
                                        title: format!("Task {}", to),
                                        message: summary.clone(),
                                        project_id: project_id.clone(),
                                        task_id: Some(task_id.clone()),
                                        session_id: None,
                                        agent_id: None,
                                        read: false,
                                        dedup_key: Some(format!("notif:{}", dedup)),
                                        action: Some("open_task".to_string()),
                                        metadata: Some(val.clone()),
                                    })
                                    .await;
                            }
                            let _ = self
                                .record_activity(crate::models::ActivityRecord {
                                    id: String::new(),
                                    timestamp: String::new(),
                                    event_type: format!("task_{}", to),
                                    summary,
                                    project_id: project_id.clone(),
                                    task_id: Some(task_id.clone()),
                                    agent_id: None,
                                    session_id: None,
                                    iteration: iter,
                                    outcome: outcome.to_string(),
                                    dedup_key: Some(format!("act:{}", dedup)),
                                    metadata: Some(val.clone()),
                                })
                                .await;
                        }
                        "verification_completed" => {
                            let status = val.get("status").and_then(|v| v.as_str()).unwrap_or("");
                            let iter = val
                                .get("iteration")
                                .and_then(|v| v.as_u64())
                                .map(|x| x as u32);
                            let summary = format!("Verification {} for task {}", status, task_id);
                            let outcome = if status == "passed" {
                                "success"
                            } else {
                                "failure"
                            };
                            let _ = self
                                .record_activity(crate::models::ActivityRecord {
                                    id: String::new(),
                                    timestamp: String::new(),
                                    event_type: "verification_completed".to_string(),
                                    summary: summary.clone(),
                                    project_id: project_id.clone(),
                                    task_id: Some(task_id.clone()),
                                    agent_id: None,
                                    session_id: None,
                                    iteration: iter,
                                    outcome: outcome.to_string(),
                                    dedup_key: Some(format!("act:{}", dedup)),
                                    metadata: Some(val.clone()),
                                })
                                .await;
                            if status != "passed" {
                                let _ = self
                                    .record_notification(crate::models::NotificationRecord {
                                        id: String::new(),
                                        created_at: String::new(),
                                        category: "verification_failed".to_string(),
                                        severity: "warning".to_string(),
                                        title: "Verification failed".to_string(),
                                        message: summary,
                                        project_id: project_id.clone(),
                                        task_id: Some(task_id.clone()),
                                        session_id: None,
                                        agent_id: None,
                                        read: false,
                                        dedup_key: Some(format!("notif:{}", dedup)),
                                        action: Some("open_task".to_string()),
                                        metadata: Some(val.clone()),
                                    })
                                    .await;
                            }
                        }
                        "review_completed" => {
                            let verdict = val.get("verdict").and_then(|v| v.as_str()).unwrap_or("");
                            let iter = val
                                .get("iteration")
                                .and_then(|v| v.as_u64())
                                .map(|x| x as u32);
                            let summary = format!("Review {} for task {}", verdict, task_id);
                            let _ = self
                                .record_activity(crate::models::ActivityRecord {
                                    id: String::new(),
                                    timestamp: String::new(),
                                    event_type: "review_completed".to_string(),
                                    summary,
                                    project_id: project_id.clone(),
                                    task_id: Some(task_id.clone()),
                                    agent_id: None,
                                    session_id: None,
                                    iteration: iter,
                                    outcome: if verdict == "complete" {
                                        "success"
                                    } else {
                                        "info"
                                    }
                                    .to_string(),
                                    dedup_key: Some(format!("act:{}", dedup)),
                                    metadata: Some(val.clone()),
                                })
                                .await;
                        }
                        "iteration_started" => {
                            let iter = val
                                .get("iteration")
                                .and_then(|v| v.as_u64())
                                .map(|x| x as u32);
                            let _ = self
                                .record_activity(crate::models::ActivityRecord {
                                    id: String::new(),
                                    timestamp: String::new(),
                                    event_type: "execution_started".to_string(),
                                    summary: format!(
                                        "Execution started for task {} (iteration {})",
                                        task_id,
                                        iter.unwrap_or(0)
                                    ),
                                    project_id: project_id.clone(),
                                    task_id: Some(task_id.clone()),
                                    agent_id: None,
                                    session_id: None,
                                    iteration: iter,
                                    outcome: "info".to_string(),
                                    dedup_key: Some(format!("act:{}", dedup)),
                                    metadata: Some(val.clone()),
                                })
                                .await;
                        }
                        "task_completed" => {
                            let iter = val
                                .get("iteration")
                                .and_then(|v| v.as_u64())
                                .map(|x| x as u32);
                            let summary = format!("Task {} completed successfully", task_id);
                            let _ = self
                                .record_notification(crate::models::NotificationRecord {
                                    id: String::new(),
                                    created_at: String::new(),
                                    category: "task_completed".to_string(),
                                    severity: "success".to_string(),
                                    title: "Task completed".to_string(),
                                    message: summary.clone(),
                                    project_id: project_id.clone(),
                                    task_id: Some(task_id.clone()),
                                    session_id: None,
                                    agent_id: None,
                                    read: false,
                                    dedup_key: Some(format!("notif:{}", dedup)),
                                    action: Some("open_task".to_string()),
                                    metadata: Some(val.clone()),
                                })
                                .await;
                            let _ = self
                                .record_activity(crate::models::ActivityRecord {
                                    id: String::new(),
                                    timestamp: String::new(),
                                    event_type: "task_completed".to_string(),
                                    summary,
                                    project_id: project_id.clone(),
                                    task_id: Some(task_id.clone()),
                                    agent_id: None,
                                    session_id: None,
                                    iteration: iter,
                                    outcome: "success".to_string(),
                                    dedup_key: Some(format!("act:{}", dedup)),
                                    metadata: Some(val.clone()),
                                })
                                .await;
                        }
                        "task_cancelled" => {
                            let summary = format!("Task {} cancelled", task_id);
                            let _ = self
                                .record_notification(crate::models::NotificationRecord {
                                    id: String::new(),
                                    created_at: String::new(),
                                    category: "task_cancelled".to_string(),
                                    severity: "warning".to_string(),
                                    title: "Task cancelled".to_string(),
                                    message: summary.clone(),
                                    project_id: project_id.clone(),
                                    task_id: Some(task_id.clone()),
                                    session_id: None,
                                    agent_id: None,
                                    read: false,
                                    dedup_key: Some(format!("notif:{}", dedup)),
                                    action: Some("open_task".to_string()),
                                    metadata: Some(val.clone()),
                                })
                                .await;
                            let _ = self
                                .record_activity(crate::models::ActivityRecord {
                                    id: String::new(),
                                    timestamp: String::new(),
                                    event_type: "task_cancelled".to_string(),
                                    summary,
                                    project_id: project_id.clone(),
                                    task_id: Some(task_id.clone()),
                                    agent_id: None,
                                    session_id: None,
                                    iteration: None,
                                    outcome: "cancelled".to_string(),
                                    dedup_key: Some(format!("act:{}", dedup)),
                                    metadata: Some(val.clone()),
                                })
                                .await;
                        }
                        "task_failed" => {
                            let err = val.get("error").and_then(|v| v.as_str()).unwrap_or("");
                            let summary = format!("Task {} failed: {}", task_id, err);
                            let _ = self
                                .record_notification(crate::models::NotificationRecord {
                                    id: String::new(),
                                    created_at: String::new(),
                                    category: "task_failed".to_string(),
                                    severity: "error".to_string(),
                                    title: "Task failed".to_string(),
                                    message: summary.clone(),
                                    project_id: project_id.clone(),
                                    task_id: Some(task_id.clone()),
                                    session_id: None,
                                    agent_id: None,
                                    read: false,
                                    dedup_key: Some(format!("notif:{}", dedup)),
                                    action: Some("open_task".to_string()),
                                    metadata: Some(val.clone()),
                                })
                                .await;
                            let _ = self
                                .record_activity(crate::models::ActivityRecord {
                                    id: String::new(),
                                    timestamp: String::new(),
                                    event_type: "task_failed".to_string(),
                                    summary,
                                    project_id: project_id.clone(),
                                    task_id: Some(task_id.clone()),
                                    agent_id: None,
                                    session_id: None,
                                    iteration: None,
                                    outcome: "failure".to_string(),
                                    dedup_key: Some(format!("act:{}", dedup)),
                                    metadata: Some(val.clone()),
                                })
                                .await;
                        }
                        "component_finished" => {
                            let success = val
                                .get("success")
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);
                            if !success {
                                let _ = self
                                    .record_activity(crate::models::ActivityRecord {
                                        id: String::new(),
                                        timestamp: String::new(),
                                        event_type: "component_failed".to_string(),
                                        summary: format!("Component failed for task {}", task_id),
                                        project_id: project_id.clone(),
                                        task_id: Some(task_id.clone()),
                                        agent_id: None,
                                        session_id: None,
                                        iteration: val
                                            .get("iteration")
                                            .and_then(|v| v.as_u64())
                                            .map(|x| x as u32),
                                        outcome: "failure".to_string(),
                                        dedup_key: Some(format!("act:{}", dedup)),
                                        metadata: Some(val.clone()),
                                    })
                                    .await;
                            }
                        }
                        _ => {}
                    }
                }
            }
            // LifecycleEvent is a struct with event_type
            if let Some(et) = payload.get("event_type") {
                let lifecycle_type = et.as_str().unwrap_or("");
                let dedup = format!("mon:{}:lifecycle:{}", ev.sequence, lifecycle_type);
                let summary = format!("Process {} for task {}", lifecycle_type, task_id);
                let (outcome, notify) = match lifecycle_type {
                    "ProcessFailed" => ("failure", true),
                    "ProcessTimedOut" => ("failure", true),
                    "StallDetected" => ("failure", true),
                    "ProcessCancelled" => ("cancelled", true),
                    _ => ("info", false),
                };
                if notify {
                    let _ = self
                        .record_notification(crate::models::NotificationRecord {
                            id: String::new(),
                            created_at: String::new(),
                            category: lifecycle_type.to_lowercase(),
                            severity: "warning".to_string(),
                            title: lifecycle_type.to_string(),
                            message: summary.clone(),
                            project_id: project_id.clone(),
                            task_id: Some(task_id.clone()),
                            session_id: None,
                            agent_id: None,
                            read: false,
                            dedup_key: Some(format!("notif:{}", dedup)),
                            action: Some("open_task".to_string()),
                            metadata: Some(payload.clone()),
                        })
                        .await;
                }
                let _ = self
                    .record_activity(crate::models::ActivityRecord {
                        id: String::new(),
                        timestamp: String::new(),
                        event_type: lifecycle_type.to_lowercase(),
                        summary,
                        project_id: project_id.clone(),
                        task_id: Some(task_id.clone()),
                        agent_id: None,
                        session_id: None,
                        iteration: None,
                        outcome: outcome.to_string(),
                        dedup_key: Some(format!("act:{}", dedup)),
                        metadata: Some(payload.clone()),
                    })
                    .await;
            }
        }
    }

    pub async fn sync_task_state_notifications(&self) {
        let history = {
            let core = self.core.lock().await;
            read_task_history(&core.history_dir)
        };
        let mut changed: Vec<(crate::models::TaskRecord, Option<TaskState>)> = Vec::new();
        {
            let mut core = self.core.lock().await;
            for (id, task) in &history {
                let prev = core.last_task_states.get(id).cloned();
                if prev.as_ref() != Some(&task.state) {
                    changed.push((task.clone(), prev));
                    core.last_task_states.insert(id.clone(), task.state.clone());
                }
            }
        }
        for (task, _prev) in changed {
            let project_id = task.project_id.clone();
            let state = task.state.clone();
            let (event_type, outcome, notify, category, severity, title): (
                &str,
                &str,
                bool,
                &str,
                &str,
                &str,
            ) = match state {
                TaskState::Queued => (
                    "task_queued",
                    "info",
                    false,
                    "task_queued",
                    "info",
                    "Task queued",
                ),
                TaskState::Planning => (
                    "planning_started",
                    "info",
                    true,
                    "planning_started",
                    "info",
                    "Planning started",
                ),
                TaskState::AwaitingApproval => (
                    "awaiting_approval",
                    "info",
                    true,
                    "task_blocked",
                    "warning",
                    "Awaiting approval",
                ),
                TaskState::Executing => (
                    "execution_started",
                    "info",
                    true,
                    "execution_started",
                    "info",
                    "Execution started",
                ),
                TaskState::Verifying => (
                    "verification_started",
                    "info",
                    true,
                    "verification_started",
                    "info",
                    "Verification started",
                ),
                TaskState::Reviewing => (
                    "review_started",
                    "info",
                    true,
                    "review_started",
                    "info",
                    "Review started",
                ),
                TaskState::Retrying => (
                    "retry_started",
                    "info",
                    true,
                    "retry_started",
                    "warning",
                    "Retry started",
                ),
                TaskState::Completed => (
                    "task_completed",
                    "success",
                    true,
                    "task_completed",
                    "success",
                    "Task completed",
                ),
                TaskState::Failed => (
                    "task_failed",
                    "failure",
                    true,
                    "task_failed",
                    "error",
                    "Task failed",
                ),
                TaskState::Cancelled => (
                    "task_cancelled",
                    "cancelled",
                    true,
                    "task_cancelled",
                    "warning",
                    "Task cancelled",
                ),
                _ => (
                    "task_state_changed",
                    "info",
                    false,
                    "task_progress",
                    "info",
                    "Task updated",
                ),
            };
            let summary = format!("Task {} is {:?}", task.id, state);
            // activity always
            let _ = self
                .record_activity(crate::models::ActivityRecord {
                    id: String::new(),
                    timestamp: String::new(),
                    event_type: event_type.to_string(),
                    summary: summary.clone(),
                    project_id: project_id.clone(),
                    task_id: Some(task.id.clone()),
                    agent_id: task.agent_name.clone(),
                    session_id: task.source_session_id.clone(),
                    iteration: Some(task.iteration).filter(|x| *x > 0),
                    outcome: outcome.to_string(),
                    dedup_key: Some(format!("state:{}:{}", task.id, event_type)),
                    metadata: None,
                })
                .await;
            if notify {
                let msg = match state {
                    TaskState::Failed => task
                        .error
                        .clone()
                        .unwrap_or_else(|| "Task failed".to_string()),
                    _ => summary.clone(),
                };
                let _ = self
                    .record_notification(crate::models::NotificationRecord {
                        id: String::new(),
                        created_at: String::new(),
                        category: category.to_string(),
                        severity: severity.to_string(),
                        title: title.to_string(),
                        message: msg,
                        project_id: project_id.clone(),
                        task_id: Some(task.id.clone()),
                        session_id: task.source_session_id.clone(),
                        agent_id: task.agent_name.clone(),
                        read: false,
                        dedup_key: Some(format!("state:{}:{}", task.id, category)),
                        action: Some("open_task".to_string()),
                        metadata: None,
                    })
                    .await;
            }
        }
    }

    pub async fn approve_task(&self, task_id: String) -> Result<TaskRecord, String> {
        let orchestrator = self.runtime_orchestrator(&task_id).await?;
        self.hydrate_task_credential(&orchestrator).await?;
        let record = orchestrator::approve_plan(orchestrator, task_id).await?;
        self.emit_workspace().await;
        let _ = self.record_task_result_in_session(&record).await;
        Ok(record)
    }

    pub async fn reject_task(
        &self,
        task_id: String,
        reason: Option<String>,
    ) -> Result<TaskRecord, String> {
        let orchestrator = self.runtime_orchestrator(&task_id).await?;
        let record = orchestrator::reject_plan(orchestrator, task_id, reason).await?;
        self.emit_workspace().await;
        let _ = self.record_task_result_in_session(&record).await;
        Ok(record)
    }

    pub async fn cancel_task(&self, task_id: String) -> Result<TaskRecord, String> {
        let orchestrator = self.runtime_orchestrator(&task_id).await?;
        let record = orchestrator::cancel_task(orchestrator, task_id).await?;
        self.emit_workspace().await;
        let _ = self.record_task_result_in_session(&record).await;
        Ok(record)
    }

    /// When a task that was started from a conversation reaches a terminal
    /// state, append a summary message to the source session so the
    /// conversation retains the execution outcome.
    async fn record_task_result_in_session(&self, task: &TaskRecord) {
        let Some(session_id) = task.source_session_id.as_deref() else {
            return;
        };
        if session_id.is_empty() {
            return;
        }
        // Avoid duplicate session messages for the same task.
        {
            let core = self.core.lock().await;
            if core.reported_task_results.contains(task.id.as_str()) {
                return;
            }
        }
        let state_summary = match task.state {
            TaskState::Completed => "completed successfully".to_string(),
            TaskState::Failed => format!(
                "failed: {}",
                task.error.as_deref().unwrap_or("unknown error")
            ),
            TaskState::Cancelled => "cancelled".to_string(),
            _ => return,
        };
        let iteration_count = task.iteration;
        let changed_files = task
            .task_level_changes
            .as_ref()
            .map(|cf| cf.all_paths().len())
            .unwrap_or(0);
        let content = format!(
            "Task '{}' {} ({} iteration(s), {} file(s) changed).",
            task.user_task, state_summary, iteration_count, changed_files
        );
        let _ = self
            .send_session_message(session_id.to_string(), content)
            .await;
        // Mark as reported.
        {
            let mut core = self.core.lock().await;
            core.reported_task_results.insert(task.id.clone());
        }
    }

    /// Explicit retry. Reuses the saved plan when one exists and starts
    /// execution immediately; otherwise the task re-enters the queue to plan
    /// again from scratch.
    pub async fn retry_task(&self, task_id: String) -> Result<TaskRecord, String> {
        let record = self
            .task_status(&task_id)
            .await?
            .ok_or_else(|| "Task not found".to_string())?;
        if !record.state.is_terminal() {
            return Err("Only finished tasks can be retried.".to_string());
        }

        let (config, profile_id) = {
            let core = self.core.lock().await;
            let profile_id = core.profile_id_for(
                record.project_id.as_deref(),
                record.provider_profile_id.as_deref(),
            );
            let config = core.resolve_config(
                record.project_id.as_deref(),
                record.provider_profile_id.as_deref(),
            );
            (config, profile_id)
        };

        let has_plan = record.plan.is_some() && record.task_graph.is_some();
        let now = chrono::Utc::now().to_rfc3339();
        let new_record = TaskRecord {
            id: Uuid::new_v4().to_string(),
            user_task: record.user_task.clone(),
            project_root: record.project_root.clone(),
            state: if has_plan {
                TaskState::AwaitingApproval
            } else {
                TaskState::Queued
            },
            plan: record.plan.clone(),
            task_graph: record.task_graph.clone().map(|mut graph| {
                graph.reset_for_retry();
                graph
            }),
            iteration: 0,
            max_iterations: record.max_iterations,
            iterations: Vec::new(),
            created_at: now.clone(),
            updated_at: now.clone(),
            error: None,
            project_id: record.project_id.clone(),
            provider_profile_id: profile_id,
            agent_name: record.agent_name.clone(),
            priority: record.priority,
            queued_at: Some(now),
            recovery_attempts: record.recovery_attempts.saturating_add(1),
            task_level_changes: None,
            task_baseline: None,
            execution_config: record.execution_config.clone(),
            planning_context: record.planning_context.clone(),
            source_session_id: record.source_session_id.clone(),
            selected_skill_ids: record.selected_skill_ids.clone(),
            task_model_id: record.task_model_id.clone(),
        };

        let orchestrator = self.attach_task(config, new_record.clone()).await;
        {
            let core = self.core.lock().await;
            // Preserve persisted model preferences and task-level override on
            // the writer used for disk persistence during recovery retries.
            let mut writer = Orchestrator::new_isolated(
                None,
                core.history_dir.clone(),
                core.config_path.clone(),
            );
            writer.model_preferences = Some(core.model_selector.preferences().clone());
            if let Some(tid) = new_record.task_model_id.clone() {
                writer.task_model_id = Some(tid);
            }
            writer.save_task_to_disk(&new_record);
        }

        if has_plan {
            self.hydrate_task_credential(&orchestrator).await?;
            let record =
                orchestrator::resume_execution(orchestrator, new_record.id.clone()).await?;
            self.emit_workspace().await;
            Ok(record)
        } else {
            // Leave the task queued; the scheduler starts it when a slot frees.
            self.emit_workspace().await;
            Ok(new_record)
        }
    }

    async fn runtime_orchestrator(&self, task_id: &str) -> Result<SharedOrchestrator, String> {
        let core = self.core.lock().await;
        core.runtimes
            .get(task_id)
            .map(|runtime| runtime.orchestrator.clone())
            .ok_or_else(|| format!("No active runtime for task {}", task_id))
    }

    async fn attach_task(&self, config: ProviderConfig, record: TaskRecord) -> SharedOrchestrator {
        let (tracker, app_handle, history_dir, config_path) = {
            let core = self.core.lock().await;
            (
                core.token_usage_tracker.clone(),
                core.app_handle.clone(),
                core.history_dir.clone(),
                core.config_path.clone(),
            )
        };
        let orchestrator = Arc::new(Mutex::new(Orchestrator::with_task(
            app_handle,
            history_dir,
            config_path,
            config,
            record.clone(),
        )));
        let monitoring_hub = { self.core.lock().await.monitoring_hub.clone() };
        orchestrator.lock().await.set_monitoring_hub(monitoring_hub);
        orchestrator
            .lock()
            .await
            .set_token_usage_tracker(Some(tracker));
        let mut core = self.core.lock().await;
        core.runtimes.insert(
            record.id.clone(),
            TaskRuntime {
                orchestrator: orchestrator.clone(),
                project_id: record.project_id.clone(),
                priority: record.priority,
            },
        );
        orchestrator
    }

    async fn emit_workspace(&self) {
        let core = self.core.lock().await;
        core.emit_workspace();
    }

    /// Start queued tasks while concurrency slots are available.
    pub async fn scheduler_tick(&self) {
        let (paused, max_slots, mut candidates, monitoring_hub, mut active_projects) = {
            let core = self.core.lock().await;
            let paused = core.settings.scheduler_paused;
            let max_slots = core.settings.max_concurrent_tasks.max(1) as usize;
            let mut active = 0usize;
            let mut candidates: Vec<(
                i32,
                String,
                String,
                SharedOrchestrator,
                bool,
                Option<String>,
            )> = Vec::new();
            let mut active_projects: HashSet<String> = HashSet::new();
            for runtime in core.runtimes.values() {
                match runtime.orchestrator.try_lock() {
                    Ok(guard) => {
                        if let Some(ref task) = guard.current_task {
                            if task.state.occupies_execution_slot() {
                                active += 1;
                                if let Some(pid) = runtime
                                    .project_id
                                    .clone()
                                    .or_else(|| task.project_id.clone())
                                {
                                    active_projects.insert(pid);
                                }
                            }
                            if task.state == TaskState::Queued {
                                let has_plan = task.plan.is_some() && task.task_graph.is_some();
                                candidates.push((
                                    task.priority,
                                    task.queued_at
                                        .clone()
                                        .unwrap_or_else(|| task.created_at.clone()),
                                    task.id.clone(),
                                    runtime.orchestrator.clone(),
                                    has_plan,
                                    runtime
                                        .project_id
                                        .clone()
                                        .or_else(|| task.project_id.clone()),
                                ));
                            }
                        }
                    }
                    Err(_) => active += 1,
                }
            }
            (
                paused,
                (max_slots, active),
                candidates,
                core.monitoring_hub.clone(),
                active_projects,
            )
        };
        let (max_slots, mut active) = max_slots;

        if paused {
            return;
        }

        // Keep notification/activity stores in sync with actual task states and
        // monitoring events.
        self.sync_task_state_notifications().await;
        self.process_monitoring_events().await;

        // Run the automatic stall watchdog against all active executor
        // processes. This is the background service that keeps stall
        // detection connected to the scheduler tick rather than a separate
        // competing loop.
        stall_watchdog::check_stalls(&monitoring_hub);

        candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

        for (_, _, task_id, orchestrator, has_plan, project_id) in candidates.drain(..) {
            if active >= max_slots {
                break;
            }
            // Same-project serialization: do not start a second task in a
            // project that already has an active executing task.
            if let Some(pid) = project_id.as_ref() {
                if active_projects.contains(pid) {
                    continue;
                }
            }
            if let Err(error) = self.hydrate_task_credential(&orchestrator).await {
                let core = self.core.lock().await;
                core.emit_log(&format!("credential_store: {error}"));
            }
            let result = if has_plan {
                orchestrator::resume_execution(orchestrator, task_id.clone()).await
            } else {
                orchestrator::begin_planning(orchestrator, task_id.clone()).await
            };
            if result.is_ok() {
                active += 1;
                if let Some(pid) = project_id.as_ref() {
                    active_projects.insert(pid.clone());
                }
            }
        }

        if !candidates.is_empty() {
            self.emit_workspace().await;
        }
    }

    // -----------------------------------------------------------------------
    // Sessions
    // -----------------------------------------------------------------------

    pub async fn create_session(
        &self,
        options: CreateSessionOptions,
    ) -> Result<SessionRecord, String> {
        let kind = options.kind.unwrap_or(SessionKind::Prompt);
        let core = self.core.lock().await;

        let project = match options.project_id.as_deref() {
            Some(id) => Some(
                core.project(id)
                    .ok_or_else(|| "Project not found".to_string())?,
            ),
            None => None,
        };
        let project_root = project.map(|project| project.root.clone());
        let profile = core.resolve_profile(
            options.project_id.as_deref(),
            options.provider_profile_id.as_deref(),
        );
        let profile_id = profile.map(|profile| profile.id.clone());

        let (cli_preset_id, cli_executable, cli_args, prompt_delivery) = match kind {
            SessionKind::Prompt => (None, None, Vec::new(), PromptDelivery::default()),
            SessionKind::Cli => {
                let preset = options.cli_preset_id.as_deref().and_then(find_cli_preset);
                let executable = options
                    .cli_executable
                    .clone()
                    .or_else(|| {
                        preset
                            .as_ref()
                            .map(|preset| preset.default_executable.clone())
                    })
                    .unwrap_or_default();
                if executable.trim().is_empty() {
                    return Err(
                        "Choose a CLI coding agent or provide an executable for this session."
                            .to_string(),
                    );
                }
                let args = options.cli_args.clone().unwrap_or_else(|| {
                    preset
                        .as_ref()
                        .map(|preset| preset.default_args.clone())
                        .unwrap_or_default()
                });
                let delivery = preset
                    .as_ref()
                    .map(|preset| preset.prompt_delivery)
                    .unwrap_or_default();
                (
                    preset.map(|preset| preset.id),
                    Some(executable),
                    args,
                    delivery,
                )
            }
        };

        if kind == SessionKind::Prompt {
            let config = profile
                .map(|profile| profile.config.clone())
                .unwrap_or_default();
            if config
                .cloud_endpoint
                .as_deref()
                .map(str::trim)
                .unwrap_or("")
                .is_empty()
            {
                return Err(
                    "Prompt sessions need a provider profile with an HTTP endpoint (cloud API or \
                     local model server). Configure one in Provider Profiles."
                        .to_string(),
                );
            }
        }

        let now = chrono::Utc::now().to_rfc3339();
        let title = options
            .title
            .map(|title| title.trim().to_string())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| match kind {
                SessionKind::Prompt => "New prompt session".to_string(),
                SessionKind::Cli => "New CLI session".to_string(),
            });

        let session = SessionRecord {
            id: Uuid::new_v4().to_string(),
            title,
            kind,
            project_id: options.project_id,
            project_root,
            provider_profile_id: profile_id,
            cli_preset_id,
            cli_executable,
            cli_args,
            prompt_delivery,
            messages: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
            busy: false,
            error: None,
            archived: false,
        };

        let mut core = core;
        core.sessions.push(session.clone());
        core.save_session(&session);
        core.emit_session(&session);
        core.emit_workspace();
        Ok(session)
    }

    pub async fn list_sessions(
        &self,
        project_id: Option<String>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<SessionRecord>, String> {
        let core = self.core.lock().await;
        let mut sessions: Vec<SessionRecord> = core
            .sessions
            .iter()
            .filter(|session| {
                // Archived sessions are excluded from default listings.
                if session.archived {
                    return false;
                }
                project_id
                    .as_deref()
                    .map(|id| session.project_id.as_deref() == Some(id))
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        // Pagination: optional offset then limit.
        let offset = offset.unwrap_or(0);
        if offset > 0 {
            if offset >= sessions.len() {
                return Ok(Vec::new());
            }
            sessions = sessions.split_off(offset);
        }
        if let Some(limit) = limit {
            sessions.truncate(limit);
        }
        Ok(sessions)
    }

    pub async fn get_session(&self, session_id: String) -> Result<SessionRecord, String> {
        let core = self.core.lock().await;
        core.sessions
            .iter()
            .find(|session| session.id == session_id)
            .cloned()
            .ok_or_else(|| "Session not found".to_string())
    }

    /// Whether a session belongs to the given project. Sessions without a
    /// project are never considered to belong to one.
    pub fn session_belongs_to_project(session: &SessionRecord, project_id: &str) -> bool {
        session.project_id.as_deref() == Some(project_id)
    }

    pub async fn validate_session_project(
        &self,
        session_id: &str,
        project_id: &str,
    ) -> Result<(), String> {
        let core = self.core.lock().await;
        let session = core
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| "Session not found".to_string())?;
        if session.project_id.as_deref() != Some(project_id) {
            return Err("Session does not belong to this project".to_string());
        }
        Ok(())
    }

    /// Return a paginated slice of a conversation's messages without loading
    /// the full session record's other fields. Useful for large conversations.
    pub async fn get_session_messages(
        &self,
        session_id: String,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<crate::models::SessionMessage>, String> {
        let core = self.core.lock().await;
        let session = core
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| "Session not found".to_string())?;
        let mut messages = session.messages.clone();
        let offset = offset.unwrap_or(0);
        if offset > 0 {
            if offset >= messages.len() {
                return Ok(Vec::new());
            }
            messages = messages.split_off(offset);
        }
        if let Some(limit) = limit {
            messages.truncate(limit);
        }
        Ok(messages)
    }

    pub async fn delete_session(&self, session_id: String) -> Result<(), String> {
        let mut core = self.core.lock().await;
        let before = core.sessions.len();
        core.sessions.retain(|session| session.id != session_id);
        if core.sessions.len() == before {
            return Err("Session not found".to_string());
        }
        // Deleting a session while its CLI turn is running must not leave the
        // process behind.
        if let Some(handle) = core.cli_turns.remove(&session_id) {
            let _ = handle.cancel_tx.send(true);
        }
        core.delete_session_file(&session_id);
        core.emit_workspace();
        Ok(())
    }

    /// Archive (or restore) a conversation. Archived conversations are excluded
    /// from default listings and cannot receive new messages until restored.
    pub async fn archive_session(
        &self,
        session_id: String,
        archived: bool,
    ) -> Result<SessionRecord, String> {
        let mut core = self.core.lock().await;
        let session = core
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
            .ok_or_else(|| "Session not found".to_string())?;
        if session.archived == archived {
            return Ok(session.clone());
        }
        session.archived = archived;
        session.updated_at = chrono::Utc::now().to_rfc3339();
        let updated = session.clone();
        core.save_session(&updated);
        core.emit_session(&updated);
        core.emit_workspace();
        Ok(updated)
    }

    /// Update a conversation's metadata (title, project association).
    /// Only managed fields may be changed; messages and execution state are
    /// untouched.
    pub async fn update_session(
        &self,
        session_id: String,
        title: Option<String>,
        project_id: Option<String>,
    ) -> Result<SessionRecord, String> {
        let mut core = self.core.lock().await;

        // Check the session exists before borrowing mutably.
        let session_exists = core.sessions.iter().any(|s| s.id == session_id);
        if !session_exists {
            return Err("Session not found".to_string());
        }

        // Resolve project info while core is still immutably borrowed.
        let project_resolution = match &project_id {
            None => None,
            Some(pid) if pid.trim().is_empty() => Some(None),
            Some(pid) => {
                let trimmed = pid.trim();
                Some(Some(
                    core.project(trimmed)
                        .map(|project| (project.id.clone(), project.root.clone()))
                        .ok_or_else(|| "Project not found.".to_string())?,
                ))
            }
        };

        let session = core
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
            .ok_or_else(|| "Session not found".to_string())?;

        if let Some(new_title) = title {
            let trimmed = new_title.trim().to_string();
            if trimmed.is_empty() {
                return Err("Session title cannot be empty.".to_string());
            }
            session.title = trimmed;
        }

        match project_resolution {
            None => {}
            Some(None) => {
                session.project_id = None;
                session.project_root = None;
            }
            Some(Some((pid, root))) => {
                session.project_id = Some(pid);
                session.project_root = Some(root);
            }
        }

        session.updated_at = chrono::Utc::now().to_rfc3339();
        let updated = session.clone();
        core.save_session(&updated);
        core.emit_session(&updated);
        core.emit_workspace();
        Ok(updated)
    }

    /// List conversations, optionally including archived ones. When
    /// `include_archived` is false, archived sessions are excluded.
    /// List conversations including archived ones. Used by the UI when the
    /// user wants to view or restore archived conversations.
    pub async fn list_sessions_including_archived(
        &self,
        project_id: Option<String>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Vec<SessionRecord>, String> {
        let core = self.core.lock().await;
        let mut sessions: Vec<SessionRecord> = core
            .sessions
            .iter()
            .filter(|session| {
                project_id
                    .as_deref()
                    .map(|id| session.project_id.as_deref() == Some(id))
                    .unwrap_or(true)
            })
            .cloned()
            .collect();
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        let offset = offset.unwrap_or(0);
        if offset > 0 {
            if offset >= sessions.len() {
                return Ok(Vec::new());
            }
            sessions = sessions.split_off(offset);
        }
        if let Some(limit) = limit {
            sessions.truncate(limit);
        }
        Ok(sessions)
    }

    // ---------------------------------------------------------------------------
    // Model Selection Methods
    // ---------------------------------------------------------------------------

    /// List all available models from all providers
    pub async fn list_available_models(&self) -> Result<Vec<ModelInfo>, String> {
        let core = self.core.lock().await;
        Ok(core
            .model_selector
            .get_all_available_models()
            .into_iter()
            .cloned()
            .collect())
    }

    /// Get current model preferences
    pub async fn get_model_preferences(&self) -> Result<ModelPreferences, String> {
        let core = self.core.lock().await;
        Ok(core.model_selector.preferences().clone())
    }

    /// Update model preferences
    pub async fn update_model_preferences(
        &self,
        preferences: ModelPreferences,
    ) -> Result<ModelPreferences, String> {
        let mut core = self.core.lock().await;
        core.model_selector
            .preferences_mut()
            .set_auto_selection(preferences.auto_selection_enabled);
        if let Some(default_model) = preferences.default_model_id {
            core.model_selector
                .preferences_mut()
                .set_default_model(Some(default_model));
        }
        for (responsibility, pref) in preferences.responsibility_preferences {
            core.model_selector
                .preferences_mut()
                .set_responsibility_preference(responsibility, pref);
        }
        Ok(core.model_selector.preferences().clone())
    }

    /// Select a model for a specific task
    pub async fn select_model_for_task(
        &self,
        task_id: String,
        model_id: String,
    ) -> Result<ModelSelection, String> {
        let mut core = self.core.lock().await;

        // Mark this as a manual override
        core.model_selector
            .preferences_mut()
            .set_manual_override(task_id.clone(), model_id.clone());

        // Select the model
        let selection =
            core.model_selector
                .select_model(Some(&task_id), AiResponsibility::Default, None);

        selection.ok_or_else(|| format!("Model {} not found or not available", model_id))
    }

    /// Select a model for a specific responsibility
    pub async fn select_model_for_responsibility(
        &self,
        responsibility: AiResponsibility,
        model_id: String,
    ) -> Result<ModelPreferences, String> {
        let mut core = self.core.lock().await;

        let pref = ResponsibilityModelPreference {
            model_id: Some(model_id.clone()),
            auto_select: false,
        };

        core.model_selector
            .preferences_mut()
            .set_responsibility_preference(responsibility, pref);

        // Verify the model is available
        if !core.model_selector.is_model_available(&model_id) {
            return Err(format!("Model {} is not available", model_id));
        }

        Ok(core.model_selector.preferences().clone())
    }

    /// Set the default model
    pub async fn set_default_model(
        &self,
        model_id: Option<String>,
    ) -> Result<ModelPreferences, String> {
        let mut core = self.core.lock().await;

        if let Some(ref mid) = model_id {
            if !core.model_selector.is_model_available(mid) {
                return Err(format!("Model {} is not available", mid));
            }
        }

        core.model_selector
            .preferences_mut()
            .set_default_model(model_id);
        Ok(core.model_selector.preferences().clone())
    }

    /// Get model selection for a specific context
    pub async fn get_model_selection(
        &self,
        task_id: Option<String>,
        responsibility: Option<AiResponsibility>,
        provider_id: Option<String>,
    ) -> Result<Option<ModelSelection>, String> {
        let core = self.core.lock().await;
        let responsibility = responsibility.unwrap_or(AiResponsibility::Default);
        Ok(core.model_selector.select_model(
            task_id.as_deref(),
            responsibility,
            provider_id.as_deref(),
        ))
    }

    /// Mark a model as deprecated or not
    pub async fn mark_model_deprecated(
        &self,
        model_id: String,
        deprecated: bool,
    ) -> Result<(), String> {
        let mut core = self.core.lock().await;

        if deprecated {
            core.model_selector.mark_deprecated(model_id);
        } else {
            // To unmark, we'd need to remove from deprecated set
            // For now, just clear all deprecated/invalid marks
            core.model_selector.clear_model_status();
        }

        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Token Usage Methods
    // ---------------------------------------------------------------------------

    /// Record token usage
    pub async fn record_token_usage(
        &self,
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        provider_id: Option<String>,
        provider_name: Option<String>,
        model_id: Option<String>,
        model_name: Option<String>,
        project_id: Option<String>,
        task_id: Option<String>,
        session_id: Option<String>,
        responsibility: Option<String>,
        operation_type: String,
    ) -> Result<TokenUsageRecord, String> {
        let core = self.core.lock().await;

        let usage = TokenUsage::from_provider(input_tokens, output_tokens, None);
        let context = UsageContext {
            provider_id,
            provider_name,
            model_id,
            model_name,
            project_id,
            project_name: None,
            task_id,
            session_id,
            responsibility,
            metadata: HashMap::new(),
        };

        let mut tracker = core
            .token_usage_tracker
            .lock()
            .map_err(|_| "Token usage tracker lock poisoned".to_string())?;
        let record = tracker.record_usage(usage, context, operation_type);

        // Save to disk
        let _ = tracker.save();

        Ok(record)
    }

    /// Get overall token usage statistics
    pub async fn get_token_usage_stats(&self) -> Result<TokenUsageStats, String> {
        let core = self.core.lock().await;
        let tracker = core
            .token_usage_tracker
            .lock()
            .map_err(|_| "Token usage tracker lock poisoned".to_string())?;
        Ok(tracker.get_stats())
    }

    /// Get token usage statistics for a specific task
    pub async fn get_token_usage_by_task(
        &self,
        task_id: String,
    ) -> Result<TokenUsageStats, String> {
        let core = self.core.lock().await;
        let tracker = core
            .token_usage_tracker
            .lock()
            .map_err(|_| "Token usage tracker lock poisoned".to_string())?;
        Ok(tracker.get_stats_by_task(&task_id))
    }

    /// Get token usage statistics for a specific provider
    pub async fn get_token_usage_by_provider(
        &self,
        provider_id: String,
    ) -> Result<TokenUsageStats, String> {
        let core = self.core.lock().await;
        let tracker = core
            .token_usage_tracker
            .lock()
            .map_err(|_| "Token usage tracker lock poisoned".to_string())?;
        Ok(tracker.get_stats_by_provider(&provider_id))
    }

    /// Get token usage statistics for a specific model
    pub async fn get_token_usage_by_model(
        &self,
        model_id: String,
    ) -> Result<TokenUsageStats, String> {
        let core = self.core.lock().await;
        let tracker = core
            .token_usage_tracker
            .lock()
            .map_err(|_| "Token usage tracker lock poisoned".to_string())?;
        Ok(tracker.get_stats_by_model(&model_id))
    }

    /// Get token usage statistics for a specific project
    pub async fn get_token_usage_by_project(
        &self,
        project_id: String,
    ) -> Result<TokenUsageStats, String> {
        let core = self.core.lock().await;
        let tracker = core
            .token_usage_tracker
            .lock()
            .map_err(|_| "Token usage tracker lock poisoned".to_string())?;
        Ok(tracker.get_stats_by_project(&project_id))
    }

    /// Get token usage records with optional filters
    pub async fn get_token_usage_records(
        &self,
        task_id: Option<String>,
        provider_id: Option<String>,
        model_id: Option<String>,
        project_id: Option<String>,
        start_time: Option<String>,
        end_time: Option<String>,
    ) -> Result<Vec<TokenUsageRecord>, String> {
        let core = self.core.lock().await;

        let mut all_records = core
            .token_usage_tracker
            .lock()
            .map_err(|_| "Token usage tracker lock poisoned".to_string())?
            .get_all_records();

        // Apply filters
        if let Some(task_id) = task_id {
            all_records.retain(|r| r.context.task_id.as_deref() == Some(task_id.as_str()));
        }
        if let Some(provider_id) = provider_id {
            all_records.retain(|r| r.context.provider_id.as_deref() == Some(provider_id.as_str()));
        }
        if let Some(model_id) = model_id {
            all_records.retain(|r| r.context.model_id.as_deref() == Some(model_id.as_str()));
        }
        if let Some(project_id) = project_id {
            all_records.retain(|r| r.context.project_id.as_deref() == Some(project_id.as_str()));
        }
        if let (Some(start), Some(end)) = (start_time, end_time) {
            all_records.retain(|r| r.created_at >= start && r.created_at <= end);
        }

        Ok(all_records)
    }

    /// Clear all token usage data
    pub async fn clear_token_usage(&self) -> Result<(), String> {
        let core = self.core.lock().await;
        let mut tracker = core
            .token_usage_tracker
            .lock()
            .map_err(|_| "Token usage tracker lock poisoned".to_string())?;
        tracker.clear_all();
        let _ = tracker.save();
        Ok(())
    }

    // ---------------------------------------------------------------------------
    // Coding-Agent Registry Methods
    // ---------------------------------------------------------------------------

    pub async fn list_registered_agents(
        &self,
    ) -> Result<Vec<crate::agent_registry::RegisteredCodingAgent>, String> {
        let path = {
            let core = self.core.lock().await;
            core.config_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("coding_agents_registry.json")
        };
        let file = crate::agent_registry::load_registry(&path);
        Ok(file.agents)
    }

    pub async fn refresh_coding_agents(
        &self,
    ) -> Result<crate::agent_registry::DetectionReport, String> {
        let path = {
            let core = self.core.lock().await;
            core.config_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("coding_agents_registry.json")
        };
        let report = crate::agent_registry::refresh_registry(&path).await;
        match &report {
            Ok(detected) => {
                let _ = crate::agent_registry::save_registry(&path, detected);
            }
            Err(e) => return Err(e.clone()),
        }
        let report = crate::agent_registry::detect_known_agents().await;
        Ok(report)
    }

    pub async fn register_coding_agent(
        &self,
        input: crate::agent_registry::ManualAgentInput,
    ) -> Result<crate::agent_registry::RegisteredCodingAgent, String> {
        let path = {
            let core = self.core.lock().await;
            core.config_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("coding_agents_registry.json")
        };
        crate::agent_registry::upsert_manual_agent(&path, input).await
    }

    pub async fn remove_coding_agent(&self, id: String) -> Result<(), String> {
        let path = {
            let core = self.core.lock().await;
            core.config_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("coding_agents_registry.json")
        };
        crate::agent_registry::remove_registered_agent(&path, &id)
    }

    pub async fn detect_coding_agents(
        &self,
    ) -> Result<crate::agent_registry::DetectionReport, String> {
        Ok(crate::agent_registry::detect_known_agents().await)
    }

    pub async fn send_session_message(
        &self,
        session_id: String,
        content: String,
    ) -> Result<SessionRecord, String> {
        let content = content.trim().to_string();
        if content.is_empty() {
            return Err("Message cannot be empty".to_string());
        }

        let turn_id = Uuid::new_v4().to_string();
        let mut cli_cancel_rx: Option<watch::Receiver<bool>> = None;

        // Snapshot everything needed for the request, then release the lock.
        let (session, mut resolved_config, profile_label, resolved_profile_id) = {
            let mut core = self.core.lock().await;
            let existing = core
                .sessions
                .iter()
                .find(|session| session.id == session_id)
                .ok_or_else(|| "Session not found".to_string())?;
            if existing.archived {
                return Err(
                    "This conversation is archived. Restore it before sending new messages."
                        .to_string(),
                );
            }
            let busy_count = core.sessions.iter().filter(|session| session.busy).count();
            let max_sessions = core.settings.max_concurrent_sessions.max(1) as usize;
            let (project_id, profile_id) = {
                let existing = core
                    .sessions
                    .iter()
                    .find(|session| session.id == session_id)
                    .ok_or_else(|| "Session not found".to_string())?;
                (
                    existing.project_id.clone(),
                    existing.provider_profile_id.clone(),
                )
            };
            // Resolve exactly the way the request will: explicit profile first,
            // then the project default, then the global default.
            let resolved_profile =
                core.resolve_profile(project_id.as_deref(), profile_id.as_deref());
            let resolved_config = resolved_profile
                .map(|profile| profile.config.clone())
                .unwrap_or_default();
            let profile_label = resolved_profile.map(|profile| profile.name.clone());
            let resolved_profile_id = resolved_profile.map(|profile| profile.id.clone());
            let session = core
                .sessions
                .iter_mut()
                .find(|session| session.id == session_id)
                .ok_or_else(|| "Session not found".to_string())?;
            if session.busy {
                return Err("This session is already waiting for a response.".to_string());
            }
            if busy_count >= max_sessions {
                return Err(format!(
                    "Too many sessions are active. Wait for one of the {} running sessions to finish.",
                    max_sessions
                ));
            }
            session.busy = true;
            session.error = None;
            let is_cli = session.kind == SessionKind::Cli;
            let session = session.clone();
            if is_cli {
                let (cancel_tx, cancel_rx) = watch::channel(false);
                core.cli_turns.insert(
                    session_id.clone(),
                    CliTurnHandle {
                        turn_id: turn_id.clone(),
                        cancel_tx,
                    },
                );
                cli_cancel_rx = Some(cancel_rx);
            }
            (session, resolved_config, profile_label, resolved_profile_id)
        };

        let credential_error = if session.kind == SessionKind::Prompt {
            match self
                .retrieve_profile_credential(resolved_profile_id.as_deref(), &resolved_config)
                .await
            {
                Ok(key) => {
                    resolved_config.api_key = key;
                    None
                }
                Err(error) => Some(error),
            }
        } else {
            None
        };

        let now = chrono::Utc::now().to_rfc3339();
        let user_message = SessionMessage {
            id: Uuid::new_v4().to_string(),
            role: "user".to_string(),
            content: content.clone(),
            created_at: now,
            duration_ms: None,
            exit_code: None,
            error: None,
        };

        {
            let mut core = self.core.lock().await;
            if let Some(stored) = core
                .sessions
                .iter_mut()
                .find(|stored| stored.id == session_id)
            {
                stored.messages.push(user_message.clone());
                stored.updated_at = chrono::Utc::now().to_rfc3339();
                let updated = stored.clone();
                core.save_session(&updated);
            }
        }

        // Prompt sessions may stream incrementally; CLI sessions remain
        // complete-response (possibly buffered) turns.
        let mut prompt_cancel_rx: Option<watch::Receiver<bool>> = None;
        let mut prompt_assistant_message_id: Option<String> = None;

        let started = std::time::Instant::now();

        let result: Result<(String, Option<i32>), String> = match session.kind {
            SessionKind::Prompt => match credential_error {
                Some(error) => Err(error),
                None => {
                    // Register a cancellation handle so prompt turns can be
                    // cancelled like CLI turns.
                    let (cancel_tx, cancel_rx) = watch::channel(false);
                    {
                        let mut core = self.core.lock().await;
                        core.prompt_turns.insert(
                            session_id.clone(),
                            CliTurnHandle {
                                turn_id: turn_id.clone(),
                                cancel_tx,
                            },
                        );
                    }
                    prompt_cancel_rx = Some(cancel_rx.clone());
                    let assistant_message_id = Uuid::new_v4().to_string();
                    prompt_assistant_message_id = Some(assistant_message_id.clone());
                    let partial = Arc::new(std::sync::Mutex::new(String::new()));

                    let app_handle = self.core.lock().await.app_handle.clone();
                    // Emit "started".
                    if let Some(h) = &app_handle {
                        let _ = h.emit(
                            "session_stream",
                            serde_json::json!({
                                "session_id": session_id,
                                "message_id": assistant_message_id,
                                "turn_id": turn_id,
                                "kind": "started",
                                "delta": null,
                                "content": null,
                                "error": null,
                            }),
                        );
                    }

                    let mut cancel_rx = cancel_rx;
                    let sid = session_id.clone();
                    let mid = assistant_message_id.clone();
                    let tid = turn_id.clone();
                    let handle_for_delta = app_handle.clone();
                    let partial_for_delta = partial.clone();
                    let on_delta = move |delta: String| {
                        if let Ok(mut guard) = partial_for_delta.lock() {
                            guard.push_str(&delta);
                        }
                        if let Some(h) = &handle_for_delta {
                            let _ = h.emit(
                                "session_stream",
                                serde_json::json!({
                                    "session_id": sid,
                                    "message_id": mid,
                                    "turn_id": tid,
                                    "kind": "delta",
                                    "delta": delta,
                                    "content": null,
                                    "error": null,
                                }),
                            );
                        }
                    };

                    match run_prompt_stream_turn(
                        &resolved_config,
                        &session,
                        &content,
                        profile_label.as_deref(),
                        &mut cancel_rx,
                        on_delta,
                    )
                    .await
                    {
                        Ok(text) => {
                            if let Some(h) = &app_handle {
                                let _ = h.emit(
                                    "session_stream",
                                    serde_json::json!({
                                        "session_id": session_id,
                                        "message_id": assistant_message_id,
                                        "turn_id": turn_id,
                                        "kind": "completed",
                                        "delta": null,
                                        "content": text,
                                        "error": null,
                                    }),
                                );
                            }
                            Ok((text, None))
                        }
                        Err(error) => {
                            let was_cancelled = cancel_rx.borrow().clone();
                            let terminal_kind = if was_cancelled { "cancelled" } else { "failed" };
                            let partial_text =
                                partial.lock().map(|g| g.clone()).unwrap_or_default();
                            if let Some(h) = &app_handle {
                                let _ = h.emit(
                                    "session_stream",
                                    serde_json::json!({
                                        "session_id": session_id,
                                        "message_id": assistant_message_id,
                                        "turn_id": turn_id,
                                        "kind": terminal_kind,
                                        "delta": null,
                                        "content": if partial_text.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(partial_text.clone()) },
                                        "error": if was_cancelled { serde_json::Value::Null } else { serde_json::Value::String(error.clone()) },
                                    }),
                                );
                            }
                            if was_cancelled {
                                // Preserve partial output by returning it as a
                                // successful assistant message.
                                if !partial_text.is_empty() {
                                    Ok((partial_text, None))
                                } else {
                                    Err("Turn cancelled by user.".to_string())
                                }
                            } else {
                                Err(error)
                            }
                        }
                    }
                }
            },
            SessionKind::Cli => {
                let cancel_rx = cli_cancel_rx
                    .as_mut()
                    .expect("CLI sessions must have a cancellation receiver");
                run_cli_turn(&session, &content, cancel_rx).await
            }
        };

        let duration_ms = started.elapsed().as_millis() as u64;
        let mut core = self.core.lock().await;
        let was_cancelled = cli_cancel_rx
            .as_ref()
            .map(|receiver| *receiver.borrow())
            .unwrap_or(false)
            || prompt_cancel_rx
                .as_ref()
                .map(|receiver| *receiver.borrow())
                .unwrap_or(false);

        // Clean up cancellation handles.
        core.finish_cli_turn(&session_id, &turn_id);
        // prompt finish
        let is_current_prompt = core
            .prompt_turns
            .get(&session_id)
            .map(|h| h.turn_id == turn_id)
            .unwrap_or(false);
        if is_current_prompt {
            core.prompt_turns.remove(&session_id);
        }

        let stored = core
            .sessions
            .iter_mut()
            .find(|stored| stored.id == session_id)
            .ok_or_else(|| "Session not found".to_string())?;
        stored.busy = false;
        stored.updated_at = chrono::Utc::now().to_rfc3339();
        if was_cancelled && session.kind == SessionKind::Cli {
            stored.error = None;
            stored.messages.push(SessionMessage {
                id: Uuid::new_v4().to_string(),
                role: "system".to_string(),
                content: "Turn cancelled by user.".to_string(),
                created_at: chrono::Utc::now().to_rfc3339(),
                duration_ms: Some(duration_ms),
                exit_code: None,
                error: None,
            });
        } else if was_cancelled && session.kind == SessionKind::Prompt {
            // Partial output already persisted as assistant message below? We
            // returned Ok(partial) in that case, handled in success branch.
            if result.is_err() {
                stored.error = None;
                stored.messages.push(SessionMessage {
                    id: Uuid::new_v4().to_string(),
                    role: "system".to_string(),
                    content: "Turn cancelled by user.".to_string(),
                    created_at: chrono::Utc::now().to_rfc3339(),
                    duration_ms: Some(duration_ms),
                    exit_code: None,
                    error: None,
                });
            } else {
                // success with partial or full text
                if let Ok((text, exit_code)) = &result {
                    stored.messages.push(SessionMessage {
                        id: prompt_assistant_message_id
                            .clone()
                            .unwrap_or_else(|| Uuid::new_v4().to_string()),
                        role: "assistant".to_string(),
                        content: text.clone(),
                        created_at: chrono::Utc::now().to_rfc3339(),
                        duration_ms: Some(duration_ms),
                        exit_code: *exit_code,
                        error: None,
                    });
                }
            }
        } else {
            match result {
                Ok((text, exit_code)) => {
                    let msg_id = if session.kind == SessionKind::Prompt {
                        prompt_assistant_message_id
                            .clone()
                            .unwrap_or_else(|| Uuid::new_v4().to_string())
                    } else {
                        Uuid::new_v4().to_string()
                    };
                    stored.messages.push(SessionMessage {
                        id: msg_id,
                        role: "assistant".to_string(),
                        content: text,
                        created_at: chrono::Utc::now().to_rfc3339(),
                        duration_ms: Some(duration_ms),
                        exit_code,
                        error: None,
                    });
                }
                Err(error) => {
                    stored.error = Some(error.clone());
                    stored.messages.push(SessionMessage {
                        id: Uuid::new_v4().to_string(),
                        role: "system".to_string(),
                        content: format!("Request failed: {}", error),
                        created_at: chrono::Utc::now().to_rfc3339(),
                        duration_ms: Some(duration_ms),
                        exit_code: None,
                        error: Some(error),
                    });
                }
            }
        }
        let updated = stored.clone();
        core.save_session(&updated);
        core.emit_session(&updated);
        Ok(updated)
    }

    /// Requests cancellation of the CLI turn currently running in `session_id`.
    ///
    /// The watch sender is the same one passed to the process executor, so the
    /// existing process-tree termination path runs. The session stays busy until
    /// the turn's completion path observes the signal and clears it.
    pub async fn cancel_cli_session_turn(
        &self,
        session_id: String,
    ) -> Result<SessionRecord, String> {
        let core = self.core.lock().await;
        let session = core
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| "Session not found".to_string())?;
        if session.kind != SessionKind::Cli {
            return Err("Only CLI sessions can be cancelled.".to_string());
        }
        let handle = core
            .cli_turns
            .get(&session_id)
            .ok_or_else(|| "This session has no active CLI turn to cancel.".to_string())?;
        handle
            .cancel_tx
            .send(true)
            .map_err(|_| "This CLI turn has already finished.".to_string())?;
        Ok(session.clone())
    }

    pub async fn cancel_prompt_session_turn(
        &self,
        session_id: String,
    ) -> Result<SessionRecord, String> {
        let core = self.core.lock().await;
        let session = core
            .sessions
            .iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| "Session not found".to_string())?;
        if session.kind != SessionKind::Prompt {
            return Err("Only prompt sessions can be cancelled this way.".to_string());
        }
        let handle = core
            .prompt_turns
            .get(&session_id)
            .ok_or_else(|| "This session has no active prompt turn to cancel.".to_string())?;
        handle
            .cancel_tx
            .send(true)
            .map_err(|_| "This prompt turn has already finished.".to_string())?;
        Ok(session.clone())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// The most recent user message is used as the task objective. The full
/// conversation is still supplied separately to the planner.
fn latest_user_objective(session: &SessionRecord) -> Option<String> {
    session
        .messages
        .iter()
        .rev()
        .filter(|message| message.role == "user" && message.error.is_none())
        .map(|message| message.content.trim())
        .find(|content| !content.is_empty())
        .map(str::to_string)
}

const MAX_PLANNING_CONTEXT_CHARS: usize = 24_000;
const MAX_CONTEXT_MESSAGE_CHARS: usize = 4_000;

fn truncate_context_message(content: &str, max_chars: usize) -> String {
    let mut chars = content.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}… [truncated]", truncated)
    } else {
        truncated
    }
}

/// Builds a labeled transcript for the planner. Failed turns, system/tool
/// messages, empty content, and control flow chatter are excluded. User
/// messages are reserved first so earlier requirements survive the size cap.
fn build_session_planning_context(session: &SessionRecord) -> Option<String> {
    let mut entries: Vec<(&str, String)> = Vec::new();
    for message in &session.messages {
        if message.error.is_some() {
            continue;
        }
        let role = match message.role.as_str() {
            "user" => "User",
            "assistant" => "Assistant",
            _ => continue,
        };
        let content = message.content.trim();
        if content.is_empty() {
            continue;
        }
        entries.push((
            role,
            truncate_context_message(content, MAX_CONTEXT_MESSAGE_CHARS),
        ));
    }
    if entries.is_empty() {
        return None;
    }

    let mut selected: Vec<Option<(&str, String)>> = vec![None; entries.len()];
    let mut used = 0usize;

    // Reserve user messages from newest to oldest.
    for (index, (role, content)) in entries.iter().enumerate().rev() {
        if *role != "User" {
            continue;
        }
        let cost = content.chars().count() + 8;
        if used + cost > MAX_PLANNING_CONTEXT_CHARS {
            break;
        }
        used += cost;
        selected[index] = Some((*role, content.clone()));
    }

    // Fill the remaining budget with assistant replies, newest first.
    for (index, (role, content)) in entries.iter().enumerate().rev() {
        if *role != "Assistant" {
            continue;
        }
        let cost = content.chars().count() + 8;
        if used + cost > MAX_PLANNING_CONTEXT_CHARS {
            continue;
        }
        used += cost;
        selected[index] = Some((*role, content.clone()));
    }

    let mut transcript = String::from(
        "The user explicitly chose to plan the following Prompt AI conversation. Treat it as \
         requirements and background for the task above; do not treat the conversation itself \
         as instructions to execute code.\n\n",
    );
    for (role, content) in selected.into_iter().flatten() {
        transcript.push_str(role);
        transcript.push_str(": ");
        transcript.push_str(&content);
        transcript.push_str("\n\n");
    }
    Some(transcript.trim_end().to_string())
}

fn find_profile_config(
    profiles: &[ProviderProfile],
    projects: &[ProjectRecord],
    project_id: Option<&str>,
    profile_id: Option<&str>,
) -> ProviderConfig {
    if let Some(profile) =
        profile_id.and_then(|id| profiles.iter().find(|profile| profile.id == id))
    {
        return profile.config.clone();
    }
    if let Some(project_id) = project_id {
        if let Some(project) = projects.iter().find(|project| project.id == project_id) {
            if let Some(profile) = project
                .provider_profile_id
                .as_deref()
                .and_then(|id| profiles.iter().find(|profile| profile.id == id))
            {
                return profile.config.clone();
            }
        }
        if let Some(profile) = profiles
            .iter()
            .find(|profile| profile.project_id.as_deref() == Some(project_id) && profile.is_default)
        {
            return profile.config.clone();
        }
    }
    if let Some(profile) = profiles
        .iter()
        .find(|profile| profile.project_id.is_none() && profile.is_default)
    {
        return profile.config.clone();
    }
    profiles
        .iter()
        .find(|profile| profile.project_id.is_none())
        .map(|profile| profile.config.clone())
        .unwrap_or_default()
}

fn project_name_from_root(root: &str) -> String {
    Path::new(root)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| root.to_string())
}

fn load_sessions(dir: &Path) -> Vec<SessionRecord> {
    let mut sessions = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(mut session) = serde_json::from_str::<SessionRecord>(&content) {
                    session.busy = false;
                    if !session.id.is_empty() {
                        sessions.push(session);
                    }
                }
            }
        }
    }
    sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    sessions
}

fn count_active_slots(core: &WorkspaceCore) -> usize {
    core.runtimes
        .values()
        .filter(|runtime| match runtime.orchestrator.try_lock() {
            Ok(guard) => guard
                .current_task
                .as_ref()
                .map(|task| task.state.occupies_execution_slot())
                .unwrap_or(false),
            Err(_) => true,
        })
        .count()
}

fn normalize_profile_config(
    existing: &ProviderConfig,
    input: ProviderConfigInput,
) -> Result<ProviderConfig, String> {
    let scratch = std::env::temp_dir().join(format!("archify-profile-{}.json", Uuid::new_v4()));
    let mut orchestrator =
        Orchestrator::new_isolated(None, scratch.with_extension("history"), scratch.clone());
    orchestrator.config = existing.clone();
    let result = orchestrator
        .configure_providers(input)
        .map(|_| orchestrator.config.clone());
    let _ = std::fs::remove_file(&scratch);
    result
}

async fn run_prompt_stream_turn<F>(
    config: &ProviderConfig,
    session: &SessionRecord,
    content: &str,
    profile_label: Option<&str>,
    cancel_rx: &mut watch::Receiver<bool>,
    on_delta: F,
) -> Result<String, String>
where
    F: FnMut(String) + Send + 'static,
{
    let endpoint = config
        .cloud_endpoint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "This session's provider profile has no HTTP endpoint configured.".to_string()
        })?
        .to_string();
    let model = config
        .cloud_model
        .clone()
        .unwrap_or_else(|| "gpt-4o-mini".to_string());

    let mut messages: Vec<(String, String)> = vec![(
        "system".to_string(),
        "You are a helpful AI coding assistant working inside the user's project. \
         Answer clearly and concretely, and prefer short code examples when useful."
            .to_string(),
    )];
    for message in &session.messages {
        let role = match message.role.as_str() {
            "user" => "user",
            "assistant" => "assistant",
            _ => continue,
        };
        if !message.content.trim().is_empty() && message.error.is_none() {
            messages.push((role.to_string(), message.content.clone()));
        }
    }
    messages.push(("user".to_string(), content.to_string()));

    let client = CloudHttpPlannerReviewer::new(endpoint, model, config.api_key.clone());
    if !client.has_usable_credentials() {
        return Err(prompt_credential_error(profile_label));
    }

    let stream_messages = messages.clone();
    match client
        .chat_stream(stream_messages, 120, cancel_rx, on_delta)
        .await
    {
        Ok(text) => Ok(text),
        Err(err) => {
            if err.to_lowercase().contains("cancel") {
                return Err(err);
            }
            // Only fall back to the complete-response path when streaming
            // produced no usable output (e.g. provider does not support SSE).
            // Mid-stream failures are surfaced as errors to preserve partial
            // output handling.
            if err.contains("empty assistant response") {
                client.chat(messages).await
            } else {
                Err(err)
            }
        }
    }
}

fn prompt_credential_error(profile_label: Option<&str>) -> String {
    let target = match profile_label {
        Some(name) => format!("provider profile '{}'", name),
        None => "the default provider profile".to_string(),
    };
    format!(
        "Prompt AI cannot authenticate with {} because no API key is available. \
         Add a key in Provider Profiles or set \
         PLANNER_API_KEY / OPENAI_API_KEY / GEMINI_API_KEY. Your conversation is preserved; \
         resend the message after updating the key.",
        target
    )
}

async fn run_cli_turn(
    session: &SessionRecord,
    content: &str,
    cancel_rx: &mut watch::Receiver<bool>,
) -> Result<(String, Option<i32>), String> {
    let root = session
        .project_root
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "CLI sessions need a project folder to run in.".to_string())?
        .to_string();
    let executable = session
        .cli_executable
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "This session has no CLI executable configured.".to_string())?
        .to_string();

    let mut prompt = String::new();
    if !session.messages.is_empty() {
        prompt.push_str("Conversation so far:\n");
        for message in &session.messages {
            let label = match message.role.as_str() {
                "user" => "User",
                "assistant" => "Assistant",
                "system" => "System",
                _ => continue,
            };
            prompt.push_str(label);
            prompt.push_str(": ");
            prompt.push_str(&message.content);
            prompt.push_str("\n\n");
        }
        prompt.push_str("Continue the conversation.\n\nUser: ");
    }
    prompt.push_str(content);

    let executor = ProcessCliExecutor::with_prompt_delivery(
        executable,
        session.cli_args.clone(),
        session.prompt_delivery,
        None,
        None,
    );
    let result = executor.execute(&prompt, &root, None, cancel_rx).await?;

    let mut text = result.stdout.clone();
    if !result.stderr.trim().is_empty() {
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        text.push_str("[stderr]\n");
        text.push_str(&result.stderr);
    }
    if text.trim().is_empty() {
        text = "(The CLI agent returned no output.)".to_string();
    }
    Ok((text, result.exit_code))
}

#[cfg(test)]
mod credential_tests {
    use super::*;
    use crate::credentials::{CredentialStore, CredentialStoreError};
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct MemoryCredentialStore {
        values: StdMutex<HashMap<String, String>>,
        fail_store: AtomicBool,
        fail_retrieve: AtomicBool,
        fail_delete: AtomicBool,
    }

    impl Default for MemoryCredentialStore {
        fn default() -> Self {
            Self {
                values: StdMutex::new(HashMap::new()),
                fail_store: AtomicBool::new(false),
                fail_retrieve: AtomicBool::new(false),
                fail_delete: AtomicBool::new(false),
            }
        }
    }

    impl CredentialStore for MemoryCredentialStore {
        fn store(&self, profile_id: &str, secret: &str) -> Result<(), CredentialStoreError> {
            if self.fail_store.load(Ordering::SeqCst) {
                return Err(CredentialStoreError::OperationFailed);
            }
            self.values
                .lock()
                .unwrap()
                .insert(profile_id.to_string(), secret.to_string());
            Ok(())
        }

        fn retrieve(&self, profile_id: &str) -> Result<Option<String>, CredentialStoreError> {
            if self.fail_retrieve.load(Ordering::SeqCst) {
                return Err(CredentialStoreError::OperationFailed);
            }
            Ok(self.values.lock().unwrap().get(profile_id).cloned())
        }

        fn delete(&self, profile_id: &str) -> Result<(), CredentialStoreError> {
            if self.fail_delete.load(Ordering::SeqCst) {
                return Err(CredentialStoreError::OperationFailed);
            }
            self.values.lock().unwrap().remove(profile_id);
            Ok(())
        }
    }

    #[tokio::test]
    async fn provider_credentials_are_profile_scoped_and_survive_workspace_reload() {
        let dir = std::env::temp_dir().join(format!("workspace_credentials_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Arc::new(MemoryCredentialStore::default());
        let first = Workspace::new_with_credential_store(None, dir.clone(), store.clone());
        let profile_a = first
            .create_provider_profile(
                "Provider A".to_string(),
                None,
                false,
                ProviderConfigInput {
                    api_key: Some("secret-for-a".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let profile_b = first
            .create_provider_profile(
                "Provider B".to_string(),
                None,
                false,
                ProviderConfigInput::default(),
            )
            .await
            .unwrap();

        assert_eq!(
            store.retrieve(&profile_a.id).unwrap().as_deref(),
            Some("secret-for-a")
        );
        assert_eq!(store.retrieve(&profile_b.id).unwrap(), None);
        let remote_config = ProviderConfig {
            planner_type: "cloud_http".to_string(),
            cloud_endpoint: Some("https://provider.example/v1".to_string()),
            ..Default::default()
        };
        assert_eq!(
            first
                .retrieve_profile_credential(Some(&profile_a.id), &remote_config)
                .await
                .unwrap()
                .as_deref(),
            Some("secret-for-a")
        );
        assert_eq!(
            first
                .retrieve_profile_credential(Some(&profile_b.id), &remote_config)
                .await
                .unwrap(),
            None
        );
        store.fail_retrieve.store(true, Ordering::SeqCst);
        let error = first
            .retrieve_profile_credential(Some(&profile_a.id), &remote_config)
            .await
            .unwrap_err();
        assert!(!error.contains("secret-for-a"));
        store.fail_retrieve.store(false, Ordering::SeqCst);
        assert!(
            serde_json::to_string(&profile_a)
                .unwrap()
                .find("secret-for-a")
                .is_none()
        );
        assert!(
            serde_json::to_string(&first.snapshot().await)
                .unwrap()
                .find("secret-for-a")
                .is_none()
        );

        let reloaded = Workspace::new_with_credential_store(None, dir.clone(), store.clone());
        assert!(
            reloaded
                .provider_status_for(Some(profile_a.id.clone()))
                .await
                .unwrap()
                .has_api_key
        );
        let environment_key_available = ["PLANNER_API_KEY", "OPENAI_API_KEY", "GEMINI_API_KEY"]
            .iter()
            .any(|name| {
                std::env::var(name)
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false)
            });
        assert_eq!(
            reloaded
                .provider_status_for(Some(profile_b.id.clone()))
                .await
                .unwrap()
                .has_api_key,
            environment_key_available
        );
        reloaded
            .delete_provider_profile(profile_a.id.clone())
            .await
            .unwrap();
        assert_eq!(store.retrieve(&profile_a.id).unwrap(), None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn replacing_or_deleting_credentials_preserves_profile_on_storage_failure() {
        let dir =
            std::env::temp_dir().join(format!("workspace_credential_update_{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Arc::new(MemoryCredentialStore::default());
        let workspace = Workspace::new_with_credential_store(None, dir.clone(), store.clone());
        let profile = workspace
            .create_provider_profile(
                "Provider".to_string(),
                None,
                false,
                ProviderConfigInput {
                    api_key: Some("original-secret".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        store.fail_store.store(true, Ordering::SeqCst);
        let error = workspace
            .update_provider_profile(
                profile.id.clone(),
                Some("Renamed".to_string()),
                None,
                ProviderConfigInput {
                    api_key: Some("replacement-secret".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert!(!error.contains("replacement-secret"));
        assert_eq!(
            store.retrieve(&profile.id).unwrap().as_deref(),
            Some("original-secret")
        );
        assert_eq!(
            workspace
                .snapshot()
                .await
                .provider_profiles
                .iter()
                .find(|candidate| candidate.id == profile.id)
                .unwrap()
                .name,
            "Provider"
        );

        store.fail_store.store(false, Ordering::SeqCst);
        workspace
            .update_provider_profile(
                profile.id.clone(),
                None,
                None,
                ProviderConfigInput {
                    api_key: Some("replacement-secret".to_string()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            store.retrieve(&profile.id).unwrap().as_deref(),
            Some("replacement-secret")
        );

        store.fail_delete.store(true, Ordering::SeqCst);
        let error = workspace
            .delete_provider_profile(profile.id.clone())
            .await
            .unwrap_err();
        assert!(!error.contains("replacement-secret"));
        assert!(
            workspace
                .snapshot()
                .await
                .provider_profiles
                .iter()
                .any(|candidate| candidate.id == profile.id)
        );
        assert_eq!(
            store.retrieve(&profile.id).unwrap().as_deref(),
            Some("replacement-secret")
        );

        store.fail_delete.store(false, Ordering::SeqCst);
        workspace
            .delete_provider_credential(profile.id.clone())
            .await
            .unwrap();
        assert_eq!(store.retrieve(&profile.id).unwrap(), None);
        assert!(
            workspace
                .snapshot()
                .await
                .provider_profiles
                .iter()
                .any(|candidate| candidate.id == profile.id)
        );
        workspace
            .delete_provider_profile(profile.id.clone())
            .await
            .unwrap();
        assert!(store.retrieve(&profile.id).unwrap().is_none());
        assert!(
            !workspace
                .snapshot()
                .await
                .provider_profiles
                .iter()
                .any(|candidate| candidate.id == profile.id)
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn provider_configuration_debug_output_redacts_secret() {
        let config = ProviderConfig {
            api_key: Some("never-print-this-secret".to_string()),
            ..Default::default()
        };
        let debug = format!("{config:?}");
        assert!(!debug.contains("never-print-this-secret"));
        assert!(debug.contains("[REDACTED]"));
    }
}

// ---------------------------------------------------------------------------
// Configuration precedence tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod config_precedence_tests {
    use super::*;

    fn make_core() -> WorkspaceCore {
        WorkspaceCore {
            active_project_id: None,
            projects: Vec::new(),
            provider_profiles: Vec::new(),
            settings: WorkspaceSettings::default(),
            sessions: Vec::new(),
            runtimes: HashMap::new(),
            handoffs_in_progress: HashSet::new(),
            cli_turns: HashMap::new(),
            prompt_turns: HashMap::new(),
            history_dir: std::env::temp_dir().join("test_history"),
            sessions_dir: std::env::temp_dir().join("test_sessions"),
            workspace_path: std::env::temp_dir().join("test_workspace.json"),
            config_path: std::env::temp_dir().join("test_config.json"),
            app_handle: None,
            credential_store: Arc::new(crate::credentials::UnavailableCredentialStore),
            model_selector: crate::model_selection::ModelSelector::new(),
            token_usage_tracker: Arc::new(StdMutex::new(
                crate::token_usage::TokenUsageTracker::new(
                    std::env::temp_dir().join("test_token_usage.json"),
                ),
            )),
            monitoring_hub: MonitoringHub::headless(),
            reported_task_results: HashSet::new(),
            notification_path: std::env::temp_dir().join("test_notifications.json"),
            notifications: Vec::new(),
            notification_dedup: HashSet::new(),
            activity_path: std::env::temp_dir().join("test_activity.json"),
            activity: Vec::new(),
            activity_dedup: HashSet::new(),
            last_task_states: HashMap::new(),
            last_monitoring_sequence: 0,
        }
    }

    #[test]
    fn task_overrides_beat_project_settings() {
        let mut core = make_core();

        // Add a project with a profile that sets cli_executable to "old-agent"
        let project_id = Uuid::new_v4().to_string();
        let profile_id = Uuid::new_v4().to_string();
        core.projects.push(ProjectRecord {
            id: project_id.clone(),
            name: "test-project".to_string(),
            root: std::env::temp_dir().to_string_lossy().to_string(),
            provider_profile_id: Some(profile_id.clone()),
            created_at: chrono::Utc::now().to_rfc3339(),
            last_opened_at: chrono::Utc::now().to_rfc3339(),
            pinned: false,
            archived: false,
            description: None,
            tags: Vec::new(),
        });
        core.provider_profiles.push(ProviderProfile {
            id: profile_id.clone(),
            name: "project-profile".to_string(),
            project_id: Some(project_id.clone()),
            is_default: true,
            config: ProviderConfig {
                cli_executable: Some("old-agent".to_string()),
                cli_args: Some(vec!["--old".to_string()]),
                ..ProviderConfig::default()
            },
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        });

        // Task-level override: cli_executable = "new-agent"
        let overrides = TaskConfigOverrides {
            cli_executable: Some("new-agent".to_string()),
            ..Default::default()
        };
        let (config, resolved_profile_id) =
            core.resolve_config_with_precedence(Some(&project_id), Some(&profile_id), &overrides);

        assert_eq!(resolved_profile_id, Some(profile_id));
        assert_eq!(
            config.cli_executable.as_deref(),
            Some("new-agent"),
            "task-level override should win over project setting"
        );
        // cli_args from project should still apply (not overridden by task)
        assert_eq!(
            config.cli_args,
            Some(vec!["--old".to_string()]),
            "project-level cli_args should apply when task doesn't override"
        );
    }

    #[test]
    fn project_settings_beat_global_defaults() {
        let mut core = make_core();

        // Global default profile (no project_id)
        let global_id = Uuid::new_v4().to_string();
        core.provider_profiles.push(ProviderProfile {
            id: global_id.clone(),
            name: "global".to_string(),
            project_id: None,
            is_default: true,
            config: ProviderConfig {
                cli_executable: Some("global-agent".to_string()),
                max_iterations: 5,
                ..ProviderConfig::default()
            },
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        });

        // Project-scoped profile overrides max_iterations
        let project_id = Uuid::new_v4().to_string();
        let profile_id = Uuid::new_v4().to_string();
        core.projects.push(ProjectRecord {
            id: project_id.clone(),
            name: "proj".to_string(),
            root: std::env::temp_dir().to_string_lossy().to_string(),
            provider_profile_id: Some(profile_id.clone()),
            created_at: chrono::Utc::now().to_rfc3339(),
            last_opened_at: chrono::Utc::now().to_rfc3339(),
            pinned: false,
            archived: false,
            description: None,
            tags: Vec::new(),
        });
        core.provider_profiles.push(ProviderProfile {
            id: profile_id.clone(),
            name: "project".to_string(),
            project_id: Some(project_id.clone()),
            is_default: true,
            config: ProviderConfig {
                cli_executable: Some("project-agent".to_string()),
                max_iterations: 7,
                ..ProviderConfig::default()
            },
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        });

        let (config, _) = core.resolve_config_with_precedence(
            Some(&project_id),
            Some(&profile_id),
            &TaskConfigOverrides::default(),
        );

        assert_eq!(
            config.cli_executable.as_deref(),
            Some("project-agent"),
            "project setting should beat global default"
        );
        assert_eq!(
            config.max_iterations, 7,
            "project max_iterations should beat global"
        );
    }

    #[test]
    fn global_default_beats_application_defaults() {
        let mut core = make_core();

        // Application default: max_iterations = 3, executor_type = "test"
        // Global default overrides max_iterations
        let global_id = Uuid::new_v4().to_string();
        core.provider_profiles.push(ProviderProfile {
            id: global_id.clone(),
            name: "global".to_string(),
            project_id: None,
            is_default: true,
            config: ProviderConfig {
                max_iterations: 8,
                ..ProviderConfig::default()
            },
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        });

        let (config, _) =
            core.resolve_config_with_precedence(None, None, &TaskConfigOverrides::default());

        assert_eq!(
            config.max_iterations, 8,
            "global default should beat application default (3)"
        );
    }

    #[test]
    fn application_defaults_when_no_profiles() {
        let core = make_core();

        let (config, profile_id) =
            core.resolve_config_with_precedence(None, None, &TaskConfigOverrides::default());

        assert_eq!(profile_id, None);
        assert_eq!(
            config,
            ProviderConfig::default(),
            "no profiles means application defaults"
        );
        assert_eq!(config.max_iterations, 3);
        assert_eq!(config.executor_type, "test");
    }

    #[test]
    fn task_override_cli_args_replaces_project_args() {
        let mut core = make_core();

        let profile_id = Uuid::new_v4().to_string();
        core.provider_profiles.push(ProviderProfile {
            id: profile_id.clone(),
            name: "profile".to_string(),
            project_id: None,
            is_default: true,
            config: ProviderConfig {
                cli_args: Some(vec!["--project-arg".to_string()]),
                ..ProviderConfig::default()
            },
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        });

        let overrides = TaskConfigOverrides {
            cli_args: Some(vec!["--task-arg".to_string()]),
            ..Default::default()
        };
        let (config, _) = core.resolve_config_with_precedence(None, Some(&profile_id), &overrides);

        assert_eq!(
            config.cli_args,
            Some(vec!["--task-arg".to_string()]),
            "task cli_args should replace project cli_args"
        );
    }

    #[test]
    fn empty_task_cli_args_clears_project_args() {
        let mut core = make_core();

        let profile_id = Uuid::new_v4().to_string();
        core.provider_profiles.push(ProviderProfile {
            id: profile_id.clone(),
            name: "profile".to_string(),
            project_id: None,
            is_default: true,
            config: ProviderConfig {
                cli_args: Some(vec!["--project-arg".to_string()]),
                ..ProviderConfig::default()
            },
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        });

        let overrides = TaskConfigOverrides {
            cli_args: Some(Vec::new()),
            ..Default::default()
        };
        let (config, _) = core.resolve_config_with_precedence(None, Some(&profile_id), &overrides);

        assert_eq!(
            config.cli_args, None,
            "empty task cli_args should clear project args"
        );
    }
}

// ---------------------------------------------------------------------------
// Conversation / session management tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod conversation_tests {
    use super::*;
    use crate::models::SessionKind;

    fn make_workspace() -> Workspace {
        let dir = std::env::temp_dir().join(format!("archify_conv_test_{}", Uuid::new_v4()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Workspace::new_with_paths(None, dir)
    }

    async fn create_test_session(workspace: &Workspace, title: &str) -> SessionRecord {
        workspace
            .create_session(CreateSessionOptions {
                kind: Some(SessionKind::Cli),
                title: Some(title.to_string()),
                cli_executable: Some("cmd.exe".to_string()),
                ..Default::default()
            })
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn session_archive_and_restore_cycle() {
        let workspace = make_workspace();
        let session = create_test_session(&workspace, "My Conversation").await;
        assert!(!session.archived);

        // Archive
        let archived = workspace
            .archive_session(session.id.clone(), true)
            .await
            .unwrap();
        assert!(archived.archived);

        // Archived sessions excluded from default listing
        let listed = workspace.list_sessions(None, None, None).await.unwrap();
        assert!(listed.iter().all(|s| s.id != session.id));

        // Archived sessions included when explicitly requested
        let all = workspace
            .list_sessions_including_archived(None, None, None)
            .await
            .unwrap();
        assert!(all.iter().any(|s| s.id == session.id && s.archived));

        // Restore
        let restored = workspace
            .archive_session(session.id.clone(), false)
            .await
            .unwrap();
        assert!(!restored.archived);

        let listed_again = workspace.list_sessions(None, None, None).await.unwrap();
        assert!(
            listed_again
                .iter()
                .any(|s| s.id == session.id && !s.archived)
        );
    }

    #[tokio::test]
    async fn session_update_rename_and_project_change() {
        let workspace = make_workspace();

        // Create a project to associate with
        let project = workspace
            .add_project(
                std::env::temp_dir().to_string_lossy().to_string(),
                Some("test project".to_string()),
            )
            .await
            .unwrap();

        let session = create_test_session(&workspace, "Original Title").await;
        assert_eq!(session.title, "Original Title");
        assert_eq!(session.project_id, None);

        // Rename
        let updated = workspace
            .update_session(session.id.clone(), Some("Renamed Title".to_string()), None)
            .await
            .unwrap();
        assert_eq!(updated.title, "Renamed Title");
        assert_eq!(updated.project_id, None);

        // Associate with project
        let updated = workspace
            .update_session(session.id.clone(), None, Some(project.id.clone()))
            .await
            .unwrap();
        assert_eq!(updated.project_id, Some(project.id.clone()));
        assert_eq!(updated.project_root, Some(project.root.clone()));

        // Clear project association
        let updated = workspace
            .update_session(session.id.clone(), None, Some(String::new()))
            .await
            .unwrap();
        assert_eq!(updated.project_id, None);
        assert_eq!(updated.project_root, None);
    }

    #[tokio::test]
    async fn session_update_rejects_empty_title() {
        let workspace = make_workspace();
        let session = create_test_session(&workspace, "Title").await;

        let err = workspace
            .update_session(session.id.clone(), Some("   ".to_string()), None)
            .await
            .unwrap_err();
        assert!(err.contains("empty"));
    }

    #[tokio::test]
    async fn session_update_rejects_unknown_project() {
        let workspace = make_workspace();
        let session = create_test_session(&workspace, "Title").await;

        let err = workspace
            .update_session(
                session.id.clone(),
                None,
                Some("non-existent-project-id".to_string()),
            )
            .await
            .unwrap_err();
        assert!(err.contains("Project not found"));
    }

    #[tokio::test]
    async fn archived_session_cannot_receive_messages() {
        let workspace = make_workspace();
        let session = create_test_session(&workspace, "To Archive").await;

        workspace
            .archive_session(session.id.clone(), true)
            .await
            .unwrap();

        // Sending a message to an archived session should fail.
        let err = workspace
            .send_session_message(session.id.clone(), "hello".to_string())
            .await
            .unwrap_err();
        assert!(err.contains("archived"));

        // Restoring allows messages again (CLI session needs a project, so
        // it will fail on that, but not on "archived").
        workspace
            .archive_session(session.id.clone(), false)
            .await
            .unwrap();
        let result = workspace
            .send_session_message(session.id.clone(), "hello".to_string())
            .await;
        match result {
            Err(err) => {
                assert!(!err.contains("archived"));
            }
            Ok(session) => {
                // The message was sent; the CLI turn may have failed internally
                // but the send itself succeeded.
                assert!(!session.archived);
            }
        }

        let fetched = workspace.get_session(session.id.clone()).await.unwrap();
        assert!(!fetched.archived);
    }

    #[tokio::test]
    async fn session_list_filters_by_project() {
        let workspace = make_workspace();

        let _ = std::fs::create_dir_all(std::env::temp_dir().join("sub"));

        let p1 = workspace
            .add_project(
                std::env::temp_dir().to_string_lossy().to_string(),
                Some("P1".to_string()),
            )
            .await
            .unwrap();
        let p2 = workspace
            .add_project(
                std::env::temp_dir()
                    .join("sub")
                    .to_string_lossy()
                    .to_string(),
                Some("P2".to_string()),
            )
            .await
            .unwrap();

        let s1 = create_test_session(&workspace, "S1").await;
        // Associate with project
        workspace
            .update_session(s1.id.clone(), None, Some(p1.id.clone()))
            .await
            .unwrap();
        let s2 = create_test_session(&workspace, "S2").await;
        workspace
            .update_session(s2.id.clone(), None, Some(p2.id.clone()))
            .await
            .unwrap();

        let p1_sessions = workspace
            .list_sessions(Some(p1.id.clone()), None, None)
            .await
            .unwrap();
        assert_eq!(p1_sessions.len(), 1);
        assert_eq!(p1_sessions[0].id, s1.id);

        let p2_sessions = workspace
            .list_sessions(Some(p2.id.clone()), None, None)
            .await
            .unwrap();
        assert_eq!(p2_sessions.len(), 1);
        assert_eq!(p2_sessions[0].id, s2.id);

        let all = workspace.list_sessions(None, None, None).await.unwrap();
        assert!(all.iter().any(|s| s.id == s1.id));
        assert!(all.iter().any(|s| s.id == s2.id));
    }
}

// ---------------------------------------------------------------------------
// ARCHIFY.md discovery tests (instructions module)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod archify_md_discovery_tests {
    use super::*;
    use crate::instructions::{
        InstructionFormat, InstructionScope, InstructionStoreFile,
        discover_project_instruction_files, resolve_instructions,
    };
    use std::fs;

    fn temp_dir(label: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let d = std::env::temp_dir().join(format!(
            "archify_archifymd_{}_{}_{}",
            label,
            std::process::id(),
            nanos
        ));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn archify_md_is_discovered_as_project_instruction() {
        let dir = temp_dir("archify_md");
        fs::write(dir.join("ARCHIFY.md"), "# Archify project guidance").unwrap();

        let found = discover_project_instruction_files(&dir.to_string_lossy());
        assert!(
            found.iter().any(|r| r.title == "ARCHIFY.md" && !r.managed),
            "ARCHIFY.md should be discovered"
        );
        let record = found.iter().find(|r| r.title == "ARCHIFY.md").unwrap();
        assert_eq!(record.scope, InstructionScope::Project);
        assert_eq!(record.format, InstructionFormat::DiscoveredFile);
        assert!(!record.managed);
        assert!(record.source_path.is_some());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn archify_md_included_in_resolved_instructions() {
        let dir = temp_dir("archify_md_resolve");
        fs::write(dir.join("ARCHIFY.md"), "# Always follow Archify rules").unwrap();

        let found = discover_project_instruction_files(&dir.to_string_lossy());
        let store = InstructionStoreFile {
            instructions: found,
            ..Default::default()
        };
        let block = resolve_instructions(
            &store,
            Some(&dir.to_string_lossy()),
            None,
            None,
            Some("planner"),
        );
        assert!(
            block.text.contains("Always follow Archify rules"),
            "ARCHIFY.md content should be in resolved instructions"
        );
        assert!(
            block.refs.iter().any(|r| r.title == "ARCHIFY.md"),
            "ARCHIFY.md should be in applied refs"
        );

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn archify_md_missing_file_is_not_an_error() {
        let dir = temp_dir("no_archify_md");
        fs::write(dir.join("CLAUDE.md"), "# Claude only").unwrap();

        let found = discover_project_instruction_files(&dir.to_string_lossy());
        assert!(
            !found.iter().any(|r| r.title == "ARCHIFY.md"),
            "ARCHIFY.md should not appear when missing"
        );
        assert!(
            found.iter().any(|r| r.title == "CLAUDE.md"),
            "CLAUDE.md should still be discovered"
        );

        let _ = fs::remove_dir_all(dir);
    }
}
