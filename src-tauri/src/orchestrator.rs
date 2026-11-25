use crate::agent_lifecycle::{
    AgentLifecycleTracker, LifecycleEvent, LifecycleEventType, TrackerConfig,
};
use crate::constants::{PROVIDER_CONFIG_KEYS, SECRET_ENV_VARS};
use crate::model_selection::{ModelSelection, ModelSelector};
use crate::models::{
    ChangedFiles, ComponentState, ExecutionEvent, ExecutorResult, IterationRecord,
    PersistedBaseline, PersistedFileSnapshot, Plan, ProviderConfig, ProviderConfigInput,
    ProviderStatus, TaskRecord, TaskState, VerificationStatus,
};
use crate::monitoring::{EmitOutcome, EventContext, MonitoringHub};
use crate::providers::cloud_http::CloudHttpPlannerReviewer;
use crate::providers::process_cli::ProcessCliExecutor;
use crate::providers::test_adapter::{
    TestExecutor, TestExecutorScenario, TestPlannerReviewer, TestPlannerScenario,
};
use crate::providers::{
    CodingExecutor, PlannerReviewer, is_local_endpoint_url, resolve_executable,
};
use crate::skills::AppliedSkillBlock;
use crate::token_usage::{TokenUsage, TokenUsageTracker, UsageContext};
use crate::verification::{
    FileSnapshot, diff_snapshots, run_verification_command, snapshot_project,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::sync::atomic::AtomicU32;
use tauri::{AppHandle, Emitter};
use tokio::sync::watch;
use uuid::Uuid;

pub struct Orchestrator {
    pub current_task: Option<TaskRecord>,
    pub task_history: HashMap<String, TaskRecord>,
    pub config: ProviderConfig,
    pub cancel_tx: Option<watch::Sender<bool>>,
    pub app_handle: Option<AppHandle>,
    pub history_dir: PathBuf,
    /// Where the non-secret provider settings are persisted between runs.
    pub config_path: PathBuf,
    pub test_planner_call_count: Arc<AtomicU32>,
    /// Last resolved model ID for planning (task-level override)
    pub task_model_id: Option<String>,
    /// Model preferences for resolution
    pub model_preferences: Option<crate::model_selection::ModelPreferences>,
    /// Pre-execution filesystem baselines captured before the first coding-agent
    /// execution of each active task. Keyed by task_id. In-memory only; lost on
    /// restart, in which case attribution gracefully degrades to non-baseline
    /// change detection.
    pub task_baselines: HashMap<String, HashMap<String, FileSnapshot>>,
    /// Shared aggregate token-usage tracker. Set by the workspace so CLI
    /// execution results can be recorded alongside provider-reported usage.
    pub token_usage_tracker: Option<Arc<StdMutex<TokenUsageTracker>>>,
    /// Shared monitoring hub for emitting lifecycle and execution events.
    /// Set by the workspace so all tasks share one event stream with
    /// deduplication and history. None when running headless without a hub.
    pub monitoring_hub: Option<MonitoringHub>,
}

impl Orchestrator {
    pub fn new(app_handle: Option<AppHandle>) -> Self {
        let (history_dir, config_path) = if let Some(ref handle) = app_handle {
            use tauri::Manager;
            let base = handle
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| PathBuf::from("."));
            (base.join("task_history"), base.join("provider_config.json"))
        } else {
            // Headless construction (used by tests and tooling). Use a unique
            // scratch directory so unit tests can never read or mutate a real
            // developer/app task history or provider config.
            let base = std::env::temp_dir()
                .join("archify-scratch")
                .join(Uuid::new_v4().to_string());
            (base.join("task_history"), base.join("provider_config.json"))
        };

        let mut orch = Self {
            current_task: None,
            task_history: HashMap::new(),
            config: ProviderConfig::default(),
            cancel_tx: None,
            app_handle,
            history_dir,
            config_path,
            test_planner_call_count: Arc::new(AtomicU32::new(0)),
            task_model_id: None,
            model_preferences: None,
            task_baselines: HashMap::new(),
            token_usage_tracker: None,
            monitoring_hub: None,
        };

        orch.load_history_from_disk();
        orch.recover_interrupted_tasks();
        orch.load_provider_config();
        orch
    }

    /// Builds an orchestrator with explicit storage locations.
    ///
    /// Useful for tests and for callers that need to inspect a specific data
    /// directory while still going through the normal restore/recovery path.
    pub fn new_with_paths(
        app_handle: Option<AppHandle>,
        history_dir: PathBuf,
        config_path: PathBuf,
    ) -> Self {
        let mut orch = Self {
            current_task: None,
            task_history: HashMap::new(),
            config: ProviderConfig::default(),
            cancel_tx: None,
            app_handle,
            history_dir,
            config_path,
            test_planner_call_count: Arc::new(AtomicU32::new(0)),
            task_model_id: None,
            model_preferences: None,
            task_baselines: HashMap::new(),
            token_usage_tracker: None,
            monitoring_hub: None,
        };

        orch.load_history_from_disk();
        orch.recover_interrupted_tasks();
        orch.load_provider_config();
        orch
    }

    /// Builds an orchestrator that owns exactly one task and does not touch the
    /// shared history or run crash recovery. The workspace creates one of these
    /// per active task so several tasks can run concurrently.
    pub fn new_isolated(
        app_handle: Option<AppHandle>,
        history_dir: PathBuf,
        config_path: PathBuf,
    ) -> Self {
        Self {
            current_task: None,
            task_history: HashMap::new(),
            config: ProviderConfig::default(),
            cancel_tx: None,
            app_handle,
            history_dir,
            config_path,
            test_planner_call_count: Arc::new(AtomicU32::new(0)),
            task_model_id: None,
            model_preferences: None,
            task_baselines: HashMap::new(),
            token_usage_tracker: None,
            monitoring_hub: None,
        }
    }

    /// Build an orchestrator that owns an existing task record (used by the
    /// workspace to re-attach queued or awaiting-approval tasks after restart).
    pub fn with_task(
        app_handle: Option<AppHandle>,
        history_dir: PathBuf,
        config_path: PathBuf,
        config: ProviderConfig,
        record: TaskRecord,
    ) -> Self {
        let mut record = record;
        let mut orch = Self::new_isolated(app_handle, history_dir, config_path);
        // Prefer the task's persisted execution configuration snapshot over the
        // freshly resolved profile config, so retries/recovery keep their
        // original settings. Fall back to the supplied config for legacy tasks.
        if let Some(snapshot) = record.execution_config.clone() {
            orch.config = snapshot;
        } else {
            orch.config = config.clone();
            // Legacy fallback: record the resolved (non-secret) config as the
            // snapshot so future reloads use it instead of re-resolving the
            // latest profile. api_key is never persisted (serde skips it).
            record.execution_config = Some(orch.config.clone());
            orch.emit_log(&format!(
                "Task {} has no persisted execution config; using resolved profile config (legacy fallback).",
                record.id
            ));
        }
        // Restore the task-level model override from the persisted record so
        // retries and restarts use the same model selection without re-resolution.
        if let Some(model_id) = record.task_model_id.clone() {
            if let Some(task) = orch.current_task.as_mut() {
                if task.id == record.id {
                    task.task_model_id = Some(model_id.clone());
                }
            }
            orch.task_model_id = Some(model_id);
        }
        // Restore the in-memory baseline for resumed mid-flight tasks.
        let resumable = matches!(
            record.state,
            TaskState::Executing
                | TaskState::Verifying
                | TaskState::Reviewing
                | TaskState::Retrying
        );
        if resumable {
            if let Some(b) = &record.task_baseline {
                orch.task_baselines
                    .insert(record.id.clone(), baseline_from_persisted(b));
            }
        }
        orch.current_task = Some(record.clone());
        orch.task_history.insert(record.id.clone(), record);
        orch
    }

    /// Load non-secret provider settings from `config_path`.
    ///
    /// Never reads or writes an API key: profile credentials are managed by the
    /// workspace credential store, and the legacy JSON config excludes the field.
    pub(crate) fn load_provider_config(&mut self) {
        if self.config_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&self.config_path) {
                if let Ok(cfg) = serde_json::from_str::<ProviderConfig>(&content) {
                    // Retain validated fields. `api_key` is never persisted in JSON;
                    // the workspace loads profile credentials from OS storage when needed.
                    // Reuse the same endpoint validation the configure path applies, so a
                    // hand-edited file cannot smuggle in an unusable or unsafe URL.
                    self.config.cloud_endpoint = match cfg.cloud_endpoint.as_deref() {
                        Some(raw) if !raw.trim().is_empty() => {
                            match Self::normalize_cloud_endpoint(raw.trim()) {
                                Some(valid) => {
                                    let base = valid
                                        .trim_end_matches('/')
                                        .trim_end_matches("/chat/completions");
                                    Some(base.to_string())
                                }
                                None => {
                                    let _ = self.emit_log(
                                        "provider_config: invalid cloud_endpoint; using default",
                                    );
                                    None
                                }
                            }
                        }
                        _ => None,
                    };
                    if cfg.max_iterations >= 1 && cfg.max_iterations <= 10 {
                        self.config.max_iterations = cfg.max_iterations;
                    } else {
                        // Out of range: report and fall back to default rather than trusting input.
                        let _ = self.emit_log(&format!(
                            "provider_config: max_iterations={} out of [1,10]; using default",
                            cfg.max_iterations
                        ));
                    }

                    // A hand-edited file must be held to the same contract as the UI:
                    // unknown provider types fall back to the safe test adapters.
                    self.config.planner_type = match cfg.planner_type.trim() {
                        "test" => "test".to_string(),
                        "cloud_http" => "cloud_http".to_string(),
                        other => {
                            let _ = self.emit_log(&format!(
                                "provider_config: invalid planner_type '{}'; using test",
                                other
                            ));
                            "test".to_string()
                        }
                    };
                    self.config.executor_type = match cfg.executor_type.trim() {
                        "test" => "test".to_string(),
                        "process_cli" => "process_cli".to_string(),
                        other => {
                            let _ = self.emit_log(&format!(
                                "provider_config: invalid executor_type '{}'; using test",
                                other
                            ));
                            "test".to_string()
                        }
                    };

                    self.config.test_planner_scenario = cfg
                        .test_planner_scenario
                        .filter(|value| !contains_control_chars(value));
                    self.config.test_executor_scenario = cfg
                        .test_executor_scenario
                        .filter(|value| !contains_control_chars(value));

                    self.config.cloud_model = cfg
                        .cloud_model
                        .as_deref()
                        .and_then(|value| sanitize_optional_text("cloud_model", value).ok())
                        .flatten();

                    self.config.cli_executable = cfg
                        .cli_executable
                        .as_deref()
                        .and_then(|value| sanitize_optional_text("cli_executable", value).ok())
                        .flatten();

                    self.config.cli_args = cfg
                        .cli_args
                        .map(|args| {
                            // Salvage valid arguments from a hand-edited file instead of
                            // discarding the whole list; the configure path stays strict.
                            args.into_iter()
                                .map(|arg| arg.trim().to_string())
                                .filter(|arg| !arg.is_empty() && !contains_control_chars(arg))
                                .take(64)
                                .collect::<Vec<_>>()
                        })
                        .filter(|args| !args.is_empty());

                    self.config.cli_preset = cfg
                        .cli_preset
                        .as_deref()
                        .and_then(|value| sanitize_optional_text("cli_preset", value).ok())
                        .flatten();
                    self.config.prompt_delivery = cfg.prompt_delivery;

                    self.config.verification_command = cfg
                        .verification_command
                        .as_deref()
                        .and_then(|value| {
                            sanitize_optional_text("verification_command", value).ok()
                        })
                        .flatten();

                    self.config.ignored_dirs = match sanitize_ignored_dirs(cfg.ignored_dirs) {
                        Ok(dirs) => dirs,
                        Err(error) => {
                            let _ = self.emit_log(&format!(
                                "provider_config: {}; ignoring ignored_dirs",
                                error
                            ));
                            Vec::new()
                        }
                    };
                } else {
                    // Malformed JSON: fall back to defaults but surface a useful, non-secret
                    // diagnostic instead of hiding the problem.
                    let _ = self.emit_log("provider_config: malformed JSON; using defaults");
                }
            } else {
                // File present but unreadable (permissions, locked): fall back to defaults.
                let _ = self.emit_log("provider_config: unreadable; using defaults");
            }
        }
    }

    pub fn save_provider_config(&self) -> Result<(), String> {
        // Serialize the config first (api_key is skipped by serde)
        let json = serde_json::to_string_pretty(&self.config)
            .map_err(|e| format!("Failed to serialize provider config: {}", e))?;

        // Write to a temporary file in the same directory for atomic replace.
        let tmp_path = self.config_path.with_extension("tmp");

        if let Some(parent) = self.config_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create config directory: {}", e))?;
        }
        // Write to temp file, then rename on top of destination.
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&tmp_path)
                .map_err(|e| format!("Failed to create temp config file: {}", e))?;
            file.write_all(json.as_bytes())
                .map_err(|e| format!("Failed to write temp config file: {}", e))?;
            // Flush to disk to ensure durability before rename.
            file.sync_all()
                .map_err(|e| format!("Failed to sync temp config: {}", e))?;
        }

        // Replace strategy with retry for Windows transient file locks, plus explicit remove fallback on Windows.
        let mut rename_res = Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "Not attempted",
        ));
        for attempt in 0..3 {
            match std::fs::rename(&tmp_path, &self.config_path) {
                Ok(()) => {
                    rename_res = Ok(());
                    break;
                }
                Err(e) => {
                    rename_res = Err(e);
                    if attempt < 2 {
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                }
            }
        }

        if let Err(e) = rename_res {
            #[cfg(windows)]
            {
                if self.config_path.exists() {
                    let _ = std::fs::remove_file(&self.config_path);
                }
                if let Err(e2) = std::fs::rename(&tmp_path, &self.config_path) {
                    let _ = std::fs::remove_file(&tmp_path);
                    return Err(format!(
                        "Failed to replace provider config on Windows: {} (original: {})",
                        e2, e
                    ));
                }
            }
            #[cfg(not(windows))]
            {
                let _ = std::fs::remove_file(&tmp_path);
                return Err(format!("Failed to replace provider config: {}", e));
            }
        }

        Ok(())
    }

    fn skills_path(&self) -> PathBuf {
        self.config_path
            .parent()
            .map(crate::skills::default_skill_store_path)
            .unwrap_or_else(|| PathBuf::from("skills.json"))
    }

    fn applied_skill_block(&self, selected_ids: &[String], agent_kind: &str) -> AppliedSkillBlock {
        let store = crate::skills::load_skill_store(&self.skills_path());
        crate::skills::resolve_skills_for_agent(&store, selected_ids, Some(agent_kind))
    }

    fn instructions_path(&self) -> PathBuf {
        self.config_path
            .parent()
            .map(crate::instructions::default_instruction_store_path)
            .unwrap_or_else(|| PathBuf::from("instructions.json"))
    }

    fn applied_instruction_block(
        &self,
        project_root: &str,
        agent_kind: &str,
    ) -> crate::instructions::AppliedInstructionBlock {
        let mut store = crate::instructions::load_instruction_store(&self.instructions_path());
        // Merge discovered project instruction files (ARCHIFY.md, CLAUDE.md,
        // AGENTS.md, etc.) so they are applied to the execution context.
        let discovered = crate::instructions::discover_project_instruction_files(project_root);
        store.instructions.extend(discovered);
        crate::instructions::resolve_instructions(
            &store,
            Some(project_root),
            None,
            None,
            Some(agent_kind),
        )
    }

    pub(crate) fn load_history_from_disk(&mut self) {
        let history = read_task_history(&self.history_dir);
        let mut latest_active: Option<TaskRecord> = None;
        for (id, record) in history {
            // Restore the in-memory baseline for tasks that were mid-flight, so
            // resumed/recovered work keeps accurate change attribution. Missing
            // or invalid baseline data degrades to "no attribution" rather than
            // fabricating changes.
            let baseline = record.task_baseline.clone();
            let state = record.state;
            let record_id = record.id.clone();
            if !record.state.is_terminal() {
                let is_newer = latest_active
                    .as_ref()
                    .map_or(true, |prev| record.updated_at > prev.updated_at);
                if is_newer {
                    latest_active = Some(record.clone());
                }
            }
            self.task_history.insert(id, record);
            let resumable = matches!(
                state,
                TaskState::Executing
                    | TaskState::Verifying
                    | TaskState::Reviewing
                    | TaskState::Retrying
            );
            if resumable {
                if let Some(b) = baseline {
                    self.task_baselines
                        .insert(record_id, baseline_from_persisted(&b));
                }
            }
        }
        if let Some(active) = latest_active {
            self.current_task = Some(active);
        }
    }

    /// Removes known secrets from text that is about to be persisted.
    ///
    /// A credential loaded for a provider operation (or supplied through the
    /// environment) could be echoed by a provider response. Task history is
    /// written to disk, so scrub those values before serialization.
    fn redact_secrets(&self, text: &str) -> String {
        let mut redacted = text.to_string();
        let mut secrets: Vec<String> = Vec::new();
        if let Some(ref key) = self.config.api_key {
            let key = key.trim();
            if key.len() >= 8 {
                secrets.push(key.to_string());
            }
        }
        // Environment-provided credentials can also appear in CLI output, so
        // scrub the well-known provider/agent variables before anything is
        // written to task history.
        for var in SECRET_ENV_VARS {
            if let Ok(value) = std::env::var(var) {
                let value = value.trim();
                if value.len() >= 8 {
                    secrets.push(value.to_string());
                }
            }
        }
        for secret in secrets {
            if redacted.contains(&secret) {
                redacted = redacted.replace(&secret, "[REDACTED]");
            }
        }
        redacted
    }

    /// Persists a task record atomically (write temp file, fsync, rename).
    ///
    /// A crash can therefore leave either the previous complete record or the
    /// new complete record in place, never a half-written JSON document.
    pub fn save_task_to_disk(&self, task: &TaskRecord) {
        // Persist the in-memory baseline alongside the task record so it can be
        // restored after a restart. A missing baseline is left as `None`.
        let mut task = task.clone();
        if let Some(baseline) = self.task_baselines.get(&task.id) {
            task.task_baseline = Some(persisted_baseline_from(baseline));
        }
        let task = &task;
        if let Err(e) = std::fs::create_dir_all(&self.history_dir) {
            self.emit_log(&format!("task_history: cannot create directory: {}", e));
            return;
        }
        let path = self.history_dir.join(format!("{}.json", task.id));
        let tmp_path = path.with_extension("tmp");
        let json = match serde_json::to_string_pretty(task) {
            Ok(json) => self.redact_secrets(&json),
            Err(e) => {
                self.emit_log(&format!(
                    "task_history: failed to serialize task {}: {}",
                    task.id, e
                ));
                return;
            }
        };

        let write_result = (|| -> std::io::Result<()> {
            use std::io::Write;
            let mut file = std::fs::File::create(&tmp_path)?;
            file.write_all(json.as_bytes())?;
            // Flush to disk before the rename so the rename cannot expose a
            // truncated record after a power loss.
            file.sync_all()?;
            Ok(())
        })();

        if let Err(e) = write_result {
            let _ = std::fs::remove_file(&tmp_path);
            self.emit_log(&format!(
                "task_history: failed to write task {}: {}",
                task.id, e
            ));
            return;
        }

        // Replace strategy with retry for Windows transient file locks.
        let mut rename_result = std::fs::rename(&tmp_path, &path);
        for _ in 0..2 {
            if rename_result.is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            rename_result = std::fs::rename(&tmp_path, &path);
        }

        if let Err(e) = rename_result {
            // Windows can refuse to replace an existing file while it is open.
            // Move the old record aside first and restore it if the new record
            // cannot be put in place, so a failed save never loses history.
            if path.exists() {
                let backup_path = path.with_extension("bak");
                let _ = std::fs::remove_file(&backup_path);
                if std::fs::rename(&path, &backup_path).is_ok() {
                    match std::fs::rename(&tmp_path, &path) {
                        Ok(()) => {
                            let _ = std::fs::remove_file(&backup_path);
                            return;
                        }
                        Err(e2) => {
                            // Restore the last good record.
                            let _ = std::fs::rename(&backup_path, &path);
                            let _ = std::fs::remove_file(&tmp_path);
                            self.emit_log(&format!(
                                "task_history: failed to persist task {}: {} (replace error: {})",
                                task.id, e2, e
                            ));
                            return;
                        }
                    }
                }
            }
            let _ = std::fs::remove_file(&tmp_path);
            self.emit_log(&format!(
                "task_history: failed to persist task {}: {}",
                task.id, e
            ));
        }
    }

    pub fn emit_update(&self, task: &TaskRecord) {
        if let Some(ref handle) = self.app_handle {
            let _ = handle.emit("orchestration_update", task);
        }
    }

    pub fn emit_log(&self, msg: &str) {
        if let Some(ref handle) = self.app_handle {
            let _ = handle.emit("orchestration_log", msg);
        }
    }

    /// Set the shared monitoring hub used to emit lifecycle and execution
    /// events. Called by the workspace so all tasks share one event stream
    /// with deduplication and history.
    pub fn set_monitoring_hub(&mut self, hub: MonitoringHub) {
        self.monitoring_hub = Some(hub);
    }

    /// Emits a structured monitoring event through the hub and broadcasts it
    /// over the Tauri event channel. The task must be non-terminal for events
    /// with task-level contexts; iteration events are validated as 1-based.
    fn emit_monitoring_event(
        &self,
        task_id: &str,
        iteration: Option<u32>,
        event: &ExecutionEvent,
    ) -> EmitOutcome {
        // Use the shared monitoring hub if available; otherwise fall back to a
        // headless hub so events are still recorded (but not delivered to a
        // consumer or deduplicated across calls).
        let hub = self
            .monitoring_hub
            .clone()
            .unwrap_or_else(MonitoringHub::headless);
        let context = match EventContext::with_iteration(task_id, iteration.unwrap_or(1)) {
            Ok(ctx) => ctx,
            Err(_) => EventContext::new(task_id).unwrap_or_else(|_| EventContext {
                task_id: task_id.to_string(),
                iteration: None,
            }),
        };
        hub.emit(&context, event)
    }

    /// Emit a lifecycle event through the shared monitoring hub, scoped to the
    /// given task and iteration.
    fn emit_lifecycle_event(
        &self,
        task_id: &str,
        iteration: Option<u32>,
        event: &LifecycleEvent,
    ) -> EmitOutcome {
        let hub = self
            .monitoring_hub
            .clone()
            .unwrap_or_else(MonitoringHub::headless);
        let context = match EventContext::with_iteration(task_id, iteration.unwrap_or(1)) {
            Ok(ctx) => ctx,
            Err(_) => EventContext::new(task_id).unwrap_or_else(|_| EventContext {
                task_id: task_id.to_string(),
                iteration: None,
            }),
        };
        hub.emit(&context, event)
    }

    /// Returns tasks that were mid-flight when the process last stopped and
    /// therefore require recovery. A plan awaiting approval is not included.
    pub fn interrupted_tasks(&self) -> Vec<TaskRecord> {
        let mut tasks: Vec<TaskRecord> = self
            .task_history
            .values()
            .filter(|t| t.state.is_interrupted_by_restart())
            .cloned()
            .collect();
        tasks.sort_by(|a, b| a.id.cmp(&b.id));
        tasks
    }

    /// Safely recovers from tasks interrupted mid-flight after a restart.
    ///
    /// No CLI command is ever re-run here: every mid-flight record is marked
    /// `Cancelled` with an explicit recovery-required error, persisted, and
    /// emitted to the UI. A task whose plan is merely awaiting approval keeps
    /// its state so the user can approve it explicitly. Resuming cancelled work
    /// requires a new, explicit decision rather than silent re-execution.
    /// Returns the IDs of all recovered tasks.
    pub fn recover_interrupted_tasks(&mut self) -> Vec<String> {
        let mut interrupted: Vec<TaskRecord> = self
            .task_history
            .values()
            .filter(|t| t.state.is_interrupted_by_restart())
            .cloned()
            .collect();

        // Defensive: an interrupted current task should always have a history
        // entry, but recover it even if the two ever drift apart.
        if let Some(current) = self.current_task.clone() {
            if current.state.is_interrupted_by_restart()
                && !interrupted.iter().any(|t| t.id == current.id)
            {
                interrupted.push(current);
            }
        }
        interrupted.sort_by(|a, b| a.id.cmp(&b.id));

        let mut recovered = Vec::new();
        for mut task in interrupted {
            let interrupted_state = task.state;
            if task.transition_to(TaskState::Cancelled).is_err() {
                // Active states are all cancellable; fall back defensively rather
                // than leaving a record that would block every future task.
                task.state = TaskState::Cancelled;
                task.updated_at = chrono::Utc::now().to_rfc3339();
            }
            task.error = Some(format!(
                "Task was interrupted by application restart while in state {:?} and was not \
                 resumed automatically. Review the workspace and start a new task to continue.",
                interrupted_state
            ));

            self.task_history.insert(task.id.clone(), task.clone());
            if self
                .current_task
                .as_ref()
                .map_or(false, |current| current.id == task.id)
            {
                self.current_task = Some(task.clone());
            }
            self.save_task_to_disk(&task);
            self.emit_update(&task);
            self.emit_log(&format!(
                "Task {} recovered after restart: marked cancelled, execution was not resumed.",
                task.id
            ));
            recovered.push(task.id.clone());
        }

        recovered
    }

    pub fn get_provider_status(&self) -> ProviderStatus {
        let is_test_mode =
            self.config.planner_type == "test" || self.config.executor_type == "test";
        // Empty environment variables must not be reported as a configured key.
        let env_key_present = PROVIDER_CONFIG_KEYS
            .iter()
            .any(|name| {
                std::env::var(name)
                    .map(|value| !value.trim().is_empty())
                    .unwrap_or(false)
            });
        let has_api_key = self
            .config
            .api_key
            .as_ref()
            .map_or(false, |k| !k.trim().is_empty())
            || env_key_present;
        let local_endpoint = self
            .config
            .cloud_endpoint
            .as_deref()
            .map_or(false, is_local_endpoint_url);

        ProviderStatus {
            planner_type: self.config.planner_type.clone(),
            executor_type: self.config.executor_type.clone(),
            is_test_mode,
            cloud_configured: self.config.cloud_endpoint.is_some()
                && (has_api_key || local_endpoint),
            cli_configured: self
                .config
                .cli_executable
                .as_ref()
                .map_or(false, |e| !e.trim().is_empty()),
            model_name: self
                .config
                .cloud_model
                .clone()
                .unwrap_or_else(|| "default".to_string()),
            cli_executable: self
                .config
                .cli_executable
                .clone()
                .unwrap_or_else(|| "none".to_string()),
            max_iterations: self.config.max_iterations,
            has_api_key,
            cloud_endpoint: self.config.cloud_endpoint.clone(),
            cli_args: self.config.cli_args.clone().unwrap_or_default(),
            cli_preset: self.config.cli_preset.clone(),
            prompt_delivery: self.config.prompt_delivery,
            verification_command: self.config.verification_command.clone().unwrap_or_default(),
            ignored_dirs: self.config.ignored_dirs.clone(),
        }
    }

    /// Validate and normalize a cloud endpoint URL.
    /// Returns `Some(normalized_url)` if valid, `None` if invalid.
    /// Valid: http:// or https:// scheme, non-empty host, numeric port if present,
    /// no control characters, no embedded credentials, no query/fragment.
    pub fn normalize_cloud_endpoint(url: &str) -> Option<String> {
        let trimmed = url.trim();
        if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
            return None;
        }
        if trimmed.chars().any(|c| c.is_control() || c.is_whitespace()) {
            return None;
        }
        if trimmed.contains('@') || trimmed.contains('?') || trimmed.contains('#') {
            return None;
        }

        let scheme_end = if trimmed.starts_with("http://") {
            "http://".len()
        } else {
            "https://".len()
        };
        let authority = trimmed[scheme_end..].split('/').next().unwrap_or("");
        if authority.is_empty() {
            return None;
        }

        // Validate the port when one is supplied (bracketed IPv6 hosts end before ':').
        if let Some(rest) = authority.strip_prefix('[') {
            let end = rest.find(']')?;
            if end == 0 {
                return None;
            }
            let after_host = &rest[end + 1..];
            if !after_host.is_empty() {
                let port = after_host.strip_prefix(':')?;
                if port.is_empty() || port.parse::<u16>().is_err() {
                    return None;
                }
            }
        } else if let Some((host, port)) = authority.rsplit_once(':') {
            if host.is_empty() || port.is_empty() || port.parse::<u16>().is_err() {
                return None;
            }
        }

        // Ensures a non-empty host without whitespace/control characters.
        crate::providers::endpoint_host(trimmed)?;
        Some(trimmed.to_string())
    }

    pub fn configure_providers(&mut self, input: ProviderConfigInput) -> Result<(), String> {
        // 1. Validate the complete input before mutating any field. A rejected
        //    batch must not leave the orchestrator partially reconfigured.
        let planner_type = match input.planner_type {
            Some(pt) => {
                let trimmed = pt.trim().to_string();
                if !matches!(trimmed.as_str(), "test" | "cloud_http") {
                    return Err(format!(
                        "Invalid planner_type '{}'. Expected one of: test, cloud_http.",
                        trimmed
                    ));
                }
                Some(trimmed)
            }
            None => None,
        };

        let executor_type = match input.executor_type {
            Some(et) => {
                let trimmed = et.trim().to_string();
                if !matches!(trimmed.as_str(), "test" | "process_cli") {
                    return Err(format!(
                        "Invalid executor_type '{}'. Expected one of: test, process_cli.",
                        trimmed
                    ));
                }
                Some(trimmed)
            }
            None => None,
        };

        let validated_endpoint = match input.cloud_endpoint {
            Some(ref ce) => {
                let trimmed = ce.trim();
                if trimmed.is_empty() {
                    Some(None)
                } else {
                    match Self::normalize_cloud_endpoint(trimmed) {
                        Some(valid) => {
                            // Keep the base URL only; the request builder appends
                            // `/chat/completions` when it is missing.
                            let base = valid
                                .trim_end_matches('/')
                                .trim_end_matches("/chat/completions");
                            Some(Some(base.to_string()))
                        }
                        None => return Err(format!("Invalid cloud endpoint URL: '{}'", trimmed)),
                    }
                }
            }
            None => None,
        };

        let cloud_model = match input.cloud_model {
            Some(ref model) => Some(sanitize_optional_text("cloud_model", model)?),
            None => None,
        };

        let max_iterations = match input.max_iterations {
            Some(mi) if (1..=10).contains(&mi) => Some(mi),
            Some(mi) => {
                return Err(format!(
                    "Invalid max_iterations: {mi}. Expected a value between 1 and 10."
                ));
            }
            None => None,
        };

        let cli_executable = match input.cli_executable {
            Some(ref exe) => Some(sanitize_optional_text("cli_executable", exe)?),
            None => None,
        };

        let cli_args = match input.cli_args {
            Some(args) => Some(sanitize_cli_args(args)?),
            None => None,
        };

        let cli_preset = match input.cli_preset {
            Some(ref preset) => Some(sanitize_optional_text("cli_preset", preset)?),
            None => None,
        };
        let prompt_delivery = input.prompt_delivery;

        let verification_command = match input.verification_command {
            Some(ref cmd) => Some(sanitize_optional_text("verification_command", cmd)?),
            None => None,
        };

        let ignored_dirs = match input.ignored_dirs {
            Some(dirs) => Some(sanitize_ignored_dirs(dirs)?),
            None => None,
        };

        // A blank key means "leave the existing credential unchanged". Non-blank
        // keys may not contain whitespace or control characters: they are interpolated
        // into an HTTP header, so newlines would be header injection.
        let api_key = match input.api_key {
            Some(ref key) => {
                let trimmed = key.trim();
                if trimmed.is_empty() {
                    None
                } else if trimmed.chars().any(|c| c.is_control() || c.is_whitespace()) {
                    return Err(
                        "Invalid api_key: keys must not contain whitespace or control characters"
                            .to_string(),
                    );
                } else {
                    Some(trimmed.to_string())
                }
            }
            None => None,
        };

        // Validate the scenario overrides before applying anything.
        if let Some(ref scenario) = input.test_planner_scenario {
            if contains_control_chars(scenario) {
                return Err(
                    "Invalid test_planner_scenario: must not contain control characters"
                        .to_string(),
                );
            }
        }
        if let Some(ref scenario) = input.test_executor_scenario {
            if contains_control_chars(scenario) {
                return Err(
                    "Invalid test_executor_scenario: must not contain control characters"
                        .to_string(),
                );
            }
        }

        // Resolve the executable against the *effective* post-update state, so
        // toggling back to process_cli reuses a previously saved executable.
        let effective_executor = executor_type
            .as_deref()
            .unwrap_or(self.config.executor_type.as_str());
        let effective_executable = match &cli_executable {
            Some(value) => value.clone(),
            None => self.config.cli_executable.clone(),
        };
        if effective_executor == "process_cli" {
            let executable = effective_executable.as_deref().unwrap_or("").trim();
            if executable.is_empty() {
                return Err(
                    "executor_type 'process_cli' requires cli_executable to be set to a \
                     resolvable program."
                        .to_string(),
                );
            }
            if resolve_executable(executable).is_none() {
                return Err(format!(
                    "CLI executable '{}' was not found on PATH or as a file path.",
                    executable
                ));
            }
        }

        // 2. Only apply mutations after the full validation pass succeeded.
        // Snapshot first so a failed persist can be rolled back, leaving memory and disk
        // consistent instead of reporting a success that did not stick.
        let previous_config = self.config.clone();
        if let Some(pt) = planner_type {
            self.config.planner_type = pt;
        }
        if let Some(et) = executor_type {
            self.config.executor_type = et;
        }
        if let Some(v) = validated_endpoint {
            self.config.cloud_endpoint = v;
        }
        if let Some(cm) = cloud_model {
            self.config.cloud_model = cm;
        }
        if let Some(key) = api_key {
            self.config.api_key = Some(key);
        }
        if let Some(ce) = cli_executable {
            self.config.cli_executable = ce;
        }
        if let Some(ca) = cli_args {
            self.config.cli_args = ca;
        }
        if let Some(preset) = cli_preset {
            self.config.cli_preset = preset;
        }
        if let Some(delivery) = prompt_delivery {
            self.config.prompt_delivery = delivery;
        }
        if let Some(vc) = verification_command {
            self.config.verification_command = vc;
        }
        if let Some(dirs) = ignored_dirs {
            self.config.ignored_dirs = dirs;
        }
        if let Some(tps) = input.test_planner_scenario {
            self.config.test_planner_scenario = Some(tps);
        }
        if let Some(tes) = input.test_executor_scenario {
            self.config.test_executor_scenario = Some(tes);
        }
        if let Some(mi) = max_iterations {
            self.config.max_iterations = mi;
        }
        // Persist non-secret settings. api_key is `skip_serializing`, so the
        // secret never reaches disk.
        if let Err(e) = self.save_provider_config() {
            // Roll back the in-memory mutation so a failed write cannot leave the app
            // running with settings that were never persisted.
            self.config = previous_config;
            return Err(format!("Failed to persist provider configuration: {}", e));
        }
        Ok(())
    }

    /// Attach the shared aggregate token-usage tracker.
    pub fn set_token_usage_tracker(&mut self, tracker: Option<Arc<StdMutex<TokenUsageTracker>>>) {
        self.token_usage_tracker = tracker;
    }

    /// Attach the workspace's model preferences and task-level model override.
    /// These are consulted by `select_model()` during planning and review.
    pub fn set_model_preferences(
        &mut self,
        preferences: Option<crate::model_selection::ModelPreferences>,
        task_model_id: Option<String>,
    ) {
        self.model_preferences = preferences;
        self.task_model_id = task_model_id;
    }

    /// Record explicit CLI token usage (or an honest unknown record when the
    /// CLI reported none) into the shared aggregate tracker.
    ///
    /// The `dedup_key` is stable for one executor result, so replayed or
    /// recovered results are never counted twice while genuinely new
    /// executions are still counted separately.
    pub fn record_cli_usage(
        &self,
        task_id: &str,
        scope: &str,
        iteration_started_at: &str,
        exec_result: &ExecutorResult,
    ) -> bool {
        let Some(ref tracker) = self.token_usage_tracker else {
            return false;
        };

        let mut context = self
            .current_task
            .as_ref()
            .filter(|task| task.id == task_id)
            .map(UsageContext::from_task)
            .unwrap_or_default();
        context.responsibility = Some("coding".to_string());

        let mut metadata = HashMap::new();
        metadata.insert("source".to_string(), "cli".to_string());
        if exec_result.timed_out {
            metadata.insert("timed_out".to_string(), "true".to_string());
        }
        if let Some(code) = exec_result.exit_code {
            metadata.insert("exit_code".to_string(), code.to_string());
        }

        let usage = match exec_result.token_usage.as_ref() {
            Some(cli) => {
                if let Some(model) = &cli.model {
                    context.model_id = Some(model.clone());
                }
                metadata.insert("usage_source".to_string(), "reported".to_string());
                if !cli.format.is_empty() {
                    metadata.insert("usage_format".to_string(), cli.format.clone());
                }
                metadata.insert(
                    "usage_completeness".to_string(),
                    match (cli.input_tokens, cli.output_tokens) {
                        (Some(_), Some(_)) => "complete",
                        _ => "partial",
                    }
                    .to_string(),
                );
                if let Some(value) = cli.cache_creation_input_tokens {
                    metadata.insert("cache_creation_input_tokens".to_string(), value.to_string());
                }
                if let Some(value) = cli.cache_read_input_tokens {
                    metadata.insert("cache_read_input_tokens".to_string(), value.to_string());
                }
                if let Some(value) = &cli.total_cost_usd {
                    metadata.insert("total_cost_usd".to_string(), value.clone());
                }
                TokenUsage::from_cli_usage(cli, Some(exec_result.duration_ms))
            }
            None => {
                // The CLI produced no machine-readable usage. Record that a
                // request happened while keeping the counts unknown; never
                // estimate from output size.
                metadata.insert("usage_source".to_string(), "unsupported".to_string());
                TokenUsage {
                    duration_ms: Some(exec_result.duration_ms),
                    ..TokenUsage::unknown()
                }
            }
        };
        context.metadata = metadata;

        let dedup_key = format!("cli:{}:{}:{}", task_id, scope, iteration_started_at);
        let Ok(mut guard) = tracker.lock() else {
            return false;
        };
        let recorded = guard
            .record_usage_deduplicated(usage, context, "execute".to_string(), Some(dedup_key))
            .is_some();
        if recorded {
            let _ = guard.save();
        }
        recorded
    }

    pub fn select_model(&self) -> Option<ModelSelection> {
        // Use workspace's populated ModelSelector if we have model preferences,
        // otherwise fall back to the orchestrator's limited selector
        if let Some(ref prefs) = self.model_preferences {
            let responsibility = crate::model_selection::AiResponsibility::Planning;
            // Use responsibility-specific preference first
            if let Some(pref) = prefs.responsibility_preferences.get(&responsibility) {
                if let Some(mid) = &pref.model_id {
                    let mut selector = ModelSelector::new();
                    selector.set_preferences(self.model_preferences.clone().unwrap_or_default());
                    if let Some(selection) = selector.select_model(
                        Some(&mid),
                        responsibility,
                        Some(self.config.planner_type.as_str()),
                    ) {
                        return Some(selection);
                    }
                }
                if pref.auto_select && prefs.auto_selection_enabled {
                    let mut selector = ModelSelector::new();
                    selector.set_preferences(self.model_preferences.clone().unwrap_or_default());
                    return selector
                        .auto_select(responsibility, Some(self.config.planner_type.as_str()));
                }
            }
            // Fall back to default model
            if let Some(model_id) = &prefs.default_model_id {
                let mut selector = ModelSelector::new();
                selector.set_preferences(self.model_preferences.clone().unwrap_or_default());
                if let Some(selection) = selector.select_model(
                    Some(&model_id),
                    responsibility,
                    Some(self.config.planner_type.as_str()),
                ) {
                    return Some(selection);
                }
            }
        }

        // Fall back to task_model_id-based selection with an empty selector
        // (caller should ideally have provider models available via workspace)
        let model_id = self.task_model_id.clone();
        let responsibility = crate::model_selection::AiResponsibility::Planning;
        if let Some(model_id) = model_id {
            let mut selector = ModelSelector::new();
            selector.set_preferences(self.model_preferences.clone().unwrap_or_default());
            if let Some(selection) = selector.select_model(
                Some(&model_id),
                responsibility,
                Some(self.config.planner_type.as_str()),
            ) {
                return Some(selection);
            }
        }

        None
    }

    pub fn create_planner(&self, override_model_id: Option<String>) -> Box<dyn PlannerReviewer> {
        if self.config.planner_type == "cloud_http" {
            let endpoint = self
                .config
                .cloud_endpoint
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
            // Use the selected model ID when override is provided, otherwise fall
            // back to the configured default model for this planner.
            let model = override_model_id
                .clone()
                .or_else(|| self.config.cloud_model.clone())
                .unwrap_or_else(|| "gpt-4o-mini".to_string());
            Box::new(CloudHttpPlannerReviewer::new(
                endpoint,
                model,
                self.config.api_key.clone(),
            ))
        } else {
            let scenario_str = self
                .config
                .test_planner_scenario
                .as_deref()
                .map(|s| s.to_lowercase().replace('_', ""));
            let scenario = match scenario_str.as_deref() {
                Some("needschangesonce") => TestPlannerScenario::NeedsChangesOnce,
                Some("alwaysneedschanges") => TestPlannerScenario::AlwaysNeedsChanges,
                Some("repeatedfailure") => TestPlannerScenario::RepeatedFailure,
                Some("blocked") => TestPlannerScenario::Blocked,
                Some("malformedplanresponse") => TestPlannerScenario::MalformedPlanResponse,
                Some("malformedreviewresponse") => TestPlannerScenario::MalformedReviewResponse,
                Some("simulatedapierror") => TestPlannerScenario::SimulatedApiError,
                Some("zerosteppplan") => TestPlannerScenario::ZeroStepPlan,
                Some("tasklevelneedschanges") => TestPlannerScenario::TaskLevelNeedsChanges,
                _ => TestPlannerScenario::NormalSuccess,
            };
            Box::new(TestPlannerReviewer::with_shared_count(
                scenario,
                self.test_planner_call_count.clone(),
            ))
        }
    }

    pub fn create_executor(&self) -> Box<dyn CodingExecutor> {
        if self.config.executor_type == "process_cli" {
            // Resolve the model ID using the model selector so display names
            // (e.g. "Arch-ai") are mapped to provider-specific IDs. Only append
            // a `--model=` flag when a selection actually resolves AND the
            // configured args do not already specify a model, so agents whose
            // args already carry `--model` (e.g. Aider) are not broken by a
            // duplicate/bogus flag. When nothing resolves we leave the CLI's
            // own default in place rather than injecting a Claude-specific ID.
            let model_selection = self.select_model();
            let mut args = self.config.cli_args.clone().unwrap_or_default();
            let already_has_model = args
                .iter()
                .any(|a| a == "--model" || a.starts_with("--model=") || a == "-m");
            if !already_has_model {
                if let Some(selection) = &model_selection {
                    args.push(format!("--model={}", selection.model_id));
                }
            }
            let executable = self
                .config
                .cli_executable
                .clone()
                .unwrap_or_else(|| "claude".to_string());
            Box::new(ProcessCliExecutor::with_prompt_delivery(
                executable,
                args,
                self.config.prompt_delivery,
                None,
                None,
            ))
        } else {
            let scenario = match self
                .config
                .test_executor_scenario
                .as_deref()
                .map(|s| s.to_lowercase().replace('_', ""))
                .as_deref()
            {
                Some("nonzeroexit") => TestExecutorScenario::NonzeroExit,
                Some("timeout") => TestExecutorScenario::Timeout,
                Some("executablenotfound") => TestExecutorScenario::ExecutableNotFound,
                Some("controlledhold") => TestExecutorScenario::ControlledHold,
                _ => TestExecutorScenario::NormalSuccess,
            };
            Box::new(TestExecutor::new(scenario))
        }
    }

    /// Reject a task start when the selected providers cannot actually run.
    ///
    /// `configure_providers` validates input at save time, but settings can also
    /// arrive from a hand-edited config file or a process restart where profile
    /// credentials are unavailable. Checking here turns those cases into an actionable
    /// error instead of a task that is created and then fails during planning.
    pub fn validate_provider_ready(&self) -> Result<(), String> {
        if self.config.planner_type == "cloud_http" {
            let endpoint = self
                .config
                .cloud_endpoint
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    "Cloud HTTP planner is not configured: set an endpoint URL in provider \
                     settings (a local server such as http://127.0.0.1:11434/v1 works without \
                     an API key)."
                        .to_string()
                })?;

            let has_key = self
                .config
                .api_key
                .as_ref()
                .map_or(false, |key| !key.trim().is_empty())
                || PROVIDER_CONFIG_KEYS
                    .iter()
                    .any(|name| {
                        std::env::var(name)
                            .map(|value| !value.trim().is_empty())
                            .unwrap_or(false)
                    });

            if !has_key && !is_local_endpoint_url(endpoint) {
                return Err(format!(
                    "Cloud HTTP planner is not configured: no API key is available for '{}'. \
                     Enter a key in provider settings or set PLANNER_API_KEY / OPENAI_API_KEY / \
                     GEMINI_API_KEY.",
                    endpoint
                ));
            }
        }

        if self.config.executor_type == "process_cli" {
            let executable = self
                .config
                .cli_executable
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| {
                    "Process CLI executor is not configured: set cli_executable to a program on \
                     PATH (for example `claude`) or an absolute file path."
                        .to_string()
                })?;

            if resolve_executable(executable).is_none() {
                return Err(format!(
                    "Process CLI executor is not configured: CLI executable '{}' was not found \
                     on PATH or as a file path.",
                    executable
                ));
            }
        }

        Ok(())
    }
}

/// Trim an optional text setting. An all-whitespace value clears the setting;
/// control characters are rejected so they cannot poison logs or child processes.
fn sanitize_optional_text(field: &str, value: &str) -> Result<Option<String>, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(format!(
            "Invalid {field}: must not contain control characters"
        ));
    }
    Ok(Some(trimmed.to_string()))
}

/// Trim CLI arguments, drop blanks, and reject control characters. Empty input
/// clears the saved arguments.
fn sanitize_cli_args(args: Vec<String>) -> Result<Option<Vec<String>>, String> {
    if args.len() > 64 {
        return Err("Invalid cli_args: at most 64 arguments are allowed".to_string());
    }
    let mut cleaned = Vec::new();
    for arg in args {
        let trimmed = arg.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.chars().any(|c| c.is_control()) {
            return Err(
                "Invalid cli_args: arguments must not contain control characters".to_string(),
            );
        }
        cleaned.push(trimmed.to_string());
    }
    if cleaned.is_empty() {
        Ok(None)
    } else {
        Ok(Some(cleaned))
    }
}

/// Trim ignored-directory entries, drop blanks and duplicates, and reject
/// control characters.
fn sanitize_ignored_dirs(dirs: Vec<String>) -> Result<Vec<String>, String> {
    let mut cleaned: Vec<String> = Vec::new();
    for dir in dirs {
        let trimmed = dir.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.chars().any(|c| c.is_control()) {
            return Err(
                "Invalid ignored_dirs: entries must not contain control characters".to_string(),
            );
        }
        if !cleaned.iter().any(|existing| existing == trimmed) {
            cleaned.push(trimmed.to_string());
        }
    }
    Ok(cleaned)
}

fn contains_control_chars(value: &str) -> bool {
    value.chars().any(|c| c.is_control())
}

/// Read every valid task record from a history directory.
///
/// Malformed or incomplete records are skipped so one bad file cannot hide the
/// rest of the history. If a primary record is corrupted or missing, a valid
/// backup (.bak) record is restored. Stale temporary (.tmp) files from interrupted
/// writes are cleaned up.
pub fn read_task_history(dir: &Path) -> HashMap<String, TaskRecord> {
    let mut history = HashMap::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut backups: HashMap<String, PathBuf> = HashMap::new();

        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str());
            if ext == Some("tmp") {
                // Incomplete temporary files must never be loaded as tasks.
                // Clean up stale temp files left behind by an interrupted write.
                let _ = std::fs::remove_file(&path);
                continue;
            }
            if ext == Some("bak") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    backups.insert(stem.to_string(), path);
                }
                continue;
            }
            if ext != Some("json") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(record) = serde_json::from_str::<TaskRecord>(&content) else {
                continue;
            };
            if record.id.is_empty() || record.user_task.trim().is_empty() {
                continue;
            }
            history.insert(record.id.clone(), record);
        }

        // For any task where the primary record was corrupted or missing,
        // recover from the backup (.bak) file if a valid one exists.
        for (id, bak_path) in backups {
            if !history.contains_key(&id) {
                if let Ok(content) = std::fs::read_to_string(&bak_path) {
                    if let Ok(record) = serde_json::from_str::<TaskRecord>(&content) {
                        if !record.id.is_empty() && !record.user_task.trim().is_empty() {
                            // Restore the valid record to primary .json so subsequent reads are direct.
                            let primary_path = bak_path.with_extension("json");
                            let _ = std::fs::write(&primary_path, &content);
                            history.insert(record.id.clone(), record);
                        }
                    }
                }
            }
        }
    }
    history
}

/// Convert an in-memory baseline into its serializable form.
fn persisted_baseline_from(
    baseline: &std::collections::HashMap<String, crate::verification::FileSnapshot>,
) -> PersistedBaseline {
    baseline
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                PersistedFileSnapshot {
                    mtime_secs: v.mtime_secs,
                    size: v.size,
                    hash: v.hash.clone(),
                },
            )
        })
        .collect()
}

/// Convert a persisted baseline back into the in-memory representation.
/// Invalid/missing entries are simply skipped by virtue of the outer
/// `Option`/`HashMap` deserialization; this never fabricates data.
fn baseline_from_persisted(
    p: &PersistedBaseline,
) -> std::collections::HashMap<String, crate::verification::FileSnapshot> {
    p.iter()
        .map(|(k, v)| {
            (
                k.clone(),
                crate::verification::FileSnapshot {
                    mtime_secs: v.mtime_secs,
                    size: v.size,
                    hash: v.hash.clone(),
                },
            )
        })
        .collect()
}

pub type SharedOrchestrator = Arc<tokio::sync::Mutex<Orchestrator>>;

/// Per-task metadata supplied by the workspace when creating a task.
#[derive(Debug, Clone, Default)]
pub struct TaskCreationOptions {
    pub project_id: Option<String>,
    pub provider_profile_id: Option<String>,
    pub agent_name: Option<String>,
    pub priority: i32,
    /// `Some(TaskState::Queued)` creates a queued task; otherwise the task
    /// starts planning immediately.
    pub initial_state: Option<TaskState>,
    /// Extra planner context, e.g. a transcript of a Prompt AI conversation.
    pub planning_context: Option<String>,
    /// Prompt AI session this task was created from, when applicable.
    pub source_session_id: Option<String>,
    /// Explicitly selected skill IDs. Empty means no skills are applied.
    pub selected_skill_ids: Vec<String>,
}

/// Create a task record without starting planning.
///
/// The caller decides whether the task starts planning right away or waits in
/// the workspace queue. `begin_planning` starts the planner for a created task.
pub async fn create_task(
    orch_lock: SharedOrchestrator,
    user_task: String,
    project_root: String,
    options: TaskCreationOptions,
) -> Result<TaskRecord, String> {
    if user_task.trim().is_empty() {
        return Err("Task description cannot be empty".to_string());
    }

    // Validate the requested project root before consulting orchestrator state.
    // Callers must get a path error for an invalid root even when a stale active
    // task exists; otherwise the real problem is masked by the duplicate check.
    let root_path = Path::new(&project_root);
    if !root_path.exists() {
        return Err(format!("Project root does not exist: {}", project_root));
    }
    let canonical_root = dunce::canonicalize(root_path)
        .map_err(|e| format!("Failed to canonicalize project root: {}", e))?;
    if !canonical_root.is_dir() {
        return Err(format!("Project root is not a directory: {}", project_root));
    }
    let root_str = canonical_root.to_string_lossy().to_string();

    // Prevent duplicate task execution within a single orchestrator. Workspace
    // runtimes are isolated per task, so this mainly protects legacy callers.
    {
        let orch = orch_lock.lock().await;
        if let Some(ref t) = orch.current_task {
            if t.state.is_active() {
                return Err(format!(
                    "A task is already active (id={}, state={:?}). Complete or cancel it before starting a new task.",
                    t.id, t.state
                ));
            }
        }
        for t in orch.task_history.values() {
            if t.state.is_active() {
                return Err(format!(
                    "An active task exists in history (id={}, state={:?}). Complete or cancel it before starting a new task.",
                    t.id, t.state
                ));
            }
        }
        // Fail before creating a task record when the selected providers cannot run.
        orch.validate_provider_ready()?;
    }

    let task_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let initial_state = options.initial_state.unwrap_or(TaskState::Planning);
    // A task must never be created already finished: terminal states are the
    // result of execution, not a valid starting point.
    if initial_state.is_terminal() {
        return Err(format!(
            "Cannot create a task in terminal state {:?}",
            initial_state
        ));
    }

    let mut task = TaskRecord {
        id: task_id.clone(),
        user_task: user_task.clone(),
        project_root: root_str.clone(),
        state: initial_state,
        plan: None,
        task_graph: None,
        iteration: 0,
        max_iterations: 3,
        iterations: Vec::new(),
        created_at: now.clone(),
        updated_at: now.clone(),
        error: None,
        project_id: options.project_id,
        provider_profile_id: options.provider_profile_id,
        agent_name: options.agent_name,
        priority: options.priority,
        queued_at: if initial_state == TaskState::Queued {
            Some(now)
        } else {
            None
        },
        recovery_attempts: 0,
        task_level_changes: None,
        task_baseline: None,
        execution_config: None,
        planning_context: options.planning_context,
        source_session_id: options.source_session_id,
        selected_skill_ids: options.selected_skill_ids.clone(),
        task_model_id: None,
    };

    {
        let mut orch = orch_lock.lock().await;
        task.max_iterations = orch.config.max_iterations;
        // Capture the resolved non-secret execution configuration so retries and
        // recovery use the original settings rather than a later profile change.
        task.execution_config = Some(orch.config.clone());
        orch.test_planner_call_count = Arc::new(AtomicU32::new(0));
        orch.current_task = Some(task.clone());
        orch.task_history.insert(task_id.clone(), task.clone());
        orch.save_task_to_disk(&task);
        orch.emit_update(&task);
        orch.emit_log(&format!(
            "Created task {}: '{}' (state: {:?})",
            task_id, user_task, initial_state
        ));
    }

    Ok(task)
}

/// Start planning for a task previously created by `create_task`.
///
/// Accepts a task in `Queued` or `Planning` state. The planner runs guarded by
/// cancellation and a panic supervisor so the task can never be stranded.
pub async fn begin_planning(
    orch_lock: SharedOrchestrator,
    task_id: String,
) -> Result<TaskRecord, String> {
    let (cancel_tx, cancel_rx) = watch::channel(false);

    let (task, user_task_clone, root_str_clone, planning_context) = {
        let mut orch = orch_lock.lock().await;
        let snapshot = {
            let cur_task = orch
                .current_task
                .as_mut()
                .ok_or_else(|| "No active task found".to_string())?;
            if cur_task.id != task_id {
                return Err(format!("Task ID mismatch: {} vs {}", cur_task.id, task_id));
            }
            if cur_task.state == TaskState::Queued {
                cur_task.transition_to(TaskState::Planning)?;
            } else if cur_task.state != TaskState::Planning {
                return Err(format!(
                    "Task {} cannot start planning from state {:?}",
                    task_id, cur_task.state
                ));
            }
            cur_task.queued_at = None;
            cur_task.updated_at = chrono::Utc::now().to_rfc3339();
            cur_task.clone()
        };
        orch.cancel_tx = Some(cancel_tx);
        orch.task_history.insert(task_id.clone(), snapshot.clone());
        orch.save_task_to_disk(&snapshot);
        orch.emit_update(&snapshot);
        orch.emit_log(&format!("Task {} started planning.", task_id));
        (
            snapshot.clone(),
            snapshot.user_task.clone(),
            snapshot.project_root.clone(),
            snapshot.planning_context.clone().unwrap_or_default(),
        )
    };

    spawn_planning(
        orch_lock,
        task_id,
        user_task_clone,
        root_str_clone,
        planning_context,
        cancel_rx,
    );

    Ok(task)
}

/// Legacy single-task entry point: create a task and start planning at once.
pub async fn start_task(
    orch_lock: SharedOrchestrator,
    user_task: String,
    project_root: String,
) -> Result<TaskRecord, String> {
    let task = create_task(
        orch_lock.clone(),
        user_task,
        project_root,
        TaskCreationOptions::default(),
    )
    .await?;
    begin_planning(orch_lock, task.id.clone()).await
}

fn spawn_planning(
    orch_clone: SharedOrchestrator,
    task_id_clone: String,
    user_task_clone: String,
    root_str_clone: String,
    planning_context: String,
    mut cancel_rx: watch::Receiver<bool>,
) {
    tokio::spawn(async move {
        let planner = {
            let orch = orch_clone.lock().await;
            orch.create_planner(None)
        };

        // Run the planner in its own task so a panic cannot strand the task in
        // Planning (which would block every future task until restart).
        let mut planner_task = tokio::spawn(async move {
            planner
                .plan(&user_task_clone, &root_str_clone, &planning_context)
                .await
        });

        let plan_res = tokio::select! {
            res = &mut planner_task => match res {
                Ok(result) => result,
                Err(e) if e.is_panic() => Err(
                    "Planner failed with an internal error; the task was stopped safely."
                        .to_string(),
                ),
                Err(e) => Err(format!("Planner task failed: {}", e)),
            },
            _ = cancel_rx.changed() => {
                planner_task.abort();
                Err("Task was cancelled during planning".to_string())
            }
        };

        let mut orch = orch_clone.lock().await;
        let mut updated_task: Option<TaskRecord> = None;
        if let Some(ref mut cur_task) = orch.current_task {
            if cur_task.id == task_id_clone {
                match plan_res {
                    Ok(plan) => match crate::models::TaskGraph::from_plan(&plan) {
                        Ok(graph) => {
                            // Only attach a plan if the transition succeeds. A task
                            // cancelled while planning must stay cancelled.
                            if cur_task.transition_to(TaskState::AwaitingApproval).is_ok() {
                                cur_task.plan = Some(plan);
                                cur_task.task_graph = Some(graph);
                            }
                        }
                        Err(e) => {
                            if cur_task.transition_to(TaskState::Failed).is_ok() {
                                cur_task.error =
                                    Some(format!("Failed to convert plan to graph: {}", e));
                            }
                        }
                    },
                    Err(e) => {
                        let cancelled = e.to_ascii_lowercase().contains("cancelled");
                        let target = if cancelled {
                            TaskState::Cancelled
                        } else {
                            TaskState::Failed
                        };
                        if cur_task.transition_to(target).is_ok() {
                            cur_task.error = Some(e);
                        }
                    }
                }
                cur_task.updated_at = chrono::Utc::now().to_rfc3339();
                updated_task = Some(cur_task.clone());
            }
        }

        if let Some(t) = updated_task {
            orch.task_history.insert(task_id_clone.clone(), t.clone());
            orch.save_task_to_disk(&t);
            orch.emit_update(&t);
            if t.state == TaskState::AwaitingApproval {
                orch.emit_log(&format!(
                    "Task {} plan generated. Awaiting user approval before execution.",
                    task_id_clone
                ));
            } else if t.state == TaskState::Failed {
                if let Some(ref err) = t.error {
                    orch.emit_log(&format!("Task {} planning failed: {}", task_id_clone, err));
                }
            }
        }
    });
}

pub async fn approve_plan(
    orch_lock: SharedOrchestrator,
    task_id: String,
) -> Result<TaskRecord, String> {
    launch_execution(orch_lock, task_id, false).await
}

/// Resume execution for a task that already has a saved plan.
///
/// Used by crash recovery and explicit retries. Unlike `approve_plan`, this
/// accepts a queued task and forces it directly into `Executing` so the saved
/// plan can continue without re-planning.
pub async fn resume_execution(
    orch_lock: SharedOrchestrator,
    task_id: String,
) -> Result<TaskRecord, String> {
    launch_execution(orch_lock, task_id, true).await
}

async fn launch_execution(
    orch_lock: SharedOrchestrator,
    task_id: String,
    resume: bool,
) -> Result<TaskRecord, String> {
    let mut orch = orch_lock.lock().await;
    let task_snapshot = {
        let cur_task = orch
            .current_task
            .as_mut()
            .ok_or_else(|| "No active task found".to_string())?;

        if cur_task.id != task_id {
            return Err(format!("Task ID mismatch: {} vs {}", cur_task.id, task_id));
        }

        if resume {
            if cur_task.state.is_terminal() {
                return Err(format!(
                    "Task {} is terminal ({:?}) and cannot be resumed",
                    task_id, cur_task.state
                ));
            }
            if cur_task.plan.is_none() {
                return Err(format!("Task {} has no saved plan to resume", task_id));
            }
            cur_task.state = TaskState::Executing;
        } else {
            if cur_task.plan.is_none() {
                return Err(format!("Task {} has no plan to execute", task_id));
            }
            cur_task.transition_to(TaskState::Executing)?;
        }
        if cur_task.iteration == 0 {
            cur_task.iteration = 1;
        }
        cur_task.error = None;
        cur_task.updated_at = chrono::Utc::now().to_rfc3339();
        cur_task.clone()
    };

    orch.task_history
        .insert(task_id.clone(), task_snapshot.clone());
    orch.save_task_to_disk(&task_snapshot);
    orch.emit_update(&task_snapshot);
    orch.emit_log(&format!(
        "{} for task {}. Launching execution loop.",
        if resume {
            "Resuming execution"
        } else {
            "Plan approved"
        },
        task_id
    ));

    // Prepare cancellation channel
    let (cancel_tx, cancel_rx) = watch::channel(false);
    orch.cancel_tx = Some(cancel_tx);

    // Spawn orchestration loop behind a supervisor. If the loop ever panics,
    // the task is failed explicitly instead of remaining active forever.
    let orch_clone = orch_lock.clone();
    let supervisor_task_id = task_id.clone();
    tokio::spawn(async move {
        let loop_handle = tokio::spawn(run_orchestration_loop(
            orch_clone.clone(),
            task_id,
            cancel_rx,
        ));

        if let Err(e) = loop_handle.await {
            if e.is_panic() {
                let message = "Orchestration stopped unexpectedly due to an internal error.";
                fail_task(&orch_clone, &supervisor_task_id, message).await;
                let orch = orch_clone.lock().await;
                orch.emit_log(&format!(
                    "Task {} internal orchestration error: {}",
                    supervisor_task_id, e
                ));
            }
        }
    });

    Ok(task_snapshot)
}

pub async fn reject_plan(
    orch_lock: SharedOrchestrator,
    task_id: String,
    reason: Option<String>,
) -> Result<TaskRecord, String> {
    let mut orch = orch_lock.lock().await;
    let snapshot = {
        let cur_task = orch
            .current_task
            .as_mut()
            .ok_or_else(|| "No active task found".to_string())?;

        if cur_task.id != task_id {
            return Err(format!("Task ID mismatch: {} vs {}", cur_task.id, task_id));
        }

        // A plan can only be rejected before execution starts. Rejecting a
        // task that is already executing or finished would silently cancel
        // in-flight work under the guise of a plan rejection.
        if cur_task.state != TaskState::Planning && cur_task.state != TaskState::AwaitingApproval {
            return Err(format!(
                "Task {} cannot reject a plan from state {:?}",
                task_id, cur_task.state
            ));
        }

        cur_task.transition_to(TaskState::Cancelled)?;
        cur_task.error = Some(reason.unwrap_or_else(|| "Plan was rejected by user".to_string()));
        cur_task.clone()
    };

    orch.task_history.insert(task_id.clone(), snapshot.clone());
    orch.save_task_to_disk(&snapshot);
    orch.emit_update(&snapshot);
    orch.emit_log(&format!(
        "Plan rejected for task {}. Task cancelled.",
        task_id
    ));

    Ok(snapshot)
}

pub async fn cancel_task(
    orch_lock: SharedOrchestrator,
    task_id: String,
) -> Result<TaskRecord, String> {
    let mut orch = orch_lock.lock().await;

    // Trigger cancellation
    if let Some(ref tx) = orch.cancel_tx {
        let _ = tx.send(true);
    }

    let snapshot = {
        let cur_task = orch
            .current_task
            .as_mut()
            .ok_or_else(|| "No active task found".to_string())?;

        if cur_task.id != task_id {
            return Err(format!("Task ID mismatch: {} vs {}", cur_task.id, task_id));
        }

        if !cur_task.state.is_terminal() {
            let _ = cur_task.transition_to(TaskState::Cancelled);
            cur_task.error = Some("Task cancelled by user".to_string());
        }

        cur_task.clone()
    };

    orch.task_history.insert(task_id.clone(), snapshot.clone());
    orch.save_task_to_disk(&snapshot);
    orch.emit_update(&snapshot);
    orch.emit_log(&format!("Task {} cancelled by user.", task_id));

    Ok(snapshot)
}

async fn run_orchestration_loop(
    orch_lock: SharedOrchestrator,
    task_id: String,
    cancel_rx: watch::Receiver<bool>,
) {
    // Check if task has a valid task_graph; if so, run the graph execution loop.
    let has_graph = {
        let orch = orch_lock.lock().await;
        orch.current_task
            .as_ref()
            .map_or(false, |t| t.id == task_id && t.task_graph.is_some())
    };

    if has_graph {
        run_graph_orchestration_loop(orch_lock, task_id, cancel_rx).await;
    } else {
        run_flat_orchestration_loop(orch_lock, task_id, cancel_rx).await;
    }
}

/// Record the token usage carried by one executor result.
async fn record_cli_usage_shared(
    orch_lock: &SharedOrchestrator,
    task_id: &str,
    scope: &str,
    iteration_started_at: &str,
    exec_result: &ExecutorResult,
) {
    let orch = orch_lock.lock().await;
    let _ = orch.record_cli_usage(task_id, scope, iteration_started_at, exec_result);
}

async fn update_and_persist_task<F>(
    orch_lock: &SharedOrchestrator,
    task_id: &str,
    f: F,
) -> Option<TaskRecord>
where
    F: FnOnce(&mut TaskRecord),
{
    let mut orch = orch_lock.lock().await;
    let snapshot = if let Some(ref mut cur) = orch.current_task {
        if cur.id == task_id {
            f(cur);
            // Every persisted snapshot must get a fresh timestamp so the
            // frontend can discard out-of-order event/invoke responses.
            cur.updated_at = chrono::Utc::now().to_rfc3339();
            Some(cur.clone())
        } else {
            None
        }
    } else {
        None
    };

    if let Some(ref s) = snapshot {
        orch.task_history.insert(s.id.clone(), s.clone());
        orch.save_task_to_disk(s);
        orch.emit_update(s);
    }
    snapshot
}

/// Captures a pre-execution filesystem baseline for a task if one does not
/// already exist. The baseline represents the workspace state before the first
/// coding-agent execution.
///
/// Returns `Ok(())` if a baseline was captured or already exists.
async fn capture_task_baseline(
    orch: &mut Orchestrator,
    task_id: &str,
    project_root: &str,
    ignored_dirs: &[String],
) -> Result<(), String> {
    if orch.task_baselines.contains_key(task_id) {
        return Ok(());
    }

    let result = snapshot_project(project_root, ignored_dirs).await;

    if !result.stable {
        // Unstable baseline: still capture it, but the caller should treat
        // attribution as limited rather than authoritative.
    }

    orch.task_baselines
        .insert(task_id.to_string(), result.entries);
    // Persist the baseline immediately so an interrupted task can still
    // attribute its changes after a restart.
    if let Some(cur) = orch.current_task.clone() {
        if cur.id == task_id {
            orch.save_task_to_disk(&cur);
        }
    }
    Ok(())
}

/// Computes iteration-level changes by diffing a pre-iteration snapshot
/// against the current workspace state.
///
/// Returns `(iteration_changes, diff_summary, post_snapshot_entries)`.
async fn compute_iteration_changes(
    project_root: &str,
    ignored_dirs: &[String],
    pre_snapshot: &HashMap<String, FileSnapshot>,
) -> (ChangedFiles, Option<String>, HashMap<String, FileSnapshot>) {
    let post_result = snapshot_project(project_root, ignored_dirs).await;

    if !post_result.stable {
        // Post-execution snapshot is unstable; still compute but attribution
        // should be treated as limited.
    }

    let iteration_changes = diff_snapshots(pre_snapshot, &post_result.entries);

    let diff_summary = if iteration_changes.is_empty() {
        None
    } else {
        Some(format!(
            "{} changed file(s) — {} created, {} modified, {} deleted.",
            iteration_changes.all_paths().len(),
            iteration_changes.created.len(),
            iteration_changes.modified.len(),
            iteration_changes.deleted.len()
        ))
    };

    (iteration_changes, diff_summary, post_result.entries)
}

async fn run_graph_orchestration_loop(
    orch_lock: SharedOrchestrator,
    task_id: String,
    mut cancel_rx: watch::Receiver<bool>,
) {
    loop {
        if *cancel_rx.borrow() {
            return;
        }

        let (
            mut graph,
            user_task,
            project_root,
            max_iterations,
            verif_cmd,
            ignored_dirs,
            planner,
            executor,
        ) = {
            let orch = orch_lock.lock().await;
            let task = match orch.current_task.as_ref() {
                Some(t) if t.id == task_id => t.clone(),
                _ => return,
            };
            if task.state == TaskState::Cancelled || task.state == TaskState::Failed {
                return;
            }
            let graph = match task.task_graph {
                Some(g) => g,
                None => return,
            };
            let selection = orch.select_model();
            (
                graph,
                task.user_task.clone(),
                task.project_root.clone(),
                task.max_iterations,
                orch.config.verification_command.clone(),
                orch.config.ignored_dirs.clone(),
                orch.create_planner(selection.as_ref().map(|s| s.model_id.clone())),
                orch.create_executor(),
            )
        };

        // Propagate any existing skips first
        let newly_skipped = graph.propagate_skips();
        if !newly_skipped.is_empty() {
            let g = graph.clone();
            update_and_persist_task(&orch_lock, &task_id, |cur| {
                cur.task_graph = Some(g);
            })
            .await;
        }

        // Check if graph is complete
        if graph.is_complete() {
            let has_failed = graph
                .components
                .iter()
                .any(|c| c.state == ComponentState::Failed);

            if has_failed {
                let mut orch = orch_lock.lock().await;

                // Compute task-level changes from baseline before terminal transition.
                let project_root = orch
                    .current_task
                    .as_ref()
                    .filter(|cur| cur.id == task_id)
                    .map(|cur| cur.project_root.clone());

                if let Some(project_root) = project_root {
                    let baseline = orch.task_baselines.get(&task_id).cloned();
                    if let Some(baseline) = baseline {
                        let result =
                            snapshot_project(&project_root, &orch.config.ignored_dirs).await;
                        let task_changes = diff_snapshots(&baseline, &result.entries);
                        if let Some(ref mut cur) = orch.current_task {
                            if cur.id == task_id {
                                cur.task_level_changes = Some(task_changes);
                            }
                        }
                    }
                    orch.task_baselines.remove(&task_id);
                }

                let snapshot = if let Some(ref mut cur) = orch.current_task {
                    if cur.id == task_id {
                        cur.task_graph = Some(graph.clone());
                        let _ = cur.transition_to(TaskState::Failed);
                        cur.error = Some(
                            "One or more components failed during graph execution".to_string(),
                        );
                        Some(cur.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };
                if let Some(s) = snapshot {
                    orch.task_history.insert(task_id.clone(), s.clone());
                    orch.save_task_to_disk(&s);
                    orch.emit_update(&s);
                    orch.emit_log(&format!(
                        "Graph task {} finished with state {:?}",
                        task_id, s.state
                    ));
                }
                return;
            }

            // Every component completed successfully. Persist the finished graph
            // first so the UI still shows component completion if the task-level
            // gate below fails.
            update_and_persist_task(&orch_lock, &task_id, |cur| {
                cur.task_graph = Some(graph.clone());
            })
            .await;

            let selection = orch_lock.lock().await.select_model();
            let (plan, user_task, project_root, verif_cmd, ignored_dirs, iterations, planner) = {
                let orch = orch_lock.lock().await;
                let task = match orch.current_task.as_ref() {
                    Some(t) if t.id == task_id => t.clone(),
                    _ => return,
                };
                (
                    task.plan.clone(),
                    task.user_task.clone(),
                    task.project_root.clone(),
                    orch.config.verification_command.clone(),
                    orch.config.ignored_dirs.clone(),
                    task.iterations.clone(),
                    orch.create_planner(selection.as_ref().map(|s| s.model_id.clone())),
                )
            };

            if *cancel_rx.borrow() {
                return;
            }

            // Task-level verification. Component-level verification already ran
            // per component; this final pass checks the aggregate workspace state
            // before the task is allowed to become Completed.
            update_and_persist_task(&orch_lock, &task_id, |cur| {
                let _ = cur.transition_to(TaskState::Verifying);
            })
            .await;
            {
                let orch = orch_lock.lock().await;
                orch.emit_log(&format!(
                    "Task {}: all graph components completed. Running task-level verification...",
                    task_id
                ));
            }

            let mut verifications = Vec::new();
            if let Some(ref cmd_str) = verif_cmd {
                if !cmd_str.trim().is_empty() {
                    let v_res = run_verification_command(cmd_str, &project_root).await;
                    verifications.push(v_res);
                }
            }

            if !verifications.is_empty() {
                let orch = orch_lock.lock().await;
                for verification in &verifications {
                    orch.emit_log(&format!(
                        "Task {}: task-level verification '{}' — {:?} (exit {:?}).",
                        task_id, verification.command, verification.status, verification.exit_code
                    ));
                }
            }

            if *cancel_rx.borrow() {
                return;
            }

            // Compute aggregate task-level attribution before review so the
            // reviewer sees the full change set and failure records retain it.
            let (task_changes, diff_summary) = {
                let mut orch = orch_lock.lock().await;
                let baseline = orch.task_baselines.remove(&task_id);
                let changes = if let Some(baseline) = baseline {
                    let result = snapshot_project(&project_root, &ignored_dirs).await;
                    diff_snapshots(&baseline, &result.entries)
                } else {
                    ChangedFiles::default()
                };
                let summary = if changes.is_empty() {
                    None
                } else {
                    Some(format!(
                        "{} changed file(s) — {} created, {} modified, {} deleted.",
                        changes.all_paths().len(),
                        changes.created.len(),
                        changes.modified.len(),
                        changes.deleted.len()
                    ))
                };
                if let Some(ref mut cur) = orch.current_task {
                    if cur.id == task_id {
                        cur.task_level_changes = Some(changes.clone());
                    }
                }
                (changes, summary)
            };

            update_and_persist_task(&orch_lock, &task_id, |cur| {
                let _ = cur.transition_to(TaskState::Reviewing);
            })
            .await;
            {
                let orch = orch_lock.lock().await;
                orch.emit_log(&format!(
                    "Task {}: running task-level reviewer evaluation...",
                    task_id
                ));
            }

            let plan = match plan {
                Some(plan) => plan,
                None => {
                    fail_task(
                        &orch_lock,
                        &task_id,
                        "Missing plan during task-level review",
                    )
                    .await;
                    return;
                }
            };

            let task_level_exec = iterations
                .iter()
                .rev()
                .find_map(|iteration| iteration.executor_result.clone())
                .unwrap_or(ExecutorResult {
                    exit_code: None,
                    stdout: String::new(),
                    stderr: String::new(),
                    timed_out: false,
                    duration_ms: 0,
                    token_usage: None,
                });
            let review_res = match planner
                .review(
                    &user_task,
                    &plan,
                    &plan.acceptance_criteria,
                    &task_changes.all_paths(),
                    diff_summary.as_deref(),
                    &task_level_exec,
                    &verifications,
                )
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    fail_task(
                        &orch_lock,
                        &task_id,
                        &format!("Task-level reviewer evaluation error: {}", e),
                    )
                    .await;
                    return;
                }
            };

            if *cancel_rx.borrow() {
                return;
            }

            // Build ExecutionReport and evaluate with recovery policy
            let report = crate::recovery_policy::ExecutionReport {
                task_description: user_task.clone(),
                acceptance_criteria: plan.acceptance_criteria.clone(),
                iteration: 1, // Task-level review happens once per task
                max_iterations: max_iterations,
                executor_result: Some(task_level_exec.clone()),
                verification_results: verifications.clone(),
                reviewer_response: Some(review_res.clone()),
                previous_unresolved_issues: vec![],
                cancelled: false,
                invalid_config_detail: None,
                changed_files_count: task_changes.all_paths().len(),
                verification_configured: verif_cmd
                    .as_ref()
                    .is_some_and(|cmd| !cmd.trim().is_empty()),
            };

            let policy_decision = crate::recovery_policy::evaluate(&report);

            match policy_decision {
                crate::recovery_policy::PolicyDecision::Complete => {
                    let mut orch = orch_lock.lock().await;
                    let snapshot = if let Some(ref mut cur) = orch.current_task {
                        if cur.id == task_id && !cur.state.is_terminal() {
                            let _ = cur.transition_to(TaskState::Completed);
                            Some(cur.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    };
                    if let Some(s) = snapshot {
                        orch.task_history.insert(task_id.clone(), s.clone());
                        orch.save_task_to_disk(&s);
                        orch.emit_update(&s);
                        orch.emit_log(&format!(
                            "Task {} completed successfully after task-level verification and review.",
                            task_id
                        ));
                    }
                }
                crate::recovery_policy::PolicyDecision::Continue {
                    corrective_prompt_hint,
                } => {
                    // Reset all components to Pending for re-execution with the corrective hint
                    let hint = corrective_prompt_hint.clone();
                    update_and_persist_task(&orch_lock, &task_id, |cur| {
                        cur.planning_context = Some(hint);
                        if let Some(ref mut g) = cur.task_graph {
                            for c in g.components.iter_mut() {
                                let _ = c.transition_to(ComponentState::Pending);
                            }
                        }
                    })
                    .await;
                    let orch = orch_lock.lock().await;
                    orch.emit_log(&format!(
                        "Task {}: task-level review requested changes, restarting components. Hint: {}",
                        task_id, corrective_prompt_hint
                    ));
                    continue; // Restart the outer loop to pick up ready components
                }
                crate::recovery_policy::PolicyDecision::Repair {
                    failure_class,
                    corrective_prompt_hint,
                } => {
                    // Reset all components to Pending for re-execution with the corrective hint
                    let hint = corrective_prompt_hint.clone();
                    update_and_persist_task(&orch_lock, &task_id, |cur| {
                        cur.planning_context = Some(hint);
                        if let Some(ref mut g) = cur.task_graph {
                            for c in g.components.iter_mut() {
                                let _ = c.transition_to(ComponentState::Pending);
                            }
                        }
                    })
                    .await;
                    let orch = orch_lock.lock().await;
                    orch.emit_log(&format!(
                        "Task {}: task-level review reported a {}, restarting components. Hint: {}",
                        task_id,
                        failure_class.label(),
                        corrective_prompt_hint
                    ));
                    continue; // Restart the outer loop to pick up ready components
                }
                crate::recovery_policy::PolicyDecision::Verify => {
                    // Restart components for additional verification
                    update_and_persist_task(&orch_lock, &task_id, |cur| {
                        if let Some(ref mut g) = cur.task_graph {
                            for c in g.components.iter_mut() {
                                let _ = c.transition_to(ComponentState::Pending);
                            }
                        }
                    })
                    .await;
                    let orch = orch_lock.lock().await;
                    orch.emit_log(&format!(
                        "Task {}: task-level review requested verification, restarting components.",
                        task_id
                    ));
                    continue; // Restart the outer loop
                }
                crate::recovery_policy::PolicyDecision::AskUser { reason } => {
                    fail_task(
                        &orch_lock,
                        &task_id,
                        &format!("Task-level review requires user input: {}", reason),
                    )
                    .await;
                }
                crate::recovery_policy::PolicyDecision::Stop { reason } => {
                    fail_task(
                        &orch_lock,
                        &task_id,
                        &format!("Task-level review blocked task: {:?}", reason),
                    )
                    .await;
                }
            }
            return;
        }

        // Check if graph is blocked (incomplete but no ready components)
        if graph.is_blocked() {
            let mut orch = orch_lock.lock().await;

            // Compute task-level changes from baseline before terminal transition.
            let project_root = orch
                .current_task
                .as_ref()
                .filter(|cur| cur.id == task_id)
                .map(|cur| cur.project_root.clone());

            if let Some(project_root) = project_root {
                let baseline = orch.task_baselines.get(&task_id).cloned();
                if let Some(baseline) = baseline {
                    let result = snapshot_project(&project_root, &orch.config.ignored_dirs).await;
                    let task_changes = diff_snapshots(&baseline, &result.entries);
                    if let Some(ref mut cur) = orch.current_task {
                        if cur.id == task_id {
                            cur.task_level_changes = Some(task_changes);
                        }
                    }
                }
                orch.task_baselines.remove(&task_id);
            }

            let snapshot = if let Some(ref mut cur) = orch.current_task {
                if cur.id == task_id {
                    cur.task_graph = Some(graph.clone());
                    let _ = cur.transition_to(TaskState::Failed);
                    cur.error = Some(
                        "Graph is blocked: no runnable components and unresolved failures/skips"
                            .to_string(),
                    );
                    Some(cur.clone())
                } else {
                    None
                }
            } else {
                None
            };
            if let Some(s) = snapshot {
                orch.task_history.insert(task_id.clone(), s.clone());
                orch.save_task_to_disk(&s);
                orch.emit_update(&s);
                orch.emit_log(&format!("Graph task {} blocked and failed.", task_id));
            }
            return;
        }

        let ready = graph.ready_ids();
        if ready.is_empty() {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            continue;
        }

        // Pick the first ready component
        let comp_id = ready[0].clone();
        let comp_desc = graph
            .components
            .iter()
            .find(|c| c.id == comp_id)
            .unwrap()
            .description
            .clone();

        // Transition component to Ready then Running and update task state to Executing
        {
            let cid = comp_id.clone();
            update_and_persist_task(&orch_lock, &task_id, |cur| {
                let _ = cur.transition_to(TaskState::Executing);
                if let Some(ref mut g) = cur.task_graph {
                    if let Some(c) = g.components.iter_mut().find(|c| c.id == cid) {
                        // If component is Pending, transition to Ready first
                        if c.state == ComponentState::Pending {
                            let _ = c.transition_to(ComponentState::Ready);
                        }
                        // Transition to Running
                        let _ = c.transition_to(ComponentState::Running);
                    }
                }
            })
            .await;
            let orch = orch_lock.lock().await;
            orch.emit_log(&format!(
                "Component {} ({}) started executing...",
                comp_id, comp_desc
            ));
        }

        // Execute component (with iteration/retry support per component up to max_iterations)
        let mut comp_iteration = 1;
        let mut previous_component_unresolved = Vec::new();
        let mut previous_corrective_hint: Option<String> = None;

        while comp_iteration <= max_iterations {
            if *cancel_rx.borrow() {
                update_and_persist_task(&orch_lock, &task_id, |cur| {
                    let _ = cur.transition_to(TaskState::Cancelled);
                    cur.error = Some("Task cancelled by user".to_string());
                })
                .await;
                return;
            }

            // Capture the real iteration start before prompt generation,
            // execution, verification, and review so duration metadata is
            // meaningful rather than collapsing to the completion timestamp.
            let iteration_started_at = chrono::Utc::now().to_rfc3339();

            // Build plan wrapper for the component
            let comp_plan = Plan {
                summary: comp_desc.clone(),
                steps: vec![comp_desc.clone()],
                acceptance_criteria: graph
                    .components
                    .iter()
                    .find(|c| c.id == comp_id)
                    .unwrap()
                    .acceptance_criteria
                    .clone(),
            };

            let feedback = if comp_iteration > 1 {
                let mut parts = Vec::new();
                if !previous_component_unresolved.is_empty() {
                    parts.push(format!(
                        "Previous issues: {:?}",
                        previous_component_unresolved
                    ));
                }
                if let Some(ref hint) = previous_corrective_hint {
                    parts.push(format!("Corrective hint: {}", hint));
                }
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join("\n"))
                }
            } else {
                None
            };

            let prompt = match planner
                .generate_coding_prompt(&user_task, &comp_plan, comp_iteration, feedback.as_deref())
                .await
            {
                Ok(p) => p,
                Err(e) => {
                    let cid = comp_id.clone();
                    update_and_persist_task(&orch_lock, &task_id, |cur| {
                        if let Some(ref mut g) = cur.task_graph {
                            if let Some(c) = g.components.iter_mut().find(|c| c.id == cid) {
                                let _ = c.transition_to(ComponentState::Failed);
                                c.error = Some(e.clone());
                            }
                        }
                    })
                    .await;
                    break;
                }
            };

            // Apply managed executor instructions (predictable, traceable). Does not modify project files.
            let prompt = {
                let block = {
                    let orch = orch_lock.lock().await;
                    orch.applied_instruction_block(&project_root, "executor")
                };
                if block.text.trim().is_empty() {
                    prompt
                } else {
                    format!("{}\n\n---\n\n{}", block.text.trim(), prompt)
                }
            };

            // Apply user-selected skills to the executor prompt (after instructions).
            let prompt = {
                let block = {
                    let orch = orch_lock.lock().await;
                    let skill_ids = orch
                        .current_task
                        .as_ref()
                        .map(|t| t.selected_skill_ids.clone())
                        .unwrap_or_default();
                    orch.applied_skill_block(&skill_ids, "executor")
                };
                if block.text.trim().is_empty() {
                    prompt
                } else {
                    format!("{}\n\n---\n\n{}", block.text.trim(), prompt)
                }
            };

            // Capture pre-execution baseline on the first component iteration
            // (before any coding-agent operation runs).
            if comp_iteration == 1 {
                let mut orch = orch_lock.lock().await;
                if let Err(e) =
                    capture_task_baseline(&mut orch, &task_id, &project_root, &ignored_dirs).await
                {
                    orch.emit_log(&format!(
                        "Component {} (attempt {}): warning: could not capture baseline: {}",
                        comp_id, comp_iteration, e
                    ));
                }
            }

            // Capture pre-iteration snapshot before the executor runs.
            let pre_iteration_result = snapshot_project(&project_root, &ignored_dirs).await;

            // Lifecycle tracking: record process start and emit through the
            // monitoring hub before handing off to the executor.
            let mut exec_tracker = AgentLifecycleTracker::new(TrackerConfig::default());
            {
                let orch = orch_lock.lock().await;
                let start_event = exec_tracker.record_start();
                let _ = orch.emit_lifecycle_event(&task_id, Some(comp_iteration), &start_event);
            }

            // Execute via executor
            let exec_result = match executor
                .execute(&prompt, &project_root, Some(&task_id), &mut cancel_rx)
                .await
            {
                Ok(res) => res,
                Err(e) => {
                    let is_cancelled = e.contains("cancelled");
                    let cid = comp_id.clone();
                    let orch = orch_lock.lock().await;
                    let terminal_event = if is_cancelled {
                        LifecycleEvent {
                            event_type: LifecycleEventType::ProcessCancelled,
                            timestamp: chrono::Utc::now(),
                            details: Some(e.clone()),
                            exit_code: None,
                        }
                    } else {
                        exec_tracker.record_failed(e.clone())
                    };
                    let _ =
                        orch.emit_lifecycle_event(&task_id, Some(comp_iteration), &terminal_event);
                    update_and_persist_task(&orch_lock, &task_id, |cur| {
                        if let Some(ref mut g) = cur.task_graph {
                            if let Some(c) = g.components.iter_mut().find(|c| c.id == cid) {
                                let _ = c.transition_to(ComponentState::Failed);
                                c.error = Some(e.clone());
                            }
                        }
                    })
                    .await;
                    fail_task(
                        &orch_lock,
                        &task_id,
                        &format!("Executor execution error: {}", e),
                    )
                    .await;
                    return;
                }
            };

            // Record the terminal lifecycle outcome and emit it.
            {
                let orch = orch_lock.lock().await;
                let terminal_event = if exec_result.timed_out {
                    exec_tracker.record_timed_out()
                } else {
                    exec_tracker.record_completed(exec_result.exit_code)
                };
                let _ = orch.emit_lifecycle_event(&task_id, Some(comp_iteration), &terminal_event);
            }

            record_cli_usage_shared(
                &orch_lock,
                &task_id,
                &format!("component:{}:{}", comp_id, comp_iteration),
                &iteration_started_at,
                &exec_result,
            )
            .await;

            {
                let orch = orch_lock.lock().await;
                orch.emit_log(&format!(
                    "Component {} (attempt {}): executor finished with exit code {:?} in {} ms{}.",
                    comp_id,
                    comp_iteration,
                    exec_result.exit_code,
                    exec_result.duration_ms,
                    if exec_result.timed_out {
                        " (timed out)"
                    } else {
                        ""
                    }
                ));
            }

            // Collect changed files via baseline-aware diff: pre-iteration
            // snapshot vs post-execution state. Excludes pre-existing changes.
            let (changed_files, diff_summary, _post_entries) = compute_iteration_changes(
                &project_root,
                &ignored_dirs,
                &pre_iteration_result.entries,
            )
            .await;

            {
                let orch = orch_lock.lock().await;
                orch.emit_log(&format!(
                    "Component {} (attempt {}): {} changed file(s) — {} created, {} modified, {} deleted.",
                    comp_id,
                    comp_iteration,
                    changed_files.all_paths().len(),
                    changed_files.created.len(),
                    changed_files.modified.len(),
                    changed_files.deleted.len()
                ));
            }

            // Transition component to Verifying
            {
                let cid = comp_id.clone();
                update_and_persist_task(&orch_lock, &task_id, |cur| {
                    let _ = cur.transition_to(TaskState::Verifying);
                    if let Some(ref mut g) = cur.task_graph {
                        if let Some(c) = g.components.iter_mut().find(|c| c.id == cid) {
                            let _ = c.transition_to(ComponentState::Verifying);
                        }
                    }
                })
                .await;
            }

            let mut verifications = Vec::new();
            if let Some(ref cmd_str) = verif_cmd {
                if !cmd_str.trim().is_empty() {
                    let v_res = run_verification_command(cmd_str, &project_root).await;
                    verifications.push(v_res);
                }
            }

            if !verifications.is_empty() {
                let orch = orch_lock.lock().await;
                for verification in &verifications {
                    orch.emit_log(&format!(
                        "Component {} (attempt {}): verification '{}' — {:?} (exit {:?}).",
                        comp_id,
                        comp_iteration,
                        verification.command,
                        verification.status,
                        verification.exit_code
                    ));
                }
            }

            // Transition component to Reviewing
            {
                update_and_persist_task(&orch_lock, &task_id, |cur| {
                    let _ = cur.transition_to(TaskState::Reviewing);
                })
                .await;
            }

            let review_res = match planner
                .review(
                    &user_task,
                    &comp_plan,
                    &comp_plan.acceptance_criteria,
                    &changed_files.all_paths(),
                    diff_summary.as_deref(),
                    &exec_result,
                    &verifications,
                )
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    let cid = comp_id.clone();
                    update_and_persist_task(&orch_lock, &task_id, |cur| {
                        if let Some(ref mut g) = cur.task_graph {
                            if let Some(c) = g.components.iter_mut().find(|c| c.id == cid) {
                                let _ = c.transition_to(ComponentState::Failed);
                                c.error = Some(e.clone());
                            }
                        }
                    })
                    .await;
                    break;
                }
            };

            let iter_record = IterationRecord {
                iteration: comp_iteration,
                coding_prompt: prompt,
                executor_result: Some(exec_result.clone()),
                changed_files: changed_files.clone(),
                diff_summary,
                verification_results: verifications.clone(),
                reviewer_response: Some(review_res.clone()),
                started_at: iteration_started_at,
                completed_at: Some(chrono::Utc::now().to_rfc3339()),
            };

            // Build ExecutionReport and evaluate with recovery policy
            let report = crate::recovery_policy::ExecutionReport {
                task_description: user_task.clone(),
                acceptance_criteria: comp_plan.acceptance_criteria.clone(),
                iteration: comp_iteration,
                max_iterations,
                executor_result: Some(exec_result),
                verification_results: verifications,
                reviewer_response: Some(review_res.clone()),
                previous_unresolved_issues: previous_component_unresolved.clone(),
                cancelled: false,
                invalid_config_detail: None,
                changed_files_count: changed_files.all_paths().len(),
                verification_configured: verif_cmd
                    .as_ref()
                    .is_some_and(|cmd| !cmd.trim().is_empty()),
            };

            let policy_decision = crate::recovery_policy::evaluate(&report);

            match policy_decision {
                crate::recovery_policy::PolicyDecision::Complete => {
                    let cid = comp_id.clone();
                    update_and_persist_task(&orch_lock, &task_id, |cur| {
                        cur.iterations.push(iter_record);
                        if let Some(ref mut g) = cur.task_graph {
                            if let Some(c) = g.components.iter_mut().find(|c| c.id == cid) {
                                let _ = c.transition_to(ComponentState::Completed);
                            }
                        }
                    })
                    .await;
                    let orch = orch_lock.lock().await;
                    orch.emit_log(&format!("Component {} completed successfully.", comp_id));
                    break;
                }
                crate::recovery_policy::PolicyDecision::Continue {
                    corrective_prompt_hint,
                } => {
                    // Store the hint in planning_context for next iteration's feedback
                    let cid = comp_id.clone();
                    update_and_persist_task(&orch_lock, &task_id, |cur| {
                        cur.iterations.push(iter_record);
                        if let Some(ref mut g) = cur.task_graph {
                            if let Some(c) = g.components.iter_mut().find(|c| c.id == cid) {
                                let _ = c.transition_to(ComponentState::Retrying);
                            }
                        }
                    })
                    .await;
                    previous_component_unresolved = review_res.unresolved_issues.clone();
                    previous_corrective_hint = Some(corrective_prompt_hint);
                    comp_iteration += 1;
                    continue;
                }
                crate::recovery_policy::PolicyDecision::Repair {
                    failure_class: _,
                    corrective_prompt_hint,
                } => {
                    // Store the hint in planning_context for next iteration's feedback
                    let cid = comp_id.clone();
                    update_and_persist_task(&orch_lock, &task_id, |cur| {
                        cur.iterations.push(iter_record);
                        if let Some(ref mut g) = cur.task_graph {
                            if let Some(c) = g.components.iter_mut().find(|c| c.id == cid) {
                                let _ = c.transition_to(ComponentState::Retrying);
                            }
                        }
                    })
                    .await;
                    previous_component_unresolved = review_res.unresolved_issues.clone();
                    previous_corrective_hint = Some(corrective_prompt_hint);
                    comp_iteration += 1;
                    continue;
                }
                crate::recovery_policy::PolicyDecision::Verify => {
                    let cid = comp_id.clone();
                    update_and_persist_task(&orch_lock, &task_id, |cur| {
                        cur.iterations.push(iter_record);
                        if let Some(ref mut g) = cur.task_graph {
                            if let Some(c) = g.components.iter_mut().find(|c| c.id == cid) {
                                let _ = c.transition_to(ComponentState::Retrying);
                            }
                        }
                    })
                    .await;
                    previous_component_unresolved = review_res.unresolved_issues.clone();
                    comp_iteration += 1;
                    continue;
                }
                crate::recovery_policy::PolicyDecision::AskUser { reason } => {
                    let cid = comp_id.clone();
                    update_and_persist_task(&orch_lock, &task_id, |cur| {
                        cur.iterations.push(iter_record);
                        if let Some(ref mut g) = cur.task_graph {
                            if let Some(c) = g.components.iter_mut().find(|c| c.id == cid) {
                                let _ = c.transition_to(ComponentState::Failed);
                            }
                        }
                    })
                    .await;
                    let orch = orch_lock.lock().await;
                    orch.emit_log(&format!("Component {} blocked: {}", comp_id, reason));
                    return;
                }
                crate::recovery_policy::PolicyDecision::Stop { reason } => {
                    let cid = comp_id.clone();
                    update_and_persist_task(&orch_lock, &task_id, |cur| {
                        cur.iterations.push(iter_record);
                        if let Some(ref mut g) = cur.task_graph {
                            if let Some(c) = g.components.iter_mut().find(|c| c.id == cid) {
                                let _ = c.transition_to(ComponentState::Failed);
                            }
                        }
                    })
                    .await;
                    let error_msg = format!("Component {} stopped: {:?}", comp_id, reason);
                    fail_task(&orch_lock, &task_id, &error_msg).await;
                    return;
                }
            }
        }
    }
}

async fn run_flat_orchestration_loop(
    orch_lock: SharedOrchestrator,
    task_id: String,
    mut cancel_rx: watch::Receiver<bool>,
) {
    let mut previous_unresolved_issues: Vec<String> = Vec::new();

    loop {
        // Step A: Read state
        let (task, planner, executor, verif_cmd, ignored_dirs) = {
            let orch = orch_lock.lock().await;
            let task = match orch.current_task.as_ref() {
                Some(t) if t.id == task_id => t.clone(),
                _ => return,
            };
            let selection = orch.select_model();
            let planner = orch.create_planner(selection.as_ref().map(|s| s.model_id.clone()));
            let executor = orch.create_executor();
            let verif_cmd = orch.config.verification_command.clone();
            let ignored_dirs = orch.config.ignored_dirs.clone();
            (task, planner, executor, verif_cmd, ignored_dirs)
        };

        if task.state == TaskState::Cancelled || task.state == TaskState::Failed {
            return;
        }

        let current_iteration = task.iteration;
        // Emit monitoring event for iteration start
        {
            let orch = orch_lock.lock().await;
            let _ = orch.emit_monitoring_event(
                &task_id,
                Some(current_iteration),
                &ExecutionEvent::IterationStarted {
                    task_id: task_id.clone(),
                    iteration: current_iteration,
                },
            );
        }

        // Capture the real iteration start before prompt generation, execution,
        // verification, and review so duration metadata is meaningful.
        let iteration_started_at = chrono::Utc::now().to_rfc3339();
        let plan = match task.plan.as_ref() {
            Some(p) => p.clone(),
            None => {
                fail_task(&orch_lock, &task_id, "Missing plan during execution").await;
                return;
            }
        };

        let feedback = if let Some(context) = &task.planning_context {
            if !context.is_empty() {
                Some(context.clone())
            } else {
                None
            }
        } else {
            // Fallback to the last iteration's reviewer response
            task.iterations.last().and_then(|it| {
                it.reviewer_response.as_ref().map(|r| {
                    format!(
                        "Reviewer findings: {}\nUnresolved: {:?}\nSuggested fix: {:?}",
                        r.findings, r.unresolved_issues, r.suggested_fix
                    )
                })
            })
        };

        let prompt = match planner
            .generate_coding_prompt(
                &task.user_task,
                &plan,
                current_iteration,
                feedback.as_deref(),
            )
            .await
        {
            Ok(p) => p,
            Err(e) => {
                fail_task(
                    &orch_lock,
                    &task_id,
                    &format!("Failed generating coding prompt: {}", e),
                )
                .await;
                return;
            }
        };

        // Apply managed executor instructions (predictable, traceable). Does not modify project files.
        let prompt = {
            let block = {
                let orch = orch_lock.lock().await;
                orch.applied_instruction_block(&task.project_root, "executor")
            };
            if block.text.trim().is_empty() {
                prompt
            } else {
                format!("{}\n\n---\n\n{}", block.text.trim(), prompt)
            }
        };

        // Apply user-selected skills to the executor prompt (after instructions).
        let prompt = {
            let block = {
                let orch = orch_lock.lock().await;
                let skill_ids = orch
                    .current_task
                    .as_ref()
                    .map(|t| t.selected_skill_ids.clone())
                    .unwrap_or_default();
                orch.applied_skill_block(&skill_ids, "executor")
            };
            if block.text.trim().is_empty() {
                prompt
            } else {
                format!("{}\n\n---\n\n{}", block.text.trim(), prompt)
            }
        };

        // Check cancellation
        if *cancel_rx.borrow() {
            return;
        }

        // Step C: Execute coding agent CLI
        // Capture pre-execution baseline on the first iteration (before any
        // coding-agent operation runs). The baseline is stored in-memory and
        // used for task-level attribution at completion/failure.
        if current_iteration == 1 {
            let mut orch = orch_lock.lock().await;
            if let Err(e) =
                capture_task_baseline(&mut orch, &task_id, &task.project_root, &ignored_dirs).await
            {
                orch.emit_log(&format!(
                    "Task {} [Iteration {}]: warning: could not capture baseline: {}",
                    task_id, current_iteration, e
                ));
            }
        }

        // Capture pre-iteration snapshot before the executor runs.
        let pre_iteration_result = snapshot_project(&task.project_root, &ignored_dirs).await;

        {
            update_and_persist_task(&orch_lock, &task_id, |cur| {
                let _ = cur.transition_to(TaskState::Executing);
            })
            .await;
            let orch = orch_lock.lock().await;
            orch.emit_log(&format!(
                "Task {} [Iteration {}]: Running executor '{}'...",
                task_id,
                current_iteration,
                executor.name()
            ));
        }

        // Lifecycle tracking: record process start and emit through the
        // monitoring hub before handing off to the executor.
        let mut exec_tracker = AgentLifecycleTracker::new(TrackerConfig::default());
        {
            let orch = orch_lock.lock().await;
            let start_event = exec_tracker.record_start();
            let _ = orch.emit_lifecycle_event(&task_id, Some(current_iteration), &start_event);
        }

        let exec_result = match executor
            .execute(&prompt, &task.project_root, Some(&task_id), &mut cancel_rx)
            .await
        {
            Ok(res) => res,
            Err(e) => {
                let is_cancelled = e.contains("cancelled");
                let orch = orch_lock.lock().await;
                let terminal_event = if is_cancelled {
                    LifecycleEvent {
                        event_type: LifecycleEventType::ProcessCancelled,
                        timestamp: chrono::Utc::now(),
                        details: Some(e.clone()),
                        exit_code: None,
                    }
                } else {
                    exec_tracker.record_failed(e.clone())
                };
                let _ =
                    orch.emit_lifecycle_event(&task_id, Some(current_iteration), &terminal_event);
                fail_task(
                    &orch_lock,
                    &task_id,
                    &format!("Executor execution error: {}", e),
                )
                .await;
                return;
            }
        };

        // Record the terminal lifecycle outcome and emit it.
        {
            let orch = orch_lock.lock().await;
            let terminal_event = if exec_result.timed_out {
                exec_tracker.record_timed_out()
            } else {
                exec_tracker.record_completed(exec_result.exit_code)
            };
            let _ = orch.emit_lifecycle_event(&task_id, Some(current_iteration), &terminal_event);
        }

        // Record any explicit CLI usage before cancellation/early-return checks:
        // the executor really ran, so its reported usage belongs in aggregates.
        record_cli_usage_shared(
            &orch_lock,
            &task_id,
            &format!("task-iteration:{}", current_iteration),
            &iteration_started_at,
            &exec_result,
        )
        .await;

        if *cancel_rx.borrow() {
            return;
        }

        {
            let orch = orch_lock.lock().await;
            orch.emit_log(&format!(
                "Task {} [Iteration {}]: Executor finished with exit code {:?} in {} ms{}.",
                task_id,
                current_iteration,
                exec_result.exit_code,
                exec_result.duration_ms,
                if exec_result.timed_out {
                    " (timed out)"
                } else {
                    ""
                }
            ));
        }

        // Step D: Compute iteration-level changes by diffing pre-iteration
        // snapshot against post-execution state. This excludes pre-existing
        // changes that were present in the baseline.
        let (changed_files, diff_summary, _post_entries) = compute_iteration_changes(
            &task.project_root,
            &ignored_dirs,
            &pre_iteration_result.entries,
        )
        .await;

        {
            let orch = orch_lock.lock().await;
            orch.emit_log(&format!(
                "Task {} [Iteration {}]: {} changed file(s) — {} created, {} modified, {} deleted.",
                task_id,
                current_iteration,
                changed_files.all_paths().len(),
                changed_files.created.len(),
                changed_files.modified.len(),
                changed_files.deleted.len()
            ));
        }

        // Step E: Run verification checks
        {
            let orch = orch_lock.lock().await;
            let _ = orch.emit_monitoring_event(
                &task_id,
                Some(current_iteration),
                &ExecutionEvent::TaskStateTransitioned {
                    task_id: task_id.clone(),
                    from: task.state,
                    to: TaskState::Verifying,
                    iteration: Some(current_iteration),
                },
            );
            update_and_persist_task(&orch_lock, &task_id, |cur| {
                let _ = cur.transition_to(TaskState::Verifying);
            })
            .await;
            orch.emit_log(&format!(
                "Task {} [Iteration {}]: Running verification checks...",
                task_id, current_iteration
            ));
        }

        let mut verifications = Vec::new();
        if let Some(ref cmd_str) = verif_cmd {
            if !cmd_str.trim().is_empty() {
                let v_res = run_verification_command(cmd_str, &task.project_root).await;
                verifications.push(v_res);
            }
        }

        // Emit verification completion event
        {
            let orch = orch_lock.lock().await;
            let passed = verifications
                .iter()
                .all(|v| v.status == VerificationStatus::Passed);
            let _ = orch.emit_monitoring_event(
                &task_id,
                Some(current_iteration),
                &ExecutionEvent::VerificationCompleted {
                    task_id: task_id.clone(),
                    status: if passed {
                        VerificationStatus::Passed
                    } else {
                        VerificationStatus::Failed
                    },
                    iteration: current_iteration,
                },
            );
        }

        if !verifications.is_empty() {
            let orch = orch_lock.lock().await;
            for verification in &verifications {
                orch.emit_log(&format!(
                    "Task {} [Iteration {}]: Verification '{}' — {:?} (exit {:?}).",
                    task_id,
                    current_iteration,
                    verification.command,
                    verification.status,
                    verification.exit_code
                ));
            }
        }

        if *cancel_rx.borrow() {
            return;
        }

        // Step F: Reviewer evaluation
        {
            update_and_persist_task(&orch_lock, &task_id, |cur| {
                let _ = cur.transition_to(TaskState::Reviewing);
            })
            .await;
            let orch = orch_lock.lock().await;
            orch.emit_log(&format!(
                "Task {} [Iteration {}]: Reviewer evaluating results...",
                task_id, current_iteration
            ));
        }

        let review_res = match planner
            .review(
                &task.user_task,
                &plan,
                &plan.acceptance_criteria,
                &changed_files.all_paths(),
                diff_summary.as_deref(),
                &exec_result,
                &verifications,
            )
            .await
        {
            Ok(r) => r,
            Err(e) => {
                fail_task(
                    &orch_lock,
                    &task_id,
                    &format!("Reviewer evaluation error: {}", e),
                )
                .await;
                return;
            }
        };

        // Step G: Evaluate Reviewer Verdict with Recovery Policy
        // Record iteration
        let iter_record = IterationRecord {
            iteration: current_iteration,
            coding_prompt: prompt,
            executor_result: Some(exec_result.clone()),
            changed_files: changed_files.clone(),
            diff_summary: diff_summary.clone(),
            verification_results: verifications.clone(),
            reviewer_response: Some(review_res.clone()),
            started_at: iteration_started_at,
            completed_at: Some(chrono::Utc::now().to_rfc3339()),
        };

        // Emit monitoring event for review completion
        {
            let orch = orch_lock.lock().await;
            let _ = orch.emit_monitoring_event(
                &task.id,
                Some(current_iteration),
                &ExecutionEvent::ReviewCompleted {
                    task_id: task.id.clone(),
                    verdict: review_res.verdict,
                    iteration: current_iteration,
                },
            );
        }

        // Build the ExecutionReport for the recovery policy
        let report = crate::recovery_policy::ExecutionReport {
            task_description: task.user_task.clone(),
            acceptance_criteria: plan.acceptance_criteria.clone(),
            iteration: current_iteration,
            max_iterations: task.max_iterations,
            executor_result: Some(exec_result.clone()),
            verification_results: verifications.clone(),
            reviewer_response: Some(review_res.clone()),
            previous_unresolved_issues: previous_unresolved_issues.clone(),
            cancelled: false,            // Already checked earlier in the loop
            invalid_config_detail: None, // We don't have this in the flat loop; could be sourced from config if needed
            changed_files_count: changed_files.all_paths().len(),
            verification_configured: verif_cmd.as_ref().is_some_and(|cmd| !cmd.trim().is_empty()),
        };

        let policy_decision = crate::recovery_policy::evaluate(&report);

        // Handle policy decision
        match policy_decision {
            crate::recovery_policy::PolicyDecision::Complete => {
                // Push the iter_record and transition to Completed
                update_and_persist_task(&orch_lock, &task_id, |cur| {
                    cur.iterations.push(iter_record);
                    let _ = cur.transition_to(TaskState::Completed);
                })
                .await;
                let orch = orch_lock.lock().await;
                orch.emit_log(&format!(
                    "Task {} completed successfully on iteration {}! All acceptance criteria met.",
                    task_id, current_iteration
                ));
                return;
            }
            crate::recovery_policy::PolicyDecision::Continue {
                corrective_prompt_hint,
            } => {
                // Store the hint in planning_context for next iteration's feedback
                update_and_persist_task(&orch_lock, &task_id, |cur| {
                    cur.iterations.push(iter_record);
                    let _ = cur.transition_to(TaskState::Retrying);
                    cur.iteration += 1;
                    cur.planning_context = Some(corrective_prompt_hint);
                })
                .await;
                let orch = orch_lock.lock().await;
                orch.emit_log(&format!(
                    "Task {} [Iteration {}] needs revisions. Starting iteration {}...",
                    task_id, current_iteration, task.iteration
                ));
                previous_unresolved_issues = review_res.unresolved_issues.clone();
            }
            crate::recovery_policy::PolicyDecision::Repair {
                failure_class: _,
                corrective_prompt_hint,
            } => {
                // Store the hint in planning_context for next iteration's feedback
                update_and_persist_task(&orch_lock, &task_id, |cur| {
                    cur.iterations.push(iter_record);
                    let _ = cur.transition_to(TaskState::Retrying);
                    cur.iteration += 1;
                    cur.planning_context = Some(corrective_prompt_hint);
                })
                .await;
                let orch = orch_lock.lock().await;
                orch.emit_log(&format!(
                    "Task {} [Iteration {}] requires repair. Starting iteration {}...",
                    task_id, current_iteration, task.iteration
                ));
                previous_unresolved_issues = review_res.unresolved_issues.clone();
            }
            crate::recovery_policy::PolicyDecision::Verify => {
                // No specific feedback, we'll just try again (planning_context remains None)
                update_and_persist_task(&orch_lock, &task_id, |cur| {
                    cur.iterations.push(iter_record);
                    let _ = cur.transition_to(TaskState::Retrying);
                    cur.iteration += 1;
                })
                .await;
                let orch = orch_lock.lock().await;
                orch.emit_log(&format!(
                    "Task {} [Iteration {}] needs verification. Starting iteration {}...",
                    task_id, current_iteration, task.iteration
                ));
                previous_unresolved_issues = review_res.unresolved_issues.clone();
            }
            crate::recovery_policy::PolicyDecision::AskUser { reason } => {
                // Fail the task and let the user decide
                update_and_persist_task(&orch_lock, &task_id, |cur| {
                    cur.iterations.push(iter_record);
                    let _ = cur.transition_to(TaskState::Failed);
                    cur.error = Some(format!("User input required: {}", reason));
                })
                .await;
                let orch = orch_lock.lock().await;
                orch.emit_log(&format!(
                    "Task {} failed: user input required - {}",
                    task_id, reason
                ));
                return;
            }
            crate::recovery_policy::PolicyDecision::Stop { reason } => {
                // Fail the task with the stop reason
                update_and_persist_task(&orch_lock, &task_id, |cur| {
                    cur.iterations.push(iter_record);
                    let _ = cur.transition_to(TaskState::Failed);
                    cur.error = Some(format!("Stopped: {:?}", reason));
                })
                .await;
                let orch = orch_lock.lock().await;
                orch.emit_log(&format!("Task {} stopped: {:?}", task_id, reason));
                return;
            }
        }
    }
}

async fn fail_task(orch_lock: &SharedOrchestrator, task_id: &str, error_msg: &str) {
    let mut orch = orch_lock.lock().await;

    // Compute task-level changes from baseline before failing, so the
    // failure record reflects what actually changed during execution.
    let project_root = orch
        .current_task
        .as_ref()
        .filter(|cur| cur.id == task_id)
        .map(|cur| cur.project_root.clone());

    if let Some(project_root) = project_root {
        let baseline = orch.task_baselines.get(task_id).cloned();
        if let Some(baseline) = baseline {
            let result = snapshot_project(&project_root, &orch.config.ignored_dirs).await;
            let task_changes = diff_snapshots(&baseline, &result.entries);
            if let Some(ref mut cur) = orch.current_task {
                if cur.id == task_id {
                    cur.task_level_changes = Some(task_changes);
                }
            }
        }
        orch.task_baselines.remove(task_id);
    }

    let snapshot = if let Some(ref mut cur) = orch.current_task {
        if cur.id == task_id && !cur.state.is_terminal() {
            let _ = cur.transition_to(TaskState::Failed);
            cur.error = Some(error_msg.to_string());
            Some(cur.clone())
        } else {
            None
        }
    } else {
        None
    };

    if let Some(s) = snapshot {
        orch.task_history.insert(task_id.to_string(), s.clone());
        orch.save_task_to_disk(&s);
        orch.emit_update(&s);
        orch.emit_log(&format!("Task {} failed: {}", task_id, error_msg));
    }
}
