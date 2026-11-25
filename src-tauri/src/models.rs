use serde::{Deserialize, Serialize};
use serde_json;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Idle,
    Queued,
    Planning,
    AwaitingApproval,
    Executing,
    Verifying,
    Reviewing,
    Retrying,
    Completed,
    Failed,
    Cancelled,
}

impl TaskState {
    pub fn can_transition_to(&self, next: &TaskState) -> bool {
        match (self, next) {
            // From Idle
            (TaskState::Idle, TaskState::Queued) => true,
            (TaskState::Idle, TaskState::Planning) => true,

            // From Queued
            (TaskState::Queued, TaskState::Planning) => true,
            (TaskState::Queued, TaskState::Cancelled) => true,

            // From Planning
            (TaskState::Planning, TaskState::AwaitingApproval) => true,
            (TaskState::Planning, TaskState::Failed) => true,
            (TaskState::Planning, TaskState::Cancelled) => true,

            // From AwaitingApproval
            (TaskState::AwaitingApproval, TaskState::Executing) => true,
            (TaskState::AwaitingApproval, TaskState::Cancelled) => true,
            (TaskState::AwaitingApproval, TaskState::Failed) => true,

            // From Executing
            (TaskState::Executing, TaskState::Verifying) => true,
            (TaskState::Executing, TaskState::Reviewing) => true, // if verifying skipped
            (TaskState::Executing, TaskState::Failed) => true,
            (TaskState::Executing, TaskState::Cancelled) => true,

            // From Verifying
            (TaskState::Verifying, TaskState::Verifying) => true, // re-verification loop
            (TaskState::Verifying, TaskState::Reviewing) => true,
            (TaskState::Verifying, TaskState::Completed) => true, // verification passed, no review needed
            (TaskState::Verifying, TaskState::Retrying) => true, // verification failed, trigger retry
            (TaskState::Verifying, TaskState::Failed) => true,
            (TaskState::Verifying, TaskState::Cancelled) => true,

            // From Reviewing
            (TaskState::Reviewing, TaskState::Verifying) => true, // task-level re-verification after component review
            (TaskState::Reviewing, TaskState::Completed) => true,
            (TaskState::Reviewing, TaskState::Retrying) => true,
            (TaskState::Reviewing, TaskState::Failed) => true,
            (TaskState::Reviewing, TaskState::Cancelled) => true,

            // From Retrying
            (TaskState::Retrying, TaskState::Executing) => true,
            (TaskState::Retrying, TaskState::Failed) => true,
            (TaskState::Retrying, TaskState::Cancelled) => true,

            // Terminal states cannot transition
            (TaskState::Completed, _) => false,
            (TaskState::Failed, _) => false,
            (TaskState::Cancelled, _) => false,

            _ => false,
        }
    }

    pub fn validate_transition(&self, next: &TaskState) -> Result<(), String> {
        if self.can_transition_to(next) {
            Ok(())
        } else {
            Err(format!(
                "Invalid state transition from {:?} to {:?}",
                self, next
            ))
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            TaskState::Completed | TaskState::Failed | TaskState::Cancelled
        )
    }

    pub fn is_active(&self) -> bool {
        !self.is_terminal() && *self != TaskState::Idle
    }

    /// Whether the task is waiting in the scheduler queue.
    pub fn is_queued(&self) -> bool {
        matches!(self, TaskState::Queued)
    }

    /// Whether the task is in the planning/approval phase.
    pub fn is_planning(&self) -> bool {
        matches!(self, TaskState::Planning | TaskState::AwaitingApproval)
    }

    /// Whether the task is in an actual execution/verification/review phase.
    pub fn is_running(&self) -> bool {
        matches!(
            self,
            TaskState::Executing
                | TaskState::Verifying
                | TaskState::Reviewing
                | TaskState::Retrying
        )
    }

    /// Whether the task is waiting for user input (approval).
    pub fn is_holding(&self) -> bool {
        matches!(self, TaskState::AwaitingApproval)
    }

    /// Whether the task is in a paused state.
    pub fn is_paused(&self) -> bool {
        false // Pause is not a task state in current architecture
    }

    /// Whether this task is currently consuming one of the scheduler's
    /// concurrency slots. A plan waiting for approval does not block another
    /// task from planning or running.
    pub fn occupies_execution_slot(&self) -> bool {
        matches!(
            self,
            TaskState::Planning
                | TaskState::Executing
                | TaskState::Verifying
                | TaskState::Reviewing
                | TaskState::Retrying
        )
    }

    /// States that were mid-flight when the process stopped and therefore must
    /// not be resumed automatically after a restart.
    ///
    /// `AwaitingApproval` is deliberately excluded: the plan is already complete,
    /// no CLI work has started, and restoring it is safe because execution still
    /// requires an explicit user approval.
    pub fn is_interrupted_by_restart(&self) -> bool {
        matches!(
            self,
            TaskState::Planning
                | TaskState::Executing
                | TaskState::Verifying
                | TaskState::Reviewing
                | TaskState::Retrying
        )
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub summary: String,
    pub steps: Vec<String>,
    pub acceptance_criteria: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ExecutorResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub duration_ms: u64,
    /// Explicitly reported token usage extracted from a CLI's structured
    /// output, if any. `None` means unknown/unsupported (never inferred).
    #[serde(default)]
    pub token_usage: Option<CliTokenUsage>,
}

/// Token usage explicitly reported by a coding-agent CLI.
///
/// All counts are optional: a CLI may report only a subset (partial usage).
/// Counts are never inferred from output size. The process CLI currently
/// recognizes Claude Code's final JSON result event and Codex CLI's cumulative
/// JSONL token-count event. The extracted fields are carried on executor
/// results; registering them in aggregate token-usage statistics still requires
/// lifecycle integration. Other registered CLI agents remain unknown until
/// they have a stable, structured usage format implemented here.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct CliTokenUsage {
    /// Number of input/prompt tokens reported by the CLI.
    #[serde(default)]
    pub input_tokens: Option<u64>,
    /// Number of output/completion tokens reported by the CLI.
    #[serde(default)]
    pub output_tokens: Option<u64>,
    /// Cache-creation input tokens (Claude Code), if reported.
    #[serde(default)]
    pub cache_creation_input_tokens: Option<u64>,
    /// Cache-read input tokens (Claude Code), if reported.
    #[serde(default)]
    pub cache_read_input_tokens: Option<u64>,
    /// Total cost in USD as reported by the CLI (kept as string to avoid
    /// floating-point equality issues).
    #[serde(default)]
    pub total_cost_usd: Option<String>,
    /// Model identifier reported by the CLI, if any.
    #[serde(default)]
    pub model: Option<String>,
    /// Identifier of the structured format that produced this usage.
    #[serde(default)]
    pub format: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStatus {
    Passed,
    Failed,
    Skipped,
    Unavailable,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct VerificationResult {
    pub command: String,
    pub status: VerificationStatus,
    pub exit_code: Option<i32>,
    pub output: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct ChangedFiles {
    pub created: Vec<String>,
    pub modified: Vec<String>,
    pub deleted: Vec<String>,
}

/// Structured execution event emitted for monitoring and UI subscription.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionEvent {
    /// Task has started planning.
    TaskStarted { task_id: String },

    /// Task has entered a new iteration (1-based).
    IterationStarted { task_id: String, iteration: u32 },

    /// A specific component in a graph task has finished an attempt.
    ComponentFinished {
        task_id: String,
        component_id: String,
        success: bool,
        iteration: u32,
    },

    /// Task has completed verification phase.
    VerificationCompleted {
        task_id: String,
        status: VerificationStatus,
        iteration: u32,
    },

    /// Task has completed reviewer evaluation.
    ReviewCompleted {
        task_id: String,
        verdict: ReviewerVerdict,
        iteration: u32,
    },

    /// Task has transitioned to a new state.
    TaskStateTransitioned {
        task_id: String,
        from: TaskState,
        to: TaskState,
        iteration: Option<u32>,
    },

    /// Task has been cancelled.
    TaskCancelled {
        task_id: String,
        iteration: Option<u32>,
    },

    /// Task has failed permanently.
    TaskFailed {
        task_id: String,
        error: String,
        iteration: Option<u32>,
    },

    /// Task has completed successfully.
    TaskCompleted { task_id: String, iteration: u32 },
}

impl ChangedFiles {
    pub fn all_paths(&self) -> Vec<String> {
        let mut paths = self.created.clone();
        paths.extend(self.modified.clone());
        paths.extend(self.deleted.clone());
        paths
    }

    pub fn is_empty(&self) -> bool {
        self.created.is_empty() && self.modified.is_empty() && self.deleted.is_empty()
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewerVerdict {
    Complete,
    NeedsChanges,
    Blocked,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ReviewerResponse {
    pub verdict: ReviewerVerdict,
    pub findings: String,
    pub unresolved_issues: Vec<String>,
    pub suggested_fix: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct IterationRecord {
    pub iteration: u32,
    pub coding_prompt: String,
    pub executor_result: Option<ExecutorResult>,
    pub changed_files: ChangedFiles,
    pub diff_summary: Option<String>,
    pub verification_results: Vec<VerificationResult>,
    pub reviewer_response: Option<ReviewerResponse>,
    pub started_at: String,
    pub completed_at: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ComponentState {
    Pending,
    Ready,
    Running,
    Verifying,
    Completed,
    Failed,
    Retrying,
    Skipped,
}

impl ComponentState {
    pub fn can_transition_to(&self, next: &ComponentState) -> bool {
        match (self, next) {
            (ComponentState::Pending, ComponentState::Ready) => true,
            (ComponentState::Pending, ComponentState::Skipped) => true,
            (ComponentState::Ready, ComponentState::Running) => true,
            (ComponentState::Ready, ComponentState::Skipped) => true,
            (ComponentState::Running, ComponentState::Verifying) => true,
            (ComponentState::Running, ComponentState::Completed) => true,
            (ComponentState::Running, ComponentState::Failed) => true,
            (ComponentState::Verifying, ComponentState::Completed) => true,
            (ComponentState::Verifying, ComponentState::Failed) => true,
            (ComponentState::Failed, ComponentState::Retrying) => true,
            (ComponentState::Retrying, ComponentState::Running) => true,
            _ => false,
        }
    }

    pub fn transition_to(&mut self, next: ComponentState) -> Result<(), String> {
        if self.can_transition_to(&next) {
            *self = next;
            Ok(())
        } else {
            Err(format!(
                "Invalid component state transition from {:?} to {:?}",
                self, next
            ))
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub id: String,
    pub description: String,
    pub state: ComponentState,
    pub depends_on: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub error: Option<String>,
}

impl Component {
    pub fn transition_to(&mut self, next: ComponentState) -> Result<(), String> {
        self.state.transition_to(next)
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TaskGraph {
    pub components: Vec<Component>,
}

impl TaskGraph {
    pub fn from_plan(plan: &Plan) -> Result<Self, String> {
        if plan.steps.is_empty() {
            return Err("Plan must have at least one step to build a TaskGraph".to_string());
        }

        let mut components = Vec::new();
        let mut prev_id: Option<String> = None;

        for (i, step_text) in plan.steps.iter().enumerate() {
            let id = format!("step-{}", i + 1);
            let depends_on = match prev_id {
                Some(ref p) => vec![p.clone()],
                None => Vec::new(),
            };

            components.push(Component {
                id: id.clone(),
                description: step_text.clone(),
                state: ComponentState::Pending,
                depends_on,
                acceptance_criteria: plan.acceptance_criteria.clone(),
                error: None,
            });
            prev_id = Some(id);
        }

        let graph = TaskGraph { components };
        graph.validate()?;
        Ok(graph)
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut ids = std::collections::HashSet::new();
        for c in &self.components {
            if c.id.trim().is_empty() {
                return Err("Component ID cannot be empty".to_string());
            }
            if !ids.insert(&c.id) {
                return Err(format!("Duplicate component ID: '{}'", c.id));
            }
        }

        for c in &self.components {
            for dep in &c.depends_on {
                if dep == &c.id {
                    return Err(format!("Component '{}' cannot depend on itself", c.id));
                }
                if !ids.contains(dep) {
                    return Err(format!(
                        "Component '{}' depends on non-existent ID '{}'",
                        c.id, dep
                    ));
                }
            }
        }

        // Cycle detection using Kahn's algorithm
        let mut in_degree: std::collections::HashMap<&String, usize> =
            std::collections::HashMap::new();
        let mut adj: std::collections::HashMap<&String, Vec<&String>> =
            std::collections::HashMap::new();
        for c in &self.components {
            in_degree.insert(&c.id, 0);
            adj.insert(&c.id, Vec::new());
        }

        for c in &self.components {
            for dep in &c.depends_on {
                adj.get_mut(dep).unwrap().push(&c.id);
                *in_degree.get_mut(&c.id).unwrap() += 1;
            }
        }

        let mut queue = std::collections::VecDeque::new();
        for (id, &deg) in &in_degree {
            if deg == 0 {
                queue.push_back(*id);
            }
        }

        let mut visited = 0;
        while let Some(node) = queue.pop_front() {
            visited += 1;
            if let Some(neighbors) = adj.get(node) {
                for next in neighbors {
                    let deg = in_degree.get_mut(next).unwrap();
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(next);
                    }
                }
            }
        }

        if visited != self.components.len() {
            return Err("Cycle detected in TaskGraph (cycle / circular dependency)".to_string());
        }

        Ok(())
    }

    /// Reset components that were mid-flight when the app stopped so a
    /// recovered graph can be executed again from a clean component state.
    pub fn reset_in_flight(&mut self) {
        for component in &mut self.components {
            if matches!(
                component.state,
                ComponentState::Ready
                    | ComponentState::Running
                    | ComponentState::Verifying
                    | ComponentState::Retrying
            ) {
                component.state = ComponentState::Pending;
                component.error = None;
            }
        }
    }

    /// Prepare a saved graph for an explicit retry. In-flight components are
    /// reset exactly like crash recovery, and failed/skipped components are
    /// requeued so a retry can actually rerun them. Completed components are
    /// retained: their work succeeded and rerunning them would duplicate
    /// changes. Dependency ordering is unchanged, so requeued components only
    /// become eligible once their prerequisites are satisfied.
    pub fn reset_for_retry(&mut self) {
        self.reset_in_flight();
        for component in &mut self.components {
            if matches!(
                component.state,
                ComponentState::Failed | ComponentState::Skipped
            ) {
                component.state = ComponentState::Pending;
                component.error = None;
            }
        }
    }

    pub fn ready_ids(&self) -> Vec<String> {
        self.components
            .iter()
            .filter(|c| c.state == ComponentState::Pending || c.state == ComponentState::Ready)
            .filter(|c| {
                c.depends_on.iter().all(|dep| {
                    self.components
                        .iter()
                        .find(|other| other.id == *dep)
                        .map_or(false, |p| p.state == ComponentState::Completed)
                })
            })
            .map(|c| c.id.clone())
            .collect()
    }

    pub fn blocked_ids(&self) -> Vec<String> {
        self.components
            .iter()
            .filter(|c| c.state == ComponentState::Pending || c.state == ComponentState::Ready)
            .filter(|c| {
                c.depends_on.iter().any(|dep| {
                    self.components
                        .iter()
                        .find(|other| other.id == *dep)
                        .map_or(false, |p| {
                            p.state == ComponentState::Failed || p.state == ComponentState::Skipped
                        })
                })
            })
            .map(|c| c.id.clone())
            .collect()
    }

    pub fn propagate_skips(&mut self) -> Vec<String> {
        let mut newly_skipped = Vec::new();
        let mut changed = true;
        while changed {
            changed = false;
            let to_skip = self.blocked_ids();
            for id in to_skip {
                if let Some(c) = self.components.iter_mut().find(|c| c.id == id) {
                    if c.state != ComponentState::Skipped {
                        c.state = ComponentState::Skipped;
                        newly_skipped.push(id);
                        changed = true;
                    }
                }
            }
        }
        newly_skipped
    }

    pub fn is_complete(&self) -> bool {
        self.components.iter().all(|c| {
            matches!(
                c.state,
                ComponentState::Completed | ComponentState::Skipped | ComponentState::Failed
            )
        })
    }

    pub fn is_blocked(&self) -> bool {
        !self.is_complete() && self.ready_ids().is_empty()
    }

    pub fn has_runnable(&self) -> bool {
        !self.ready_ids().is_empty()
            || self.components.iter().any(|c| {
                matches!(
                    c.state,
                    ComponentState::Running | ComponentState::Verifying | ComponentState::Retrying
                )
            })
    }
}

/// Serializable mirror of `verification::FileSnapshot`.
///
/// `FileSnapshot` itself is not serialized (it lives in the verification
/// module and is intentionally kept out of the serialized contract), so the
/// task baseline is persisted through this small, stable, backward-compatible
/// representation. Older task records without a `task_baseline` field simply
/// deserialize to `None`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct PersistedFileSnapshot {
    pub mtime_secs: u64,
    pub size: u64,
    pub hash: Option<String>,
}

/// A persisted pre-execution filesystem baseline, keyed by normalized relative
/// path. `None` means no baseline was captured (attribution is then skipped
/// rather than fabricated).
pub type PersistedBaseline = std::collections::HashMap<String, PersistedFileSnapshot>;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TaskRecord {
    pub id: String,
    pub user_task: String,
    pub project_root: String,
    pub state: TaskState,
    pub plan: Option<Plan>,
    #[serde(default)]
    pub task_graph: Option<TaskGraph>,
    pub iteration: u32,
    pub max_iterations: u32,
    pub iterations: Vec<IterationRecord>,
    pub created_at: String,
    pub updated_at: String,
    pub error: Option<String>,
    /// Project this task belongs to (None for legacy/single-project tasks).
    #[serde(default)]
    pub project_id: Option<String>,
    /// Provider profile used to create the task.
    #[serde(default)]
    pub provider_profile_id: Option<String>,
    /// Human-readable executor/agent name recorded for the history view.
    #[serde(default)]
    pub agent_name: Option<String>,
    /// Higher values are scheduled first.
    #[serde(default)]
    pub priority: i32,
    /// When the task entered the queue.
    #[serde(default)]
    pub queued_at: Option<String>,
    /// How many times an interrupted task has been recovered/retried.
    #[serde(default)]
    pub recovery_attempts: u32,
    /// Task-level changes: diff between the pre-execution baseline and the
    /// final workspace state. Only populated when a baseline was captured.
    #[serde(default)]
    pub task_level_changes: Option<ChangedFiles>,
    /// Persisted pre-execution filesystem baseline, if one was captured.
    /// Used to restore in-memory baseline state after a restart. Missing or
    /// invalid data degrades gracefully to no attribution.
    #[serde(default)]
    pub task_baseline: Option<PersistedBaseline>,
    /// Resolved non-secret execution configuration captured at task creation.
    /// Retries and recovery prefer this over re-resolving the latest profile.
    /// Credentials are never serialized (ProviderConfig skips api_key).
    #[serde(default)]
    pub execution_config: Option<ProviderConfig>,
    /// Additional context captured when a task is created from a Prompt AI
    /// session. Persisted so queued tasks retain it when planning starts.
    #[serde(default)]
    pub planning_context: Option<String>,
    /// Prompt AI session this task was created from, when applicable.
    #[serde(default)]
    pub source_session_id: Option<String>,
    /// Explicitly selected skill IDs for this task. Persisted so retries and
    /// restarts reproduce the same skill context. Skill content is never
    /// duplicated here; only the stable IDs are stored.
    #[serde(default)]
    pub selected_skill_ids: Vec<String>,
    /// Task-level model override (model ID). Persisted so retries and restarts
    /// use the same model selection. When None, selection falls back to
    /// workspace preferences (default/responsibility/auto).
    #[serde(default)]
    pub task_model_id: Option<String>,
}

impl TaskRecord {
    pub fn transition_to(&mut self, next: TaskState) -> Result<(), String> {
        self.state.validate_transition(&next)?;
        self.state = next;
        self.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PromptDelivery {
    /// Current default: `-p` plus configured args, prompt on stdin.
    #[default]
    PrefixedStdin,
    /// Configured args only, prompt on stdin.
    Stdin,
    /// Configured args plus the prompt as the final argument.
    Argument,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(default)]
pub struct ProviderConfig {
    pub planner_type: String,  // "test" | "cloud_http"
    pub executor_type: String, // "test" | "process_cli"
    pub test_planner_scenario: Option<String>,
    pub test_executor_scenario: Option<String>,
    pub cloud_endpoint: Option<String>,
    pub cloud_model: Option<String>,
    // NEVER send the secret key to the frontend or serialize it to disk.
    // Profile credentials are retrieved from the OS credential store only for
    // authorized provider operations.
    #[serde(skip_serializing, skip_deserializing)]
    pub api_key: Option<String>,
    pub cli_executable: Option<String>,
    pub cli_args: Option<Vec<String>>,
    /// Preset used to populate executable/args (informational).
    pub cli_preset: Option<String>,
    /// How the prompt reaches the CLI.
    pub prompt_delivery: PromptDelivery,
    pub verification_command: Option<String>,
    pub max_iterations: u32,
    pub ignored_dirs: Vec<String>,
}

impl std::fmt::Debug for ProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderConfig")
            .field("planner_type", &self.planner_type)
            .field("executor_type", &self.executor_type)
            .field("test_planner_scenario", &self.test_planner_scenario)
            .field("test_executor_scenario", &self.test_executor_scenario)
            .field("cloud_endpoint", &self.cloud_endpoint)
            .field("cloud_model", &self.cloud_model)
            .field("api_key", &self.api_key.as_ref().map(|_| "[REDACTED]"))
            .field("cli_executable", &self.cli_executable)
            .field("cli_args", &self.cli_args)
            .field("cli_preset", &self.cli_preset)
            .field("prompt_delivery", &self.prompt_delivery)
            .field("verification_command", &self.verification_command)
            .field("max_iterations", &self.max_iterations)
            .field("ignored_dirs", &self.ignored_dirs)
            .finish()
    }
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            planner_type: "test".to_string(),
            executor_type: "test".to_string(),
            test_planner_scenario: None,
            test_executor_scenario: None,
            cloud_endpoint: None,
            cloud_model: None,
            api_key: None,
            cli_executable: None,
            cli_args: None,
            cli_preset: None,
            prompt_delivery: PromptDelivery::default(),
            verification_command: None,
            max_iterations: 3,
            ignored_dirs: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ProviderConfigInput {
    pub planner_type: Option<String>,
    pub executor_type: Option<String>,
    pub test_planner_scenario: Option<String>,
    pub test_executor_scenario: Option<String>,
    pub cloud_endpoint: Option<String>,
    pub cloud_model: Option<String>,
    pub api_key: Option<String>,
    pub cli_executable: Option<String>,
    pub cli_args: Option<Vec<String>>,
    pub cli_preset: Option<String>,
    pub prompt_delivery: Option<PromptDelivery>,
    pub verification_command: Option<String>,
    pub max_iterations: Option<u32>,
    pub ignored_dirs: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct AvailabilityCheck {
    pub ok: bool,
    pub path: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ProviderStatus {
    pub planner_type: String,
    pub executor_type: String,
    pub is_test_mode: bool,
    pub cloud_configured: bool,
    pub cli_configured: bool,
    pub model_name: String,
    pub cli_executable: String,
    pub max_iterations: u32,
    pub has_api_key: bool,
    pub cloud_endpoint: Option<String>,
    pub cli_args: Vec<String>,
    pub cli_preset: Option<String>,
    pub prompt_delivery: PromptDelivery,
    pub verification_command: String,
    pub ignored_dirs: Vec<String>,
}

// ---------------------------------------------------------------------------
// Workspace / multi-project models
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecord {
    pub id: String,
    pub name: String,
    /// Canonical project root.
    pub root: String,
    /// Default provider profile for tasks created in this project.
    #[serde(default)]
    pub provider_profile_id: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub last_opened_at: String,
    #[serde(default)]
    pub pinned: bool,
    /// Whether the project registration has been archived. Archived projects
    /// are retained for history but must not receive new tasks.
    #[serde(default)]
    pub archived: bool,
    /// Optional human-readable description.
    #[serde(default)]
    pub description: Option<String>,
    /// Freeform tags for filtering and organization.
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProviderProfile {
    pub id: String,
    pub name: String,
    /// None = available to every project; Some(id) = project-scoped.
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub is_default: bool,
    pub config: ProviderConfig,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(default)]
pub struct WorkspaceSettings {
    /// Maximum number of tasks actively planning/executing at once.
    pub max_concurrent_tasks: u32,
    /// Maximum number of concurrent interactive sessions.
    pub max_concurrent_sessions: u32,
    /// When true, queued tasks are not auto-started.
    pub scheduler_paused: bool,
    /// Automatically retry interrupted tasks that have a saved plan.
    pub auto_retry_interrupted: bool,
    /// Maximum recovery attempts per task.
    pub max_recovery_attempts: u32,
}

impl Default for WorkspaceSettings {
    fn default() -> Self {
        Self {
            max_concurrent_tasks: 3,
            max_concurrent_sessions: 4,
            scheduler_paused: false,
            auto_retry_interrupted: false,
            max_recovery_attempts: 1,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    /// Direct assistant conversation (planner model, no code execution loop).
    Prompt,
    /// Independent CLI coding-agent conversation.
    Cli,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SessionMessage {
    pub id: String,
    /// "user" | "assistant" | "system" | "tool"
    pub role: String,
    pub content: String,
    pub created_at: String,
    #[serde(default)]
    pub duration_ms: Option<u64>,
    #[serde(default)]
    pub exit_code: Option<i32>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionRecord {
    pub id: String,
    pub title: String,
    pub kind: SessionKind,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub project_root: Option<String>,
    #[serde(default)]
    pub provider_profile_id: Option<String>,
    #[serde(default)]
    pub cli_preset_id: Option<String>,
    #[serde(default)]
    pub cli_executable: Option<String>,
    #[serde(default)]
    pub cli_args: Vec<String>,
    #[serde(default)]
    pub prompt_delivery: PromptDelivery,
    #[serde(default)]
    pub messages: Vec<SessionMessage>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    /// Runtime-only: true while a request is in flight.
    #[serde(default)]
    pub busy: bool,
    #[serde(default)]
    pub error: Option<String>,
    /// True when the conversation has been archived by the user. Archived
    /// conversations are excluded from default listings and cannot receive new
    /// messages until restored.
    #[serde(default)]
    pub archived: bool,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct StartTaskOptions {
    pub project_id: Option<String>,
    pub provider_profile_id: Option<String>,
    /// Optional coding-agent preset id selected by the user. When present it
    /// resolves the executor executable, args, and prompt delivery.
    #[serde(default)]
    pub cli_preset_id: Option<String>,
    /// Select a different executor/agent than the profile's default.
    pub cli_executable: Option<String>,
    pub cli_args: Option<Vec<String>>,
    pub prompt_delivery: Option<PromptDelivery>,
    pub priority: Option<i32>,
    /// Explicitly selected skill IDs. Empty/None means no skills are applied.
    /// Skills are never injected implicitly; only user-selected ones reach prompts.
    #[serde(default)]
    pub selected_skill_ids: Vec<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
pub struct CreateSessionOptions {
    pub kind: Option<SessionKind>,
    pub title: Option<String>,
    pub project_id: Option<String>,
    pub provider_profile_id: Option<String>,
    pub cli_preset_id: Option<String>,
    pub cli_executable: Option<String>,
    pub cli_args: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct CliAgentPreset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub default_executable: String,
    pub default_args: Vec<String>,
    pub prompt_delivery: PromptDelivery,
    /// Whether the preset executable is currently resolvable on this machine.
    pub available: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct LocalModelPreset {
    pub id: String,
    pub name: String,
    pub description: String,
    pub endpoint: String,
    pub suggested_models: Vec<String>,
    pub requires_api_key: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WorkspaceSnapshot {
    pub projects: Vec<ProjectRecord>,
    pub provider_profiles: Vec<ProviderProfile>,
    pub settings: WorkspaceSettings,
    /// Id of the project the user has selected as active, if any.
    #[serde(default)]
    pub active_project_id: Option<String>,
    /// Non-terminal task IDs, newest first.
    pub active_task_ids: Vec<String>,
    /// Queued task IDs in scheduling order.
    pub queued_task_ids: Vec<String>,
}

// ---------------------------------------------------------------------------
// Notifications, activity, history and reporting
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationSeverity {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct NotificationRecord {
    pub id: String,
    pub created_at: String,
    pub category: String,
    pub severity: String,
    pub title: String,
    pub message: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub read: bool,
    #[serde(default)]
    pub dedup_key: Option<String>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ActivityRecord {
    pub id: String,
    pub timestamp: String,
    pub event_type: String,
    pub summary: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub iteration: Option<u32>,
    #[serde(default)]
    pub outcome: String,
    #[serde(default)]
    pub dedup_key: Option<String>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct NotificationFilter {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub severity: Option<String>,
    #[serde(default)]
    pub unread_only: bool,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ActivityFilter {
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub event_type: Option<String>,
    #[serde(default)]
    pub outcome: Option<String>,
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub until: Option<String>,
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub offset: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct TaskReport {
    #[serde(default)]
    pub task_id: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub project_name: Option<String>,
    #[serde(default)]
    pub original_request: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
    #[serde(default)]
    pub elapsed_ms: Option<u64>,
    #[serde(default)]
    pub planning_summary: Option<String>,
    #[serde(default)]
    pub agents: Vec<String>,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub execution_attempts: u32,
    #[serde(default)]
    pub verification_results: Vec<VerificationResult>,
    #[serde(default)]
    pub review_results: Vec<ReviewerResponse>,
    #[serde(default)]
    pub corrections: u32,
    #[serde(default)]
    pub retries: u32,
    #[serde(default)]
    pub files_changed: ChangedFiles,
    #[serde(default)]
    pub tests_and_commands: Vec<String>,
    #[serde(default)]
    pub final_outcome: String,
    #[serde(default)]
    pub remaining_issues: Vec<String>,
    #[serde(default)]
    pub token_input: Option<u64>,
    #[serde(default)]
    pub token_output: Option<u64>,
    #[serde(default)]
    pub token_total: Option<u64>,
    #[serde(default)]
    pub known_facts: Vec<String>,
    #[serde(default)]
    pub unavailable: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ProjectReport {
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub total_tasks: usize,
    #[serde(default)]
    pub completed_tasks: usize,
    #[serde(default)]
    pub failed_tasks: usize,
    #[serde(default)]
    pub cancelled_tasks: usize,
    #[serde(default)]
    pub queued_tasks: usize,
    #[serde(default)]
    pub active_tasks: usize,
    #[serde(default)]
    pub completion_rate: f64,
    #[serde(default)]
    pub execution_attempts: u32,
    #[serde(default)]
    pub retries: u32,
    #[serde(default)]
    pub verification_passed: usize,
    #[serde(default)]
    pub verification_failed: usize,
    #[serde(default)]
    pub token_input: Option<u64>,
    #[serde(default)]
    pub token_output: Option<u64>,
    #[serde(default)]
    pub token_total: Option<u64>,
    #[serde(default)]
    pub recent_failures: Vec<String>,
    #[serde(default)]
    pub recent_tasks: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ProjectSummary {
    #[serde(default)]
    pub project_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub root: String,
    #[serde(default)]
    pub archived: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub health: String,
    #[serde(default)]
    pub directory_available: bool,
    #[serde(default)]
    pub active_task_count: usize,
    #[serde(default)]
    pub queued_task_count: usize,
    #[serde(default)]
    pub completed_task_count: usize,
    #[serde(default)]
    pub failed_task_count: usize,
    #[serde(default)]
    pub cancelled_task_count: usize,
    #[serde(default)]
    pub total_task_count: usize,
    #[serde(default)]
    pub latest_task_status: Option<String>,
    #[serde(default)]
    pub latest_task_id: Option<String>,
    #[serde(default)]
    pub last_activity_at: Option<String>,
    #[serde(default)]
    pub recent_tasks: Vec<TaskRecord>,
    #[serde(default)]
    pub token_usage: crate::token_usage::TokenUsageStats,
}
