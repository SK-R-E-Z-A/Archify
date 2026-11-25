import {
  lazy,
  Suspense,
  useState,
  useCallback,
  useEffect,
  useRef,
} from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import "./App.css";
import InstructionManager from "./features/InstructionManager";
import SkillManager from "./features/SkillManager";
import { ExecutionMonitor } from "./components/ExecutionMonitor";
import type { ExecutionEvent } from "./components/EventTimeline";
import { NotificationPanel } from "./components/NotificationPanel";
import { ActivityFeed } from "./components/ActivityFeed";
import { TaskTimeline } from "./components/TaskTimeline";
import { TaskReportModal } from "./components/TaskReportModal";
import { ProjectReportModal } from "./components/ProjectReportModal";

// Kept out of the initial bundle: xterm is only needed once the SHELL tab is
// opened, and the workspace itself mounts lazily.
const TerminalWorkspace = lazy(
  () => import("./features/terminal/TerminalWorkspace"),
);

type Message = {
  role: "user" | "assistant";
  content: string;
};

type FileEntry = {
  name: string;
  path: string;
  is_dir: boolean;
};

type AvailabilityCheck = {
  ok: boolean;
  path: string | null;
  error: string | null;
};

type TaskState =
  | "idle"
  | "queued"
  | "planning"
  | "awaiting_approval"
  | "executing"
  | "verifying"
  | "reviewing"
  | "retrying"
  | "completed"
  | "failed"
  | "cancelled";

type Plan = {
  summary: string;
  steps: string[];
  acceptance_criteria: string[];
};

type ReviewerVerdict = "complete" | "needs_changes" | "blocked";

type ReviewerResponse = {
  verdict: ReviewerVerdict;
  findings: string;
  unresolved_issues: string[];
  suggested_fix: string | null;
};

type ExecutorResult = {
  exit_code: number | null;
  stdout: string;
  stderr: string;
  timed_out: boolean;
  duration_ms: number;
};

type VerificationStatus = "passed" | "failed" | "skipped" | "unavailable";

type VerificationResult = {
  command: string;
  status: VerificationStatus;
  exit_code: number | null;
  output: string;
};

type ChangedFiles = {
  created: string[];
  modified: string[];
  deleted: string[];
};

type IterationRecord = {
  iteration: number;
  coding_prompt: string;
  executor_result: ExecutorResult | null;
  changed_files: ChangedFiles;
  diff_summary: string | null;
  verification_results: VerificationResult[];
  reviewer_response: ReviewerResponse | null;
  started_at: string;
  completed_at: string | null;
};

type TaskRecord = {
  id: string;
  user_task: string;
  project_root: string;
  state: TaskState;
  plan: Plan | null;
  task_graph: TaskGraph | null;
  iteration: number;
  max_iterations: number;
  iterations: IterationRecord[];
  created_at: string;
  updated_at: string;
  error: string | null;
  project_id: string | null;
  provider_profile_id: string | null;
  agent_name: string | null;
  priority: number;
  queued_at: string | null;
  recovery_attempts: number;
};

type NotificationRecord = {
  id: string;
  created_at: string;
  category: string;
  severity: string;
  title: string;
  message: string;
  project_id: string | null;
  task_id: string | null;
  session_id: string | null;
  agent_id: string | null;
  read: boolean;
  dedup_key: string | null;
  action: string | null;
  metadata: Record<string, unknown> | null;
};

type TaskReport = {
  task_id: string;
  project_id: string | null;
  project_name: string | null;
  original_request: string;
  status: string;
  created_at: string;
  updated_at: string;
  elapsed_ms: number | null;
  planning_summary: string | null;
  agents: string[];
  models: string[];
  execution_attempts: number;
  verification_results: unknown[];
  review_results: unknown[];
  corrections: number;
  retries: number;
  files_changed: { created: string[]; modified: string[]; deleted: string[] };
  tests_and_commands: string[];
  final_outcome: string;
  remaining_issues: string[];
  token_input: number | null;
  token_output: number | null;
  token_total: number | null;
  known_facts: string[];
  unavailable: string[];
};

type ProjectReport = {
  project_id: string;
  name: string;
  total_tasks: number;
  completed_tasks: number;
  failed_tasks: number;
  cancelled_tasks: number;
  queued_tasks: number;
  active_tasks: number;
  completion_rate: number;
  execution_attempts: number;
  retries: number;
  verification_passed: number;
  verification_failed: number;
  token_input: number | null;
  token_output: number | null;
  token_total: number | null;
  recent_failures: string[];
  recent_tasks: string[];
};

type ComponentState =
  | "pending"
  | "ready"
  | "running"
  | "verifying"
  | "completed"
  | "failed"
  | "retrying"
  | "skipped";

type Component = {
  id: string;
  description: string;
  state: ComponentState;
  depends_on: string[];
  acceptance_criteria: string[];
  error: string | null;
};

type TaskGraph = {
  components: Component[];
};

type ProviderStatus = {
  planner_type: string;
  executor_type: string;
  is_test_mode: boolean;
  cloud_configured: boolean;
  cli_configured: boolean;
  model_name: string;
  cli_executable: string;
  max_iterations: number;
  has_api_key: boolean;
  cloud_endpoint: string | null;
  cli_args: string[];
  cli_preset: string | null;
  prompt_delivery: PromptDelivery;
  verification_command: string;
  ignored_dirs: string[];
};

type PromptDelivery = "prefixed_stdin" | "stdin" | "argument";

type ProviderConfigInput = {
  planner_type?: string;
  executor_type?: string;
  test_planner_scenario?: string | null;
  test_executor_scenario?: string | null;
  cloud_endpoint?: string;
  cloud_model?: string;
  api_key?: string | null;
  cli_executable?: string;
  cli_args?: string[];
  cli_preset?: string | null;
  prompt_delivery?: PromptDelivery;
  verification_command?: string;
  max_iterations?: number;
  ignored_dirs?: string[];
};

type ProjectRecord = {
  id: string;
  name: string;
  root: string;
  provider_profile_id: string | null;
  created_at: string;
  last_opened_at: string;
  pinned: boolean;
  archived?: boolean;
};

type ProviderProfile = {
  id: string;
  name: string;
  project_id: string | null;
  is_default: boolean;
  config: {
    planner_type: string;
    executor_type: string;
    test_planner_scenario: string | null;
    test_executor_scenario: string | null;
    cloud_endpoint: string | null;
    cloud_model: string | null;
    cli_executable: string | null;
    cli_args: string[] | null;
    cli_preset: string | null;
    prompt_delivery: PromptDelivery;
    verification_command: string | null;
    max_iterations: number;
    ignored_dirs: string[];
  };
  created_at: string;
  updated_at: string;
};

type WorkspaceSettings = {
  max_concurrent_tasks: number;
  max_concurrent_sessions: number;
  scheduler_paused: boolean;
  auto_retry_interrupted: boolean;
  max_recovery_attempts: number;
};

type WorkspaceSnapshot = {
  projects: ProjectRecord[];
  provider_profiles: ProviderProfile[];
  settings: WorkspaceSettings;
  active_task_ids: string[];
  queued_task_ids: string[];
};

// ---------------------------------------------------------------------------
// Token Usage Types
// ---------------------------------------------------------------------------

type TokenUsage = {
  input_tokens: number | null;
  output_tokens: number | null;
  total_tokens: number | null;
  tokens_unknown: boolean;
  timestamp: string;
  duration_ms: number | null;
};

type UsageContext = {
  provider_id: string | null;
  provider_name: string | null;
  model_id: string | null;
  model_name: string | null;
  project_id: string | null;
  project_name: string | null;
  task_id: string | null;
  session_id: string | null;
  responsibility: string | null;
};

type TokenUsageRecord = {
  id: string;
  usage: TokenUsage;
  context: UsageContext;
  operation_type: string;
  is_retry: boolean;
  count_towards_totals: boolean;
  created_at: string;
};

type UsageBucketStats = {
  input_tokens: number;
  output_tokens: number;
  total_tokens: number;
  request_count: number;
};

type TokenUsageStats = {
  total_input_tokens: number;
  total_output_tokens: number;
  total_tokens: number;
  request_count: number;
  unknown_request_count: number;
  by_provider: Record<string, UsageBucketStats>;
  by_model: Record<string, UsageBucketStats>;
  by_project: Record<string, UsageBucketStats>;
  by_task: Record<string, UsageBucketStats & { task_id?: string }>;
  by_responsibility: Record<string, UsageBucketStats>;
  by_date: Record<string, UsageBucketStats>;
};

// Model selection (backend-owned; frontend reflects returned values)
type AiResponsibilityId =
  | "planning"
  | "prompt_generation"
  | "code_review"
  | "coding"
  | "chat"
  | "default";

type ModelCapability =
  | "planning"
  | "prompt_generation"
  | "code_review"
  | "code_generation"
  | "chat"
  | "tool_execution"
  | "all";

type ModelInfo = {
  id: string;
  name: string;
  description?: string;
  provider_id: string;
  provider_name: string;
  context_window?: number | null;
  max_output_tokens?: number | null;
  available?: boolean;
  deprecated?: boolean;
  capabilities?: ModelCapability[];
};

type ResponsibilityModelPreference = {
  model_id?: string | null;
  auto_select: boolean;
};

type ModelPreferences = {
  default_model_id?: string | null;
  responsibility_preferences: Partial<Record<AiResponsibilityId, ResponsibilityModelPreference>>;
  manual_overrides: Record<string, string>;
  auto_selection_enabled: boolean;
};

type SelectionStrategy =
  | "manual_override"
  | "responsibility_preference"
  | "default_model"
  | "auto_selected";

type ModelSelection = {
  model_id: string;
  model_name: string;
  provider_id: string;
  provider_name: string;
  responsibility: AiResponsibilityId;
  strategy: SelectionStrategy;
  auto_selected: boolean;
};

const RESPONSIBILITY_LABELS: Record<AiResponsibilityId, string> = {
  planning: "Planning & Reasoning",
  prompt_generation: "Prompt Generation",
  code_review: "Code Review & Analysis",
  coding: "Coding & Execution",
  chat: "Chat & Conversation",
  default: "Default",
};

const ALL_RESPONSIBILITIES: AiResponsibilityId[] = [
  "planning",
  "prompt_generation",
  "code_review",
  "coding",
  "chat",
  "default",
];
// Session and preset types (backend-shaped)
type SessionKind = "prompt" | "cli";

type SessionMessage = {
  id: string;
  role: "user" | "assistant" | "system" | "tool";
  content: string;
  created_at: string;
  duration_ms: number | null;
  exit_code: number | null;
  error: string | null;
};

type SessionRecord = {
  id: string;
  title: string;
  kind: SessionKind;
  project_id: string | null;
  project_root: string | null;
  provider_profile_id: string | null;
  cli_preset_id: string | null;
  cli_executable: string | null;
  cli_args: string[];
  prompt_delivery: PromptDelivery;
  messages: SessionMessage[];
  created_at: string;
  updated_at: string;
  busy: boolean;
  error: string | null;
};

type CliAgentPreset = {
  id: string;
  name: string;
  description: string;
  default_executable: string;
  default_args: string[];
  prompt_delivery: PromptDelivery;
  available: boolean;
};

type LocalModelPreset = {
  id: string;
  name: string;
  description: string;
  endpoint: string;
  suggested_models: string[];
  requires_api_key: boolean;
};


// Coding-agent registry (backend shape; read-only display/registration in frontend)
type AgentAvailability = "installed" | "unavailable" | "unsupported" | "detection_error";
type DetectionMethod = "auto" | "manual";
type AgentKind = "known" | "custom";

type RegisteredCodingAgent = {
  id: string;
  agent_type: string;
  name: string;
  kind: AgentKind;
  executable: string;
  resolved_path?: string | null;
  version?: string | null;
  detection_method: DetectionMethod;
  availability: AgentAvailability;
  status_message?: string | null;
  default_args: string[];
  prompt_delivery: PromptDelivery;
  version_args?: string[];
  created_at?: string;
  updated_at?: string;
};

type ManualAgentInput = {
  id?: string | null;
  name?: string | null;
  executable?: string | null;
  default_args?: string[] | null;
  prompt_delivery?: PromptDelivery | null;
};

type DetectionReport = {
  agents: RegisteredCodingAgent[];
  warnings: string[];
};



type PendingAction = "start" | "approve" | "reject" | "cancel";

type TaskProblem = {
  title: string;
  detail: string;
};

const TASK_STATE_LABELS: Record<TaskState, string> = {
  idle: "Idle",
  queued: "Queued",
  planning: "Planning",
  awaiting_approval: "Awaiting approval",
  executing: "Executing",
  verifying: "Verifying",
  reviewing: "Reviewing",
  retrying: "Retrying",
  completed: "Completed",
  failed: "Failed",
  cancelled: "Cancelled",
};

const TERMINAL_TASK_STATES: TaskState[] = ["completed", "failed", "cancelled"];

const COMPONENT_STATE_LABELS: Record<ComponentState, string> = {
  pending: "Pending",
  ready: "Ready",
  running: "Running",
  verifying: "Verifying",
  completed: "Completed",
  failed: "Failed",
  retrying: "Retrying",
  skipped: "Skipped",
};

const VERIFICATION_STATUS_LABELS: Record<VerificationStatus, string> = {
  passed: "Passed",
  failed: "Failed",
  skipped: "Skipped",
  unavailable: "Unavailable",
};

const REVIEW_VERDICT_LABELS: Record<ReviewerVerdict, string> = {
  complete: "Complete",
  needs_changes: "Needs changes",
  blocked: "Blocked",
};

function describeError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Unknown error";
}

/** True for loopback endpoints, which do not require an API key. */
function isLocalEndpointUrl(value: string): boolean {
  try {
    const url = new URL(value.trim());
    const host = url.hostname.replace(/^\[|\]$/g, "").toLowerCase();
    return host === "localhost" || host === "127.0.0.1" || host === "::1";
  } catch {
    return false;
  }
}

function isTerminalTask(task: TaskRecord) {
  return TERMINAL_TASK_STATES.includes(task.state);
}

function isBlockedFailure(task: TaskRecord) {
  return (
    task.state === "failed" &&
    ((task.error ?? "").startsWith("Reviewer reported unresolvable blocker") ||
      (task.error ?? "").startsWith("Graph is blocked"))
  );
}

function taskStateClass(state: TaskState) {
  switch (state) {
    case "completed":
      return "state-completed";
    case "failed":
      return "state-failed";
    case "cancelled":
      return "state-cancelled";
    case "awaiting_approval":
      return "state-awaiting";
    default:
      return "state-active";
  }
}

function componentStateClass(state: ComponentState) {
  switch (state) {
    case "completed":
      return "component-completed";
    case "failed":
      return "component-failed";
    case "skipped":
      return "component-skipped";
    case "running":
    case "verifying":
    case "retrying":
      return "component-active";
    default:
      return "component-idle";
  }
}

function verificationStatusClass(status: VerificationStatus) {
  return `verification-${status}`;
}

function verdictClass(verdict: ReviewerVerdict | null) {
  if (verdict === "complete") return "verdict-complete";
  if (verdict === "needs_changes") return "verdict-needs-changes";
  if (verdict === "blocked") return "verdict-blocked";
  return "verdict-none";
}

function changedFilesAll(files: ChangedFiles | null | undefined): string[] {
  if (!files) return [];
  return [...files.created, ...files.modified, ...files.deleted];
}

function changedFilesCount(files: ChangedFiles | null | undefined): number {
  return changedFilesAll(files).length;
}

function collectTaskChangedFiles(task: TaskRecord): ChangedFiles {
  const created = new Set<string>();
  const modified = new Set<string>();
  const deleted = new Set<string>();
  for (const iteration of task.iterations) {
    iteration.changed_files.created.forEach((path) => created.add(path));
    iteration.changed_files.modified.forEach((path) => modified.add(path));
    iteration.changed_files.deleted.forEach((path) => deleted.add(path));
  }
  return {
    created: [...created].sort(),
    modified: [...modified].sort(),
    deleted: [...deleted].sort(),
  };
}

function verificationBlocksCompletion(iteration: IterationRecord) {
  return iteration.verification_results.some(
    (verification) => verification.status !== "passed",
  );
}

function effectiveVerdict(iteration: IterationRecord): ReviewerVerdict | null {
  if (verificationBlocksCompletion(iteration)) return "needs_changes";
  return iteration.reviewer_response?.verdict ?? null;
}

function graphProgress(graph: TaskGraph | null) {
  const components = graph?.components ?? [];
  const completed = components.filter((c) => c.state === "completed").length;
  const skipped = components.filter((c) => c.state === "skipped").length;
  const failed = components.filter((c) => c.state === "failed").length;
  const done = completed;
  const total = components.length;
  return {
    total,
    completed,
    skipped,
    failed,
    done,
    percent: total === 0 ? 0 : Math.round((done / total) * 100),
  };
}

function dependencyLabel(graph: TaskGraph, id: string) {
  const index = graph.components.findIndex((component) => component.id === id);
  return index >= 0 ? `step ${index + 1}` : id;
}

function taskProgressLabel(task: TaskRecord) {
  if (task.state === "awaiting_approval") return "awaiting approval";
  if (task.task_graph) {
    const progress = graphProgress(task.task_graph);
    return `${progress.done}/${progress.total} steps complete`;
  }
  return `iteration ${task.iteration}/${task.max_iterations}`;
}

function formatDuration(durationMs: number) {
  if (durationMs < 1000) return `${durationMs} ms`;
  return `${(durationMs / 1000).toFixed(1)} s`;
}

function formatTimestamp(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function shortId(id: string) {
  return id.slice(0, 8);
}

function projectName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

function parseList(value: string): string[] {
  return value
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function taskResultTitle(task: TaskRecord) {
  if (task.state === "completed") {
    return task.task_graph
      ? "Task completed"
      : `Task completed on iteration ${task.iteration}`;
  }
  if (task.state === "cancelled") return "Task cancelled";

  const error = task.error ?? "";
  if (error.startsWith("Graph is blocked")) return "Execution blocked";
  if (error.startsWith("Reviewer reported unresolvable blocker")) {
    return "Blocked by the reviewer";
  }
  if (error.startsWith("Iteration limit reached")) return "Iteration limit reached";
  if (error.startsWith("Repeated failure")) return "Repeated failure detected";
  return "Task failed";
}

function taskResultClass(task: TaskRecord) {
  if (task.state === "completed") return "result-success";
  if (task.state === "cancelled") return "result-neutral";
  if (isBlockedFailure(task)) return "result-blocked";
  return "result-error";
}

function taskNextStep(task: TaskRecord) {
  if (task.state === "completed") {
    return "Review the changed files below, then start a new task when you are ready for the next change.";
  }
  if (task.state === "cancelled") {
    if ((task.error ?? "").includes("interrupted by application restart")) {
      return "The app restarted while this task was running, so it was not resumed. Review the workspace, then start a new task.";
    }
    return "The task was stopped before completion. Start a new task when you are ready.";
  }
  const error = task.error ?? "";
  if (error.startsWith("Reviewer reported unresolvable blocker")) {
    return "The reviewer found a blocker that another iteration cannot resolve. Clarify the task or reduce its scope, then start a new task.";
  }
  if (error.startsWith("Graph is blocked")) {
    return "No runnable steps remain because a prerequisite failed or was skipped. Review the failed step below, then start a new task.";
  }
  if (error.startsWith("Iteration limit reached")) {
    return "Raise Max Iterations in Provider Configuration, or narrow the task, then start a new task.";
  }
  if (error.startsWith("Repeated failure")) {
    return "The same issue persisted across iterations. Review the unresolved issues below and clarify the task before retrying.";
  }
  return "Review the error above, fix any environment or provider issue, then start a new task.";
}

function collectTaskProblems(task: TaskRecord): TaskProblem[] {
  const problems: TaskProblem[] = [];

  if (task.error) {
    problems.push({ title: "Task error", detail: task.error });
  }

  task.task_graph?.components.forEach((component) => {
    if (component.error) {
      problems.push({
        title: `${component.id} failed`,
        detail: component.error,
      });
    }
  });

  task.iterations.forEach((iteration) => {
    iteration.verification_results
      .filter((verification) => verification.status !== "passed")
      .forEach((verification) => {
        const exit =
          verification.exit_code === null
            ? ""
            : ` (exit ${verification.exit_code})`;
        problems.push({
          title: `Iteration ${iteration.iteration}: verification ${VERIFICATION_STATUS_LABELS[verification.status].toLowerCase()}`,
          detail: `${verification.command}${exit}${verification.output ? `\n${verification.output}` : ""}`,
        });
      });

    const review = iteration.reviewer_response;
    if (review) {
      if (review.verdict === "blocked") {
        problems.push({
          title: `Iteration ${iteration.iteration}: reviewer blocked the task`,
          detail: review.findings,
        });
      }
      review.unresolved_issues.forEach((issue) => {
        problems.push({
          title: `Iteration ${iteration.iteration}: unresolved issue`,
          detail: issue,
        });
      });
    }

    const executor = iteration.executor_result;
    if (
      executor &&
      (executor.timed_out ||
        (executor.exit_code !== null && executor.exit_code !== 0))
    ) {
      problems.push({
        title: `Iteration ${iteration.iteration}: executor ${
          executor.timed_out
            ? "timed out"
            : `exited with code ${executor.exit_code}`
        }`,
        detail:
          executor.stderr.trim() ||
          executor.stdout.trim() ||
          "No executor output was captured.",
      });
    }
  });

  return problems;
}

function latestExecutorOutput(task: TaskRecord | null): string {
  const iteration = task?.iterations[task.iterations.length - 1];
  const executor = iteration?.executor_result;
  if (!executor) return "";

  const sections: string[] = [
    `$ executor (exit ${executor.exit_code ?? "n/a"} · ${formatDuration(executor.duration_ms)}${
      executor.timed_out ? " · timed out" : ""
    })`,
  ];
  if (executor.stdout.trim()) sections.push(executor.stdout.trimEnd());
  if (executor.stderr.trim()) sections.push(`[stderr]\n${executor.stderr.trimEnd()}`);
  return sections.join("\n\n");
}

function getFileBadge(filename: string) {
  const ext = filename.split(".").pop()?.toLowerCase();
  switch (ext) {
    case "ts":
    case "tsx":
      return <span className="file-badge ts">TS</span>;
    case "js":
    case "jsx":
      return <span className="file-badge js">JS</span>;
    case "css":
    case "scss":
    case "sass":
      return <span className="file-badge css">CSS</span>;
    case "json":
      return <span className="file-badge json">JSON</span>;
    case "rs":
      return <span className="file-badge rs">RS</span>;
    case "md":
      return <span className="file-badge md">MD</span>;
    case "html":
      return <span className="file-badge html">HTML</span>;
    default:
      return <span className="file-badge default">◇</span>;
  }
}


function formatTokenCount(value: number | null | undefined): string {
  if (value == null) return "unknown";
  return value.toLocaleString();
}

function usageBucketRows(
  buckets: Record<string, UsageBucketStats>,
): Array<[string, UsageBucketStats]> {
  return Object.entries(buckets).sort((a, b) => b[1].total_tokens - a[1].total_tokens);
}

function UsageSection({
  title,
  buckets,
  labelFor,
  emptyText,
}: {
  title: string;
  buckets: Record<string, UsageBucketStats>;
  labelFor?: (key: string, bucket: UsageBucketStats) => string;
  emptyText: string;
}) {
  const rows = usageBucketRows(buckets);
  return (
    <section className="usage-section">
      <h3>{title}</h3>
      {rows.length === 0 ? (
        <p className="muted-note">{emptyText}</p>
      ) : (
        <div className="usage-table-wrap">
          <table className="usage-table">
            <thead>
              <tr>
                <th>Name</th>
                <th>Input</th>
                <th>Output</th>
                <th>Total</th>
                <th>Requests</th>
              </tr>
            </thead>
            <tbody>
              {rows.map(([key, bucket]) => (
                <tr key={key}>
                  <td title={key}>{labelFor ? labelFor(key, bucket) : key}</td>
                  <td>{bucket.input_tokens.toLocaleString()}</td>
                  <td>{bucket.output_tokens.toLocaleString()}</td>
                  <td><strong>{bucket.total_tokens.toLocaleString()}</strong></td>
                  <td>{bucket.request_count}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </section>
  );
}

function TokenUsageModal({
  isOpen,
  onClose,
  projects,
}: {
  isOpen: boolean;
  onClose: () => void;
  projects: ProjectRecord[];
}) {
  const [stats, setStats] = useState<TokenUsageStats | null>(null);
  const [records, setRecords] = useState<TokenUsageRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [clearing, setClearing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [filterProject, setFilterProject] = useState("");
  const [filterModel, setFilterModel] = useState("");
  const [filterProvider, setFilterProvider] = useState("");
  const loadSeq = useRef(0);

  const load = useCallback(async (overrides?: { projectId?: string; modelId?: string; providerId?: string }) => {
    const seq = ++loadSeq.current;
    setLoading(true);
    setError(null);
    const projectId = overrides?.projectId ?? filterProject;
    const modelId = overrides?.modelId ?? filterModel;
    const providerId = overrides?.providerId ?? filterProvider;
    try {
      const [s, r] = await Promise.all([
        invoke<TokenUsageStats>("get_token_usage_stats"),
        invoke<TokenUsageRecord[]>("get_token_usage_records", {
          taskId: null,
          providerId: providerId || null,
          modelId: modelId || null,
          projectId: projectId || null,
          startTime: null,
          endTime: null,
        }),
      ]);
      if (seq !== loadSeq.current) return; // stale response
      setStats(s);
      setRecords(r);
    } catch (e) {
      if (seq !== loadSeq.current) return;
      setError(describeError(e));
    } finally {
      if (seq === loadSeq.current) setLoading(false);
    }
  }, [filterProject, filterModel, filterProvider]);

  useEffect(() => {
    if (!isOpen) return;
    setStatus(null);
    void load();
  }, [isOpen, load]);

  useEffect(() => {
    if (!isOpen) return;
    void load({ projectId: filterProject, modelId: filterModel, providerId: filterProvider });
  }, [isOpen, filterProject, filterModel, filterProvider, load]);

  const handleClear = async () => {
    if (clearing) return;
    if (!window.confirm("Clear all token usage records? This cannot be undone.")) return;
    setClearing(true);
    setError(null);
    setStatus(null);
    try {
      await invoke("clear_token_usage");
      await load();
      setStatus("Token usage history cleared (backend-confirmed).");
    } catch (e) {
      setError(describeError(e));
    } finally {
      setClearing(false);
    }
  };

  if (!isOpen) return null;

  const projectName = (id: string | null | undefined) => {
    if (!id) return "Unknown project";
    return projects.find((p) => p.id === id)?.name ?? id;
  };

  return (
    <div className="config-modal-backdrop" onClick={onClose}>
      <div
        className="config-modal token-usage-modal"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="config-modal-header">
          <h2>Token Usage & Analytics</h2>
          <div className="history-header-actions">
            <button className="secondary-button" onClick={() => void load()} disabled={loading || clearing}>
              {loading ? "Loading..." : "Refresh"}
            </button>
            <button className="danger-button" onClick={() => void handleClear()} disabled={loading || clearing}>
              {clearing ? "Clearing..." : "Clear Usage"}
            </button>
            <button className="icon-button small" onClick={onClose} title="Close">
              ×
            </button>
          </div>
        </div>

        <div className="config-modal-body token-usage-body">
          <div className="config-hint">
            Totals and breakdowns come from backend-reported usage records. Missing provider/model/project/task
            values are shown as unknown/missing and are not counted as zero.
          </div>

          {error && <div className="inline-error" role="alert">{error}</div>}
          {status && <div className="inline-success" role="status">{status}</div>}
          {loading && !stats && <p className="muted-note">Loading token usage from backend…</p>}

          {stats && !loading && stats.request_count === 0 && records.length === 0 && (
            <p className="muted-note">No token usage recorded yet. Usage appears here when backend-reported token records are available.</p>
          )}

          {stats && (
            <>
              <div className="usage-stats-grid">
                <div className="usage-stat-card">
                  <div className="usage-stat-label">Total Tokens</div>
                  <div className="usage-stat-value">{stats.total_tokens.toLocaleString()}</div>
                </div>
                <div className="usage-stat-card">
                  <div className="usage-stat-label">Input Tokens</div>
                  <div className="usage-stat-value accent-blue">{stats.total_input_tokens.toLocaleString()}</div>
                </div>
                <div className="usage-stat-card">
                  <div className="usage-stat-label">Output Tokens</div>
                  <div className="usage-stat-value accent-green">{stats.total_output_tokens.toLocaleString()}</div>
                </div>
                <div className="usage-stat-card">
                  <div className="usage-stat-label">Requests</div>
                  <div className="usage-stat-value accent-amber">{stats.request_count.toLocaleString()}</div>
                </div>
                <div className="usage-stat-card">
                  <div className="usage-stat-label">Unknown Token Requests</div>
                  <div className="usage-stat-value accent-muted">{stats.unknown_request_count.toLocaleString()}</div>
                </div>
              </div>

              <UsageSection title="By Provider" buckets={stats.by_provider} emptyText="No provider-attributed usage." />
              <UsageSection title="By Model" buckets={stats.by_model} emptyText="No model-attributed usage." />
              <UsageSection
                title="By Project"
                buckets={stats.by_project}
                labelFor={(key) => projectName(key)}
                emptyText="No project-attributed usage."
              />
              <UsageSection
                title="By Task"
                buckets={stats.by_task as Record<string, UsageBucketStats>}
                labelFor={(key) => `Task ${key.slice(0, 8)}…`}
                emptyText="No task-attributed usage."
              />
            </>
          )}

          <section className="usage-section">
            <h3>Recent Records</h3>
            <div className="usage-filters">
              <label>
                Project
                <select value={filterProject} onChange={(e) => setFilterProject(e.target.value)} disabled={loading || clearing}>
                  <option value="">All projects</option>
                  {projects.map((p) => (
                    <option key={p.id} value={p.id}>{p.name}</option>
                  ))}
                </select>
              </label>
              <label>
                Provider
                <input value={filterProvider} onChange={(e) => setFilterProvider(e.target.value)} placeholder="provider id" disabled={loading || clearing} />
              </label>
              <label>
                Model
                <input value={filterModel} onChange={(e) => setFilterModel(e.target.value)} placeholder="model id" disabled={loading || clearing} />
              </label>
            </div>
            {records.length === 0 ? (
              <p className="muted-note">No records match the current filters.</p>
            ) : (
              <div className="usage-records">
                {[...records].reverse().slice(0, 100).map((rec) => (
                  <div key={rec.id} className="usage-record">
                    <div className="usage-record-main">
                      <strong>{rec.operation_type || "operation"}</strong>
                      <span className="usage-record-meta">
                        {formatTimestamp(rec.created_at)}
                        {rec.context.provider_id || rec.context.provider_name ? ` · ${rec.context.provider_name ?? rec.context.provider_id}` : " · provider unknown"}
                        {rec.context.model_id || rec.context.model_name ? ` · ${rec.context.model_name ?? rec.context.model_id}` : " · model unknown"}
                        {rec.context.project_id ? ` · ${projectName(rec.context.project_id)}` : " · project missing"}
                        {rec.context.task_id ? ` · task ${rec.context.task_id.slice(0, 8)}…` : " · task missing"}
                        {rec.context.responsibility ? ` · ${rec.context.responsibility}` : ""}
                        {rec.is_retry ? " · retry" : ""}
                        {!rec.count_towards_totals ? " · not counted in totals" : ""}
                      </span>
                    </div>
                    <div className="usage-record-tokens">
                      {rec.usage.tokens_unknown ? (
                        <span className="usage-unknown">Token counts unknown</span>
                      ) : (
                        <>
                          <span>In: {formatTokenCount(rec.usage.input_tokens)}</span>
                          <span>Out: {formatTokenCount(rec.usage.output_tokens)}</span>
                          <span><strong>Total: {formatTokenCount(rec.usage.total_tokens)}</strong></span>
                        </>
                      )}
                    </div>
                  </div>
                ))}
              </div>
            )}
          </section>
        </div>

        <div className="config-modal-footer">
          <button className="secondary-button" onClick={onClose}>Close</button>
        </div>
      </div>
    </div>
  );
}

function FileTreeItem({
  entry,
  depth,
  projectRoot,
  expandedPaths,
  dirChildren,
  dirLoading,
  dirErrors,
  selectedPath,
  onToggleDir,
  onSelectFile,
  onRetryDir,
}: {
  entry: FileEntry;
  depth: number;
  projectRoot: string;
  expandedPaths: Set<string>;
  dirChildren: Record<string, FileEntry[]>;
  dirLoading: Record<string, boolean>;
  dirErrors: Record<string, string>;
  selectedPath: string | null;
  onToggleDir: (path: string) => void;
  onSelectFile: (entry: FileEntry) => void;
  onRetryDir: (path: string) => void;
}) {
  const isExpanded = expandedPaths.has(entry.path);
  const isLoading = dirLoading[entry.path] ?? false;
  const error = dirErrors[entry.path];
  const children = dirChildren[entry.path];
  const isSelected = selectedPath === entry.path;
  const paddingLeft = 10 + depth * 14;

  if (entry.is_dir) {
    return (
      <div className="tree-dir-group">
        <button
          type="button"
          className={`tree-node ${isSelected ? "selected" : ""}`}
          style={{ paddingLeft: `${paddingLeft}px` }}
          onClick={() => onToggleDir(entry.path)}
          title={entry.path}
        >
          <span className="tree-arrow">{isExpanded ? "▾" : "▸"}</span>
          <span className="tree-icon">{isExpanded ? "📂" : "📁"}</span>
          <span className="tree-name">{entry.name}</span>
        </button>

        {isExpanded && (
          <div className="tree-children">
            {isLoading && (
              <div
                className="tree-status-sub"
                style={{ paddingLeft: `${paddingLeft + 18}px` }}
              >
                Loading...
              </div>
            )}
            {error && (
              <div
                className="tree-status-sub tree-status-error"
                style={{ paddingLeft: `${paddingLeft + 18}px` }}
              >
                <span>{error}</span>
                <button
                  type="button"
                  onClick={(e) => {
                    e.stopPropagation();
                    onRetryDir(entry.path);
                  }}
                >
                  Retry
                </button>
              </div>
            )}
            {!isLoading && !error && children && children.length === 0 && (
              <div
                className="tree-status-sub"
                style={{ paddingLeft: `${paddingLeft + 18}px` }}
              >
                (empty)
              </div>
            )}
            {!isLoading &&
              !error &&
              children &&
              children.map((child) => (
                <FileTreeItem
                  key={child.path}
                  entry={child}
                  depth={depth + 1}
                  projectRoot={projectRoot}
                  expandedPaths={expandedPaths}
                  dirChildren={dirChildren}
                  dirLoading={dirLoading}
                  dirErrors={dirErrors}
                  selectedPath={selectedPath}
                  onToggleDir={onToggleDir}
                  onSelectFile={onSelectFile}
                  onRetryDir={onRetryDir}
                />
              ))}
          </div>
        )}
      </div>
    );
  }

  return (
    <button
      type="button"
      className={`tree-node ${isSelected ? "selected" : ""}`}
      style={{ paddingLeft: `${paddingLeft}px` }}
      onClick={() => onSelectFile(entry)}
      title={entry.path}
    >
      <span className="tree-arrow" />
      <span className="tree-icon">{getFileBadge(entry.name)}</span>
      <span className="tree-name">{entry.name}</span>
    </button>
  );
}

// Provider Configuration Modal
function ProviderConfigModal({
  isOpen,
  onClose,
  providerStatus,
  onSaved,
}: {
  isOpen: boolean;
  onClose: () => void;
  providerStatus: ProviderStatus | null;
  onSaved: (status: ProviderStatus) => void;
}) {
  const [plannerType, setPlannerType] = useState(
    providerStatus?.planner_type ?? "test",
  );
  const [executorType, setExecutorType] = useState(
    providerStatus?.executor_type ?? "test",
  );
  const [cloudEndpoint, setCloudEndpoint] = useState(
    providerStatus?.cloud_endpoint ?? "",
  );
  const [cloudModel, setCloudModel] = useState(
    providerStatus?.model_name === "default"
      ? ""
      : (providerStatus?.model_name ?? ""),
  );
  const [apiKey, setApiKey] = useState("");
  const [cliExecutable, setCliExecutable] = useState(
    providerStatus?.cli_executable === "none"
      ? ""
      : (providerStatus?.cli_executable ?? ""),
  );
  const [cliArgs, setCliArgs] = useState("");
  const [verificationCmd, setVerificationCmd] = useState("");
  const [ignoredDirs, setIgnoredDirs] = useState("");
  const [maxIterations, setMaxIterations] = useState(
    providerStatus?.max_iterations ?? 3,
  );
  const [isSaving, setIsSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [cliPresets, setCliPresets] = useState<{ id: string; name: string; default_executable: string }[]>([]);
  const [selectedExecutorPreset, setSelectedExecutorPreset] = useState<{ id: string; name: string; default_executable: string } | null>(null);

  useEffect(() => {
    if (providerStatus) {
      setPlannerType(providerStatus.planner_type);
      setExecutorType(providerStatus.executor_type);
      setCloudEndpoint(providerStatus.cloud_endpoint ?? "");
      setCloudModel(
        providerStatus.model_name === "default" ? "" : providerStatus.model_name,
      );
      setCliExecutable(
        providerStatus.cli_executable === "none"
          ? ""
          : providerStatus.cli_executable,
      );
      setCliArgs((providerStatus.cli_args ?? []).join(", "));
      setVerificationCmd(providerStatus.verification_command ?? "");
      setIgnoredDirs((providerStatus.ignored_dirs ?? []).join(", "));
      setMaxIterations(providerStatus.max_iterations);
    }
    // Fetch presets from backend
    invoke<{ id: string; name: string; default_executable: string }[]>("list_cli_agent_presets")
      .then(setCliPresets)
      .catch(() => setCliPresets([]));
  }, [providerStatus]);

  if (!isOpen) return null;

  async function handleSave() {
    setError(null);

    // Mirror the backend's hard requirements with immediate, actionable feedback.
    if (plannerType === "cloud_http" && !cloudEndpoint.trim()) {
      setError(
        "Cloud HTTP planner requires an endpoint URL. For a local model use a URL such as http://127.0.0.1:11434/v1.",
      );
      return;
    }
    if (executorType === "process_cli" && !cliExecutable.trim()) {
      setError(
        "Process CLI executor requires a CLI executable (a command on PATH or a full file path).",
      );
      return;
    }

    setIsSaving(true);
    try {
      const status = await invoke<ProviderStatus>("configure_providers", {
        config: {
          planner_type: plannerType,
          executor_type: executorType,
          // Empty strings intentionally clear optional settings; null means
          // "leave unchanged" and is reserved for the secret API key.
          cloud_endpoint: cloudEndpoint.trim(),
          cloud_model: cloudModel.trim(),
          api_key: apiKey.trim() || null,
          cli_executable: cliExecutable.trim(),
          cli_args: parseList(cliArgs),
          verification_command: verificationCmd.trim(),
          ignored_dirs: parseList(ignoredDirs),
          max_iterations: Math.max(1, Math.min(10, maxIterations)),
        },
      });
      onSaved(status);
      setApiKey("");
      onClose();
    } catch (e) {
      setError(describeError(e));
    } finally {
      setIsSaving(false);
    }
  }

  return (
    <div className="config-modal-backdrop" onClick={onClose}>
      <div className="config-modal" onClick={(e) => e.stopPropagation()}>
        <div className="config-modal-header">
          <h2>Provider Configuration</h2>
          <button
            className="icon-button small"
            onClick={onClose}
            title="Close"
          >
            ×
          </button>
        </div>

        <div className="config-modal-body">
          {providerStatus && (
            <div className="config-mode-banner">
              {providerStatus.is_test_mode && (
                <span className="beta-tag test-mode">TEST MODE</span>
              )}
              {!providerStatus.is_test_mode && (
                <span className="beta-tag">PRODUCTION MODE</span>
              )}
            </div>
          )}

          <div className="config-form-row">
            <label>Planner Type</label>
            <div className="config-radio-group">
              <label>
                <input
                  type="radio"
                  name="planner_type"
                  value="test"
                  checked={plannerType === "test"}
                  onChange={() => setPlannerType("test")}
                />
                Test (default simulator)
              </label>
              <label>
                <input
                  type="radio"
                  name="planner_type"
                  value="cloud_http"
                  checked={plannerType === "cloud_http"}
                  onChange={() => setPlannerType("cloud_http")}
                />
                Cloud HTTP (OpenAI-compatible)
              </label>
            </div>
          </div>

          <div className="config-form-row">
            <label>Executor Type</label>
            <div className="config-radio-group">
              <label>
                <input
                  type="radio"
                  name="executor_type"
                  value="test"
                  checked={executorType === "test"}
                  onChange={() => setExecutorType("test")}
                />
                Test (default simulator)
              </label>
              <label>
                <input
                  type="radio"
                  name="executor_type"
                  value="process_cli"
                  checked={executorType === "process_cli"}
                  onChange={() => setExecutorType("process_cli")}
                />
                Process CLI
              </label>
            </div>
          </div>

          {cliPresets.length > 0 && (
            <>
              <div className="config-form-row">
                <label>Executor Preset</label>
                <select
                  value={selectedExecutorPreset?.id ?? ""}
                  onChange={(e) => {
                    const id = e.target.value;
                    const preset = cliPresets.find((p) => p.id === id);
                    if (preset) {
                      setSelectedExecutorPreset(preset);
                      setCliExecutable(preset.default_executable);
                    }
                  }}
                  className="config-input"
                >
                  <option value="">None</option>
                  {cliPresets.map((preset) => (
                    <option key={preset.id} value={preset.id}>
                      {preset.name}
                    </option>
                  ))}
                </select>
              </div>
            </>
          )}

          {executorType === "process_cli" && (
            <>
              <div className="config-form-row">
                <label>CLI Executable</label>
                <div style={{ display: "flex", gap: 8 }}>
                  <input
                    type="text"
                    className="config-input"
                    placeholder="/path/to/coding-agent"
                    value={cliExecutable}
                    onChange={(e) => setCliExecutable(e.target.value)}
                  />
                  <button
                    type="button"
                    className="secondary-button"
                    onClick={() => {
                      void invoke<AvailabilityCheck>("check_cli_availability", {
                        executable: cliExecutable.trim(),
                      }).then((result) => {
                        if (result.ok) {
                          setError(`Available: ${result.path}`);
                          setTimeout(() => setError(null), 3000);
                        } else {
                          setError(result.error || "Executable not found");
                        }
                      }).catch((e) => setError(describeError(e)));
                    }}
                  >
                    Check availability
                  </button>
                </div>
                <div className="config-hint">
                  The program must exist on PATH or be an absolute file path.
                  Arguments belong in the next field, not here.
                </div>
              </div>

              <div className="config-form-row">
                <label>CLI Args (comma-separated)</label>
                <input
                  type="text"
                  className="config-input"
                  placeholder="--mode edit, --max-tokens, 4000"
                  value={cliArgs}
                  onChange={(e) => setCliArgs(e.target.value)}
                />
                <div className="config-hint">
                  Each comma-separated entry becomes one argument; arguments with
                  spaces are passed as a single value.
                </div>
              </div>
            </>
          )}

          {plannerType === "cloud_http" && (
            <>
              <div className="config-form-row">
                <label>Endpoint URL</label>
                <input
                  type="text"
                  className="config-input"
                  placeholder="https://api.openai.com/v1"
                  value={cloudEndpoint}
                  onChange={(e) => setCloudEndpoint(e.target.value)}
                />
                <div className="config-hint">
                  Local OpenAI-compatible servers (Ollama, LM Studio) need no API
                  key; use a loopback URL such as http://127.0.0.1:11434/v1.
                </div>
              </div>

              <div className="config-form-row">
                <label>Model Name</label>
                <input
                  type="text"
                  className="config-input"
                  placeholder="gpt-4o-mini"
                  value={cloudModel}
                  onChange={(e) => setCloudModel(e.target.value)}
                />
              </div>

              <div className="config-form-row">
                <label>API Key</label>
                <input
                  type="password"
                  className="config-input"
                  placeholder={
                    providerStatus?.has_api_key
                      ? "Key already configured — leave blank to keep it"
                      : "Enter API key"
                  }
                  value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)}
                />
                <div className="config-hint">
                  Kept in memory only and never written to disk. After a restart,
                  re-enter it or use PLANNER_API_KEY / OPENAI_API_KEY /
                  GEMINI_API_KEY.
                </div>
                {cloudEndpoint.trim() &&
                  !isLocalEndpointUrl(cloudEndpoint) &&
                  !providerStatus?.has_api_key &&
                  !apiKey.trim() && (
                    <div className="config-warning">
                      No API key is configured. Requests to a remote endpoint will
                      fail until you enter a key or set one of the environment
                      variables above.
                    </div>
                  )}
              </div>
            </>
          )}

          {executorType === "process_cli" && (
            <>
              <div className="config-form-row">
                <label>CLI Executable</label>
                <input
                  type="text"
                  className="config-input"
                  placeholder="/path/to/coding-agent"
                  value={cliExecutable}
                  onChange={(e) => setCliExecutable(e.target.value)}
                />
                <div className="config-hint">
                  The program must exist on PATH or be an absolute file path.
                  Arguments belong in the next field, not here.
                </div>
              </div>

              <div className="config-form-row">
                <label>CLI Args (comma-separated)</label>
                <input
                  type="text"
                  className="config-input"
                  placeholder="--mode edit, --max-tokens, 4000"
                  value={cliArgs}
                  onChange={(e) => setCliArgs(e.target.value)}
                />
                <div className="config-hint">
                  Each comma-separated entry becomes one argument; arguments with
                  spaces are passed as a single value.
                </div>
              </div>
            </>
          )}

          <div className="config-form-row">
            <label>Verification Command</label>
            <input
              type="text"
              className="config-input"
              placeholder="cargo test (or left blank)"
              value={verificationCmd}
              onChange={(e) => setVerificationCmd(e.target.value)}
            />
          </div>

          <div className="config-form-row">
            <label>Ignored Directories (comma-separated)</label>
            <input
              type="text"
              className="config-input"
              placeholder="node_modules, target, dist"
              value={ignoredDirs}
              onChange={(e) => setIgnoredDirs(e.target.value)}
            />
          </div>

          <div className="config-form-row">
            <label>Max Iterations</label>
            <input
              type="number"
              className="config-input"
              min={1}
              max={10}
              value={maxIterations}
              onChange={(e) =>
                setMaxIterations(
                  Math.max(1, Math.min(10, parseInt(e.target.value) || 3)),
                )
              }
            />
          </div>

          {error && (
            <div className="config-error">{error}</div>
          )}
        </div>

        <div className="config-modal-footer">
          <button className="secondary-button" onClick={onClose} disabled={isSaving}>
            Cancel
          </button>
          <button
            className="primary-button"
            onClick={handleSave}
            disabled={isSaving}
          >
            {isSaving ? "Saving..." : "Save Configuration"}
          </button>
        </div>
      </div>
    </div>
  );
}


function CodingAgentsModal({
  isOpen,
  onClose,
}: {
  isOpen: boolean;
  onClose: () => void;
}) {
  const [agents, setAgents] = useState<RegisteredCodingAgent[]>([]);
  const [loading, setLoading] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [successMessage, setSuccessMessage] = useState<string | null>(null);

  const [showAddForm, setShowAddForm] = useState(false);
  const [manualName, setManualName] = useState("");
  const [manualExecutable, setManualExecutable] = useState("");
  const [manualArgs, setManualArgs] = useState("");
  const [manualPromptDelivery, setManualPromptDelivery] = useState<PromptDelivery>("prefixed_stdin");
  const [submitting, setSubmitting] = useState(false);

  const loadAgents = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<RegisteredCodingAgent[]>("list_registered_agents");
      setAgents(list);
    } catch (e) {
      setError(describeError(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (isOpen) {
      void loadAgents();
    }
  }, [isOpen, loadAgents]);

  const handleRefresh = async () => {
    setRefreshing(true);
    setError(null);
    setSuccessMessage(null);
    try {
      const report = await invoke<DetectionReport>("refresh_coding_agents");
      setAgents(report.agents);
      const installedCount = report.agents.filter((a) => a.availability === "installed").length;
      setSuccessMessage(`Scan complete. Found ${installedCount} installed agent(s).`);
      setTimeout(() => setSuccessMessage(null), 4000);
    } catch (e) {
      setError(describeError(e));
    } finally {
      setRefreshing(false);
    }
  };

  const handleRegister = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!manualExecutable.trim()) {
      setError("Executable path or command is required.");
      return;
    }
    setSubmitting(true);
    setError(null);
    try {
      const input: ManualAgentInput = {
        name: manualName.trim() || null,
        executable: manualExecutable.trim(),
        default_args: parseList(manualArgs),
        prompt_delivery: manualPromptDelivery,
      };
      await invoke<RegisteredCodingAgent>("register_coding_agent", { input });
      setManualName("");
      setManualExecutable("");
      setManualArgs("");
      setShowAddForm(false);
      setSuccessMessage("Coding agent registered successfully.");
      setTimeout(() => setSuccessMessage(null), 4000);
      await loadAgents();
    } catch (e) {
      setError(describeError(e));
    } finally {
      setSubmitting(false);
    }
  };

  const handleRemove = async (id: string) => {
    setError(null);
    try {
      await invoke("remove_coding_agent", { id });
      setSuccessMessage("Agent registration removed.");
      setTimeout(() => setSuccessMessage(null), 4000);
      await loadAgents();
    } catch (e) {
      setError(describeError(e));
    }
  };

  if (!isOpen) return null;

  return (
    <div className="config-modal-backdrop" onClick={onClose}>
      <div
        className="config-modal profiles-modal"
        onClick={(event) => event.stopPropagation()}
        style={{ maxWidth: 860 }}
      >
        <div className="config-modal-header">
          <h2>Coding-Agent Registry</h2>
          <button className="icon-button small" onClick={onClose} title="Close">×</button>
        </div>

        <div style={{ display: "flex", flexDirection: "column", gap: 16, padding: 20 }}>
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
            <div style={{ fontSize: 13, color: "#9aa0ad" }}>
              CLI coding agents execute tasks in your project repositories. Identity is separate from AI provider APIs. Use <code style={{ background: "#21252d", padding: "1px 4px", borderRadius: 4, color: "#73c69c" }}>Scan / Refresh Agents</code> to detect installed tools, or register a custom executable manually.
            </div>
            <div style={{ display: "flex", gap: 8 }}>
              <button
                type="button"
                className="secondary-button"
                disabled={refreshing}
                onClick={() => void handleRefresh()}
              >
                {refreshing ? "Scanning…" : "Scan / Refresh Agents"}
              </button>
              <button
                type="button"
                className="primary-button"
                onClick={() => setShowAddForm(!showAddForm)}
              >
                {showAddForm ? "Cancel" : "＋ Register Custom Agent"}
              </button>
            </div>
          </div>

          {error && <div style={{ color: "#ff8a8a", fontSize: 13 }}>{error}</div>}
          {successMessage && <div style={{ color: "#73c69c", fontSize: 13 }}>{successMessage}</div>}

          {showAddForm && (
            <form onSubmit={handleRegister} style={{ background: "#1a1d24", padding: 16, borderRadius: 8, display: "flex", flexDirection: "column", gap: 12, border: "1px solid #2f3542" }}>
              <h3 style={{ margin: 0, fontSize: 14, color: "#e3e5ec" }}>Register Custom Coding Agent</h3>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
                <label style={{ display: "flex", flexDirection: "column", fontSize: 12, gap: 4, color: "#9aa0ad" }}>
                  Display Name
                  <input
                    type="text"
                    value={manualName}
                    onChange={(event) => setManualName(event.target.value)}
                    placeholder="e.g. My Custom Agent"
                    style={{ background: "#21252d", border: "1px solid #2f3542", borderRadius: 4, padding: "6px 8px", color: "#e3e5ec" }}
                  />
                </label>
                <label style={{ display: "flex", flexDirection: "column", fontSize: 12, gap: 4, color: "#9aa0ad" }}>
                  Executable Path or Command *
                  <input
                    type="text"
                    value={manualExecutable}
                    onChange={(event) => setManualExecutable(event.target.value)}
                    placeholder="e.g. my-agent or C:\tools\agent.exe"
                    style={{ background: "#21252d", border: "1px solid #2f3542", borderRadius: 4, padding: "6px 8px", color: "#e3e5ec" }}
                  />
                </label>
              </div>
              <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 12 }}>
                <label style={{ display: "flex", flexDirection: "column", fontSize: 12, gap: 4, color: "#9aa0ad" }}>
                  Default Arguments (comma separated)
                  <input
                    type="text"
                    value={manualArgs}
                    onChange={(event) => setManualArgs(event.target.value)}
                    placeholder="e.g. --run, --non-interactive"
                    style={{ background: "#21252d", border: "1px solid #2f3542", borderRadius: 4, padding: "6px 8px", color: "#e3e5ec" }}
                  />
                </label>
                <label style={{ display: "flex", flexDirection: "column", fontSize: 12, gap: 4, color: "#9aa0ad" }}>
                  Prompt Delivery
                  <select
                    value={manualPromptDelivery}
                    onChange={(event) => setManualPromptDelivery(event.target.value as PromptDelivery)}
                    style={{ background: "#21252d", border: "1px solid #2f3542", borderRadius: 4, padding: "6px 8px", color: "#e3e5ec" }}
                  >
                    <option value="prefixed_stdin">Prefixed Stdin (-p + stdin)</option>
                    <option value="stdin">Stdin Only</option>
                    <option value="argument">Final Argument</option>
                  </select>
                </label>
              </div>
              <div style={{ display: "flex", justifyContent: "flex-end", gap: 8 }}>
                <button type="button" className="secondary-button" onClick={() => setShowAddForm(false)}>Cancel</button>
                <button type="submit" className="primary-button" disabled={submitting}>
                  {submitting ? "Registering…" : "Save Registration"}
                </button>
              </div>
            </form>
          )}

          <div style={{ display: "flex", flexDirection: "column", gap: 8, maxHeight: 440, overflowY: "auto" }}>
            {loading ? (
              <div style={{ padding: 24, textAlign: "center", color: "#9aa0ad" }}>Loading registered agents…</div>
            ) : agents.length === 0 ? (
              <div style={{ padding: 24, textAlign: "center", color: "#9aa0ad", background: "#1a1d24", borderRadius: 8 }}>
                No registered coding agents found. Click <code style={{ background: "#21252d", padding: "1px 4px", borderRadius: 4, color: "#73c69c" }}>Scan / Refresh Agents</code> to detect installed tools.
              </div>
            ) : (
              agents.map((agent) => {
                const isInstalled = agent.availability === "installed";
                const badgeColor = isInstalled ? "#73c69c" : agent.availability === "detection_error" ? "#ff8a8a" : "#9aa0ad";
                const badgeBg = isInstalled ? "#173322" : agent.availability === "detection_error" ? "#3a1c1c" : "#232731";
                return (
                  <div
                    key={agent.id}
                    style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 14, display: "flex", justifyContent: "space-between", alignItems: "flex-start" }}
                  >
                    <div style={{ display: "flex", flexDirection: "column", gap: 4, minWidth: 0 }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
                        <span style={{ fontWeight: 650, color: "#e3e5ec", fontSize: 14, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{agent.name}</span>
                        <span style={{ fontSize: 11, padding: "2px 6px", borderRadius: 4, background: badgeBg, color: badgeColor, fontWeight: 500, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                          {agent.availability.replace("_", " ")}
                        </span>
                        <span style={{ fontSize: 11, color: "#9aa0ad", background: "#21252d", padding: "2px 6px", borderRadius: 4 }}>
                          {agent.kind} · {agent.detection_method}
                        </span>
                      </div>
                      <div style={{ fontSize: 12, color: "#9aa0ad", fontFamily: "monospace" }}>
                        Executable: {agent.executable} {agent.resolved_path && agent.resolved_path !== agent.executable ? `(${agent.resolved_path})` : ""}
                      </div>
                      {agent.version && (
                        <div style={{ fontSize: 12, color: "#73c69c" }}>Version: {agent.version}</div>
                      )}
                      {agent.status_message && (
                        <div style={{ fontSize: 12, color: "#ff8a8a" }}>{agent.status_message}</div>
                      )}
                    </div>
                    {agent.kind === "custom" && (
                      <button
                        type="button"
                        className="secondary-button"
                        style={{ color: "#ff8a8a", borderColor: "#ff8a8a33" }}
                        onClick={() => void handleRemove(agent.id)}
                        title="Remove this custom agent registration"
                      >
                        Remove
                      </button>
                    )}
                  </div>
                );
              })
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function ModelPreferencesModal({
  isOpen,
  onClose,
}: {
  isOpen: boolean;
  onClose: () => void;
}) {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [prefs, setPrefs] = useState<ModelPreferences | null>(null);
  const [savedPrefs, setSavedPrefs] = useState<ModelPreferences | null>(null);
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [selectionPreview, setSelectionPreview] = useState<ModelSelection | null>(null);
  const [selectionError, setSelectionError] = useState<string | null>(null);
  const loadSeq = useRef(0);

  const load = useCallback(async () => {
    const seq = ++loadSeq.current;
    setLoading(true);
    setError(null);
    setStatus(null);
    try {
      const [m, p] = await Promise.all([
        invoke<ModelInfo[]>("list_available_models"),
        invoke<ModelPreferences>("get_model_preferences"),
      ]);
      if (seq !== loadSeq.current) return;
      setModels(m);
      setPrefs(p);
      setSavedPrefs(p);
    } catch (e) {
      if (seq !== loadSeq.current) return;
      setError(describeError(e));
    } finally {
      if (seq === loadSeq.current) setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (isOpen) void load();
  }, [isOpen, load]);

  useEffect(() => {
    if (!isOpen) return;
    let cancelled = false;
    void (async () => {
      try {
        const sel = await invoke<ModelSelection | null>("get_model_selection", {
          taskId: null,
          responsibility: "default",
          providerId: null,
        });
        if (!cancelled) {
          setSelectionPreview(sel);
          setSelectionError(null);
        }
      } catch (e) {
        if (!cancelled) {
          setSelectionPreview(null);
          setSelectionError(describeError(e));
        }
      }
    })();
    return () => { cancelled = true; };
  }, [isOpen, savedPrefs]);

  if (!isOpen) return null;

  const modelById = new Map(models.map((m) => [m.id, m]));
  const availableModels = models.filter((m) => m.available !== false && !m.deprecated);
  const unavailableOrDeprecated = models.filter((m) => m.available === false || m.deprecated);

  const setDefault = (modelId: string) => {
    setStatus(null);
    setPrefs((p) => p ? { ...p, default_model_id: modelId || null } : p);
  };

  const setResp = (resp: AiResponsibilityId, modelId: string, autoSelect: boolean) => {
    setStatus(null);
    setPrefs((p) => {
      if (!p) return p;
      const next = { ...p, responsibility_preferences: { ...(p.responsibility_preferences ?? {}) } };
      next.responsibility_preferences[resp] = { model_id: modelId || null, auto_select: autoSelect };
      return next;
    });
  };

  const save = async () => {
    if (!prefs || saving) return;
    setSaving(true);
    setError(null);
    setStatus(null);
    try {
      const next = await invoke<ModelPreferences>("update_model_preferences", { preferences: prefs });
      setPrefs(next);
      setSavedPrefs(next); // reflect backend-returned values, not frontend-only state
      setStatus("Model preferences saved (backend-confirmed).");
    } catch (e) {
      setError(describeError(e));
    } finally {
      setSaving(false);
    }
  };

  const renderModelOption = (m: ModelInfo) => {
    const flags = [
      m.deprecated ? "deprecated" : null,
      m.available === false ? "unavailable" : null,
    ].filter(Boolean).join(", ");
    return `${m.name} (${m.provider_name})${flags ? ` — ${flags}` : ""}`;
  };

  return (
    <div className="config-modal-backdrop" onClick={onClose}>
      <div className="config-modal model-prefs-modal" onClick={(e) => e.stopPropagation()}>
        <div className="config-modal-header">
          <h2>Model Preferences</h2>
          <div className="history-header-actions">
            <button className="secondary-button" onClick={() => void load()} disabled={loading || saving}>
              {loading ? "Loading..." : "Refresh"}
            </button>
            <button className="icon-button small" onClick={onClose} title="Close">×</button>
          </div>
        </div>

        <div className="config-modal-body model-prefs-body">
          <div className="config-hint">
            Preferences are loaded from and saved to the backend. A selected/default model is a preference only;
            the backend/runtime determines actual execution. Project-level overrides and task-level manual selection
            are shown only where backend commands support them (task override is available for existing tasks via
            backend APIs; new-task preselection is not exposed by the current backend contract).
          </div>

          {error && <div className="inline-error" role="alert">{error}</div>}
          {status && <div className="inline-success" role="status">{status}</div>}
          {loading && !prefs && <p className="muted-note">Loading model preferences from backend…</p>}

          {prefs && (
            <>
              <div className="model-selection-status">
                <strong>Backend selection preview (default context):</strong>{" "}
                {selectionError
                  ? <span>Unavailable ({selectionError})</span>
                  : selectionPreview
                    ? <>{selectionPreview.model_name} ({selectionPreview.provider_name}) · strategy: {selectionPreview.strategy}{selectionPreview.auto_selected ? " · auto-selected" : ""}</>
                    : <span>No model currently selected by backend for the default context.</span>}
                {savedPrefs?.auto_selection_enabled === false && (
                  <div className="config-warning" style={{ marginTop: 8 }}>Automatic selection is disabled in saved backend preferences.</div>
                )}
              </div>

              <div className="model-card">
                <h3>Global Default Model</h3>
                <div className="model-row">
                  <label>
                    Default model
                    <select value={prefs.default_model_id ?? ""} onChange={(e) => setDefault(e.target.value)} disabled={saving || loading}>
                      <option value="">None (use backend/default behavior)</option>
                      {models.map((m) => (
                        <option key={m.id} value={m.id} disabled={m.available === false}>
                          {renderModelOption(m)}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label>
                    Automatic selection
                    <select
                      value={prefs.auto_selection_enabled ? "enabled" : "disabled"}
                      onChange={(e) => { setStatus(null); setPrefs((p) => p ? { ...p, auto_selection_enabled: e.target.value === "enabled" } : p); }}
                      disabled={saving || loading}
                    >
                      <option value="enabled">Enabled</option>
                      <option value="disabled">Disabled</option>
                    </select>
                  </label>
                  <div />
                </div>
                {prefs.default_model_id && modelById.get(prefs.default_model_id)?.deprecated && (
                  <div className="config-warning">The saved default model is marked deprecated by the backend catalog. Select a replacement.</div>
                )}
                {prefs.default_model_id && modelById.get(prefs.default_model_id)?.available === false && (
                  <div className="config-warning">The saved default model is marked unavailable by the backend catalog.</div>
                )}
              </div>

              <div className="model-card">
                <h3>Responsibility-Specific Preferences</h3>
                {ALL_RESPONSIBILITIES.map((resp) => {
                  const rp = prefs.responsibility_preferences?.[resp];
                  const currentId = rp?.model_id ?? "";
                  const current = currentId ? modelById.get(currentId) : undefined;
                  return (
                    <div key={resp} className="model-row" style={{ gridTemplateColumns: "180px 1fr 1fr" }}>
                      <label>
                        Responsibility
                        <input value={RESPONSIBILITY_LABELS[resp]} readOnly />
                      </label>
                      <label>
                        Preferred model
                        <select
                          value={currentId}
                          onChange={(e) => setResp(resp, e.target.value, rp?.auto_select ?? false)}
                          disabled={saving || loading}
                        >
                          <option value="">None (inherit global/default/auto)</option>
                          {models.map((m) => (
                            <option key={m.id} value={m.id} disabled={m.available === false}>
                              {renderModelOption(m)}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label>
                        Auto-select for this responsibility
                        <select
                          value={rp?.auto_select ? "yes" : "no"}
                          onChange={(e) => setResp(resp, currentId, e.target.value === "yes")}
                          disabled={saving || loading}
                        >
                          <option value="no">No</option>
                          <option value="yes">Yes</option>
                        </select>
                      </label>
                      {(current?.deprecated || current?.available === false) && (
                        <div className="config-warning" style={{ gridColumn: "1 / -1" }}>
                          {RESPONSIBILITY_LABELS[resp]}: selected model is {current.deprecated ? "deprecated" : "unavailable"}.
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>

              <div className="model-prefs-grid">
                <div className="model-card">
                  <h3>Available Models ({availableModels.length})</h3>
                  {availableModels.length === 0 ? (
                    <p className="muted-note">No available models reported by the backend.</p>
                  ) : (
                    <div className="model-catalog">
                      {availableModels.map((m) => (
                        <div key={`${m.provider_id}:${m.id}`} className="model-catalog-item">
                          <div className="name">{m.name} <span className="model-badges"><span className="model-badge available">available</span></span></div>
                          <div className="meta">
                            id: {m.id} · provider: {m.provider_name} ({m.provider_id})
                            {m.context_window ? ` · ctx: ${m.context_window.toLocaleString()}` : ""}
                            {m.capabilities?.length ? ` · capabilities: ${m.capabilities.join(", ")}` : ""}
                            {m.description ? ` · ${m.description}` : ""}
                          </div>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
                <div className="model-card">
                  <h3>Deprecated / Unavailable ({unavailableOrDeprecated.length})</h3>
                  {unavailableOrDeprecated.length === 0 ? (
                    <p className="muted-note">No deprecated or unavailable models reported.</p>
                  ) : (
                    <div className="model-catalog">
                      {unavailableOrDeprecated.map((m) => (
                        <div key={`${m.provider_id}:${m.id}`} className="model-catalog-item">
                          <div className="name">
                            {m.name}{" "}
                            <span className="model-badges">
                              {m.deprecated && <span className="model-badge deprecated">deprecated</span>}
                              {m.available === false && <span className="model-badge unavailable">unavailable</span>}
                            </span>
                          </div>
                          <div className="meta">id: {m.id} · provider: {m.provider_name} ({m.provider_id})</div>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              </div>
            </>
          )}
        </div>

        <div className="config-modal-footer">
          <button className="secondary-button" onClick={onClose} disabled={saving}>Close</button>
          <button className="primary-button" onClick={() => void save()} disabled={saving || loading || !prefs}>
            {saving ? "Saving..." : "Save Preferences"}
          </button>
        </div>
      </div>
    </div>
  );
}
function ProfilesModal({
  isOpen,
  onClose,
  projects,
  profiles,
  onChanged,
}: {
  isOpen: boolean;
  onClose: () => void;
  projects: ProjectRecord[];
  profiles: ProviderProfile[];
  onChanged: () => Promise<void>;
}) {
  const [selectedId, setSelectedId] = useState<string>("");
  const [name, setName] = useState("New profile");
  const [scope, setScope] = useState("");
  const [isDefault, setIsDefault] = useState(false);
  const [plannerType, setPlannerType] = useState("test");
  const [executorType, setExecutorType] = useState("test");
  const [endpoint, setEndpoint] = useState("");
  const [model, setModel] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [cliExecutable, setCliExecutable] = useState("");
  const [cliArgs, setCliArgs] = useState("");
  const [promptDelivery, setPromptDelivery] =
    useState<PromptDelivery>("prefixed_stdin");
  const [verificationCmd, setVerificationCmd] = useState("");
  const [maxIterations, setMaxIterations] = useState(3);
  const [ignoredDirs, setIgnoredDirs] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [localModels, setLocalModels] = useState<LocalModelPreset[]>([]);
  const [profileHasKey, setProfileHasKey] = useState<boolean | null>(null);
  const [providerTestStatus, setProviderTestStatus] = useState<
    "idle" | "testing" | "success" | "error"
  >("idle");
  const [providerTestMessage, setProviderTestMessage] = useState<string>("");

  useEffect(() => {
    if (!isOpen) return;
    invoke<LocalModelPreset[]>("list_local_model_presets")
      .then(setLocalModels)
      .catch(() => setLocalModels([]));
  }, [isOpen]);

  useEffect(() => {
    setProfileHasKey(null);
    setProviderTestStatus("idle");
    setProviderTestMessage("");
    if (!isOpen || !selectedId) {
      return;
    }
    let disposed = false;
    invoke<ProviderStatus>("get_provider_status_for", { profileId: selectedId })
      .then((status) => {
        if (!disposed) setProfileHasKey(status.has_api_key);
      })
      .catch(() => {
        if (!disposed) setProfileHasKey(null);
      });
    return () => {
      disposed = true;
    };
  }, [isOpen, selectedId, profiles]);

  useEffect(() => {
    if (!isOpen) return;
    const profile = profiles.find((candidate) => candidate.id === selectedId);
    if (!profile) {
      setName("New profile");
      setScope("");
      setIsDefault(false);
      setPlannerType("test");
      setExecutorType("test");
      setEndpoint("");
      setModel("");
      setApiKey("");
      setCliExecutable("");
      setCliArgs("");
      setPromptDelivery("prefixed_stdin");
      setVerificationCmd("");
      setMaxIterations(3);
      setIgnoredDirs("");
      return;
    }
    setName(profile.name);
    setScope(profile.project_id ?? "");
    setIsDefault(profile.is_default);
    setPlannerType(profile.config.planner_type);
    setExecutorType(profile.config.executor_type);
    setEndpoint(profile.config.cloud_endpoint ?? "");
    setModel(profile.config.cloud_model ?? "");
    setApiKey("");
    setCliExecutable(profile.config.cli_executable ?? "");
    setCliArgs((profile.config.cli_args ?? []).join(", "));
    setPromptDelivery(profile.config.prompt_delivery ?? "prefixed_stdin");
    setVerificationCmd(profile.config.verification_command ?? "");
    setMaxIterations(profile.config.max_iterations);
    setIgnoredDirs(profile.config.ignored_dirs.join(", "));
  }, [isOpen, selectedId, profiles]);

  if (!isOpen) return null;

  async function clearSavedCredential() {
    if (!selectedId) return;
    setSaving(true);
    setError(null);
    try {
      await invoke("delete_provider_credential", { profileId: selectedId });
      setApiKey("");
      await onChanged();
      const status = await invoke<ProviderStatus>("get_provider_status_for", {
        profileId: selectedId,
      });
      setProfileHasKey(status.has_api_key);
    } catch (clearError) {
      setError(describeError(clearError));
    } finally {
      setSaving(false);
    }
  }

  async function saveProfile() {
    if (!name.trim()) {
      setError("Give the profile a name.");
      return;
    }
    if (plannerType === "cloud_http" && !endpoint.trim()) {
      setError("Cloud HTTP planner requires an endpoint URL.");
      return;
    }
    if (executorType === "process_cli" && !cliExecutable.trim()) {
      setError("Process CLI executor requires an executable.");
      return;
    }
    setSaving(true);
    setError(null);
    const config: ProviderConfigInput = {
      planner_type: plannerType,
      executor_type: executorType,
      cloud_endpoint: endpoint.trim(),
      cloud_model: model.trim(),
      api_key: apiKey.trim() || null,
      cli_executable: cliExecutable.trim(),
      cli_args: parseList(cliArgs),
      prompt_delivery: promptDelivery,
      verification_command: verificationCmd.trim(),
      max_iterations: Math.max(1, Math.min(10, maxIterations)),
      ignored_dirs: parseList(ignoredDirs),
    };
    try {
      if (selectedId) {
        await invoke("update_provider_profile", {
          profileId: selectedId,
          name: name.trim(),
          isDefault,
          config,
        });
      } else {
        await invoke("create_provider_profile", {
          name: name.trim(),
          projectId: scope || null,
          isDefault,
          config,
        });
      }
      await onChanged();
      setApiKey("");
      setSelectedId("");
    } catch (saveError) {
      setError(describeError(saveError));
    } finally {
      setSaving(false);
    }
  }

  async function deleteProfile() {
    if (!selectedId) return;
    setSaving(true);
    setError(null);
    try {
      await invoke("delete_provider_profile", { profileId: selectedId });
      await onChanged();
      setSelectedId("");
    } catch (deleteError) {
      setError(describeError(deleteError));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="config-modal-backdrop" onClick={onClose}>
      <div
        className="config-modal profiles-modal"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="config-modal-header">
          <h2>Provider Profiles</h2>
          <button className="icon-button small" onClick={onClose} title="Close">
            ×
          </button>
        </div>

        <div className="profiles-layout">
          <div className="profiles-list">
            {profiles.map((profile) => (
              <button
                key={profile.id}
                type="button"
                className={`profile-item ${
                  selectedId === profile.id ? "selected" : ""
                }`}
                onClick={() => setSelectedId(profile.id)}
              >
                <span>
                  {profile.name}
                  {profile.is_default ? " ★" : ""}
                </span>
                <small>
                  {profile.project_id
                    ? projects.find((p) => p.id === profile.project_id)?.name ??
                      "Project"
                    : "All projects"}
                </small>
              </button>
            ))}
            <button
              type="button"
              className="profile-item new"
              onClick={() => setSelectedId("")}
            >
              ＋ New profile
            </button>
          </div>

          <div className="profiles-form">
            <div className="form-row">
              <label>
                Name
                <input
                  value={name}
                  onChange={(event) => setName(event.target.value)}
                />
              </label>
              <label>
                Scope
                <select
                  value={scope}
                  onChange={(event) => setScope(event.target.value)}
                  disabled={Boolean(selectedId)}
                >
                  <option value="">All projects</option>
                  {projects.map((project) => (
                    <option key={project.id} value={project.id}>
                      {project.name}
                    </option>
                  ))}
                </select>
              </label>
            </div>

            <label className="checkbox-row">
              <input
                type="checkbox"
                checked={isDefault}
                onChange={(event) => setIsDefault(event.target.checked)}
              />
              Default profile for this scope
            </label>

            <div className="form-row">
              <label>
                Planner
                <select
                  value={plannerType}
                  onChange={(event) => setPlannerType(event.target.value)}
                >
                  <option value="test">Test (simulated)</option>
                  <option value="cloud_http">Cloud / local HTTP</option>
                </select>
              </label>
              <label>
                Executor
                <select
                  value={executorType}
                  onChange={(event) => setExecutorType(event.target.value)}
                >
                  <option value="test">Test (simulated)</option>
                  <option value="process_cli">CLI coding agent</option>
                </select>
              </label>
            </div>

            <label>
              HTTP endpoint (cloud or local model server)
              <input
                value={endpoint}
                onChange={(event) => setEndpoint(event.target.value)}
                placeholder="http://127.0.0.1:11434/v1"
              />
            </label>
            <div className="form-row">
              <label>
                Model
                <input
                  value={model}
                  onChange={(event) => setModel(event.target.value)}
                  placeholder="qwen2.5-coder"
                />
                <select
                  value=""
                  onChange={(event) => {
                    const preset = localModels.find(
                      (candidate) => candidate.id === event.target.value,
                    );
                    if (preset) {
                      setEndpoint(preset.endpoint);
                      if (preset.suggested_models[0]) {
                        setModel(preset.suggested_models[0]);
                      }
                    }
                  }}
                >
                  <option value="">Use a local model preset…</option>
                  {localModels.map((preset) => (
                    <option key={preset.id} value={preset.id}>
                      {preset.name}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                API key (saved securely on this device)
                <input
                  type="password"
                  value={apiKey}
                  onChange={(event) => setApiKey(event.target.value)}
                  placeholder="leave blank to keep"
                />
                <span className="config-hint">
                  {endpoint.trim() && !isLocalEndpointUrl(endpoint)
                    ? profileHasKey
                      ? "A key is available for this profile (secure storage or environment). Leave blank to keep it."
                      : profileHasKey === false
                        ? "No key is currently available for this remote endpoint. Enter one to use it."
                        : "Credential availability could not be confirmed. Check secure storage or environment settings."
                    : "Only needed for remote endpoints; local servers can run without a key."}
                </span>
              </label>
              <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <button
                  type="button"
                  className="secondary-button"
                  disabled={providerTestStatus === "testing" || !endpoint.trim()}
                  onClick={() => {
                    setProviderTestStatus("testing");
                    setProviderTestMessage("");
                    void invoke<string>("test_provider_connection", {
                      endpoint: endpoint.trim(),
                      model: model.trim(),
                      apiKey: apiKey.trim() || null,
                    })
                      .then((msg) => {
                        setProviderTestStatus("success");
                        setProviderTestMessage(msg);
                      })
                      .catch((e) => {
                        setProviderTestStatus("error");
                        setProviderTestMessage(describeError(e));
                      });
                  }}
                >
                  {providerTestStatus === "testing" ? "Testing…" : "Test connection"}
                </button>
                {providerTestStatus !== "idle" && providerTestMessage && (
                  <span
                    style={{
                      fontSize: 12,
                      color:
                        providerTestStatus === "success"
                          ? "#73c69c"
                          : "#ff8a8a",
                    }}
                  >
                    {providerTestMessage}
                  </span>
                )}
              </div>
            </div>

            <div className="form-row">
              <label>
                CLI executable
                <input
                  value={cliExecutable}
                  onChange={(event) => setCliExecutable(event.target.value)}
                  placeholder="claude"
                />
              </label>
              <label>
                CLI args (comma separated)
                <input
                  value={cliArgs}
                  onChange={(event) => setCliArgs(event.target.value)}
                />
              </label>
            </div>
            {selectedId && (
              <button
                type="button"
                className="secondary-button"
                disabled={saving}
                onClick={clearSavedCredential}
              >
                Remove saved key
              </button>
            )}

            <div className="form-row">
              <label>
                Prompt delivery
                <select
                  value={promptDelivery}
                  onChange={(event) =>
                    setPromptDelivery(event.target.value as PromptDelivery)
                  }
                >
                  <option value="prefixed_stdin">-p + stdin</option>
                  <option value="stdin">stdin only</option>
                  <option value="argument">prompt as argument</option>
                </select>
              </label>
              <label>
                Max iterations
                <input
                  type="number"
                  min={1}
                  max={10}
                  value={maxIterations}
                  onChange={(event) =>
                    setMaxIterations(Number(event.target.value))
                  }
                />
              </label>
            </div>

            <label>
              Verification command
              <input
                value={verificationCmd}
                onChange={(event) => setVerificationCmd(event.target.value)}
                placeholder="cargo test"
              />
            </label>
            <label>
              Ignored directories (comma separated)
              <input
                value={ignoredDirs}
                onChange={(event) => setIgnoredDirs(event.target.value)}
              />
            </label>

            {error && <div className="inline-error">{error}</div>}

            <div className="profiles-actions">
              <button
                type="button"
                className="primary-button"
                onClick={() => void saveProfile()}
                disabled={saving}
              >
                {saving ? "Saving..." : selectedId ? "Update profile" : "Create profile"}
              </button>
              {selectedId && (
                <button
                  type="button"
                  className="danger-button"
                  onClick={() => void deleteProfile()}
                  disabled={saving}
                >
                  Delete
                </button>
              )}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}


function PlanCard({
  plan,
  awaitingApproval,
  busy,
  pendingAction,
  showRejectForm,
  rejectReason,
  onApprove,
  onStartReject,
  onCancelReject,
  onReject,
  onRejectReasonChange,
  showSteps = true,
}: {
  plan: Plan;
  awaitingApproval: boolean;
  busy: boolean;
  pendingAction: PendingAction | null;
  showRejectForm: boolean;
  rejectReason: string;
  onApprove: () => void;
  onStartReject: () => void;
  onCancelReject: () => void;
  onReject: () => void;
  onRejectReasonChange: (value: string) => void;
  showSteps?: boolean;
}) {
  return (
    <div className={`plan-card ${awaitingApproval ? "plan-card-awaiting" : ""}`}>
      <div className="plan-card-head">
        <span>PLAN</span>
        {awaitingApproval && (
          <span className="plan-awaiting">awaiting your approval</span>
        )}
      </div>
      <p className="plan-summary">{plan.summary}</p>

      {showSteps && plan.steps.length > 0 && (
        <>
          <div className="plan-section-title">Steps</div>
          <ol className="plan-list">
            {plan.steps.map((step, index) => (
              <li key={index}>{step}</li>
            ))}
          </ol>
        </>
      )}

      {plan.acceptance_criteria.length > 0 && (
        <>
          <div className="plan-section-title">Acceptance criteria</div>
          <ul className="plan-list">
            {plan.acceptance_criteria.map((criterion, index) => (
              <li key={index}>{criterion}</li>
            ))}
          </ul>
        </>
      )}

      {awaitingApproval && (
        <div className="plan-actions">
          {!showRejectForm ? (
            <>
              <button
                type="button"
                className="primary-button"
                onClick={onApprove}
                disabled={busy}
              >
                {pendingAction === "approve" ? "Approving..." : "Approve plan"}
              </button>
              <button
                type="button"
                className="secondary-button"
                onClick={onStartReject}
                disabled={busy}
              >
                Reject
              </button>
            </>
          ) : (
            <div className="reject-form">
              <label htmlFor="reject-reason">
                Why are you rejecting this plan? (optional)
              </label>
              <textarea
                id="reject-reason"
                className="reject-input"
                rows={2}
                value={rejectReason}
                onChange={(event) => onRejectReasonChange(event.target.value)}
                placeholder="Tell the orchestrator what to change, or leave blank."
              />
              <div className="plan-actions">
                <button
                  type="button"
                  className="danger-button"
                  onClick={onReject}
                  disabled={busy}
                >
                  {pendingAction === "reject" ? "Rejecting..." : "Confirm reject"}
                </button>
                <button
                  type="button"
                  className="secondary-button"
                  onClick={onCancelReject}
                  disabled={busy}
                >
                  Back
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function GraphCard({ graph }: { graph: TaskGraph }) {
  const progress = graphProgress(graph);

  return (
    <div className="graph-card">
      <div className="graph-head">
        <span>EXECUTION STEPS</span>
        <span>
          {progress.done}/{progress.total} complete
          {progress.failed > 0 ? ` · ${progress.failed} failed` : ""}
        </span>
      </div>
      <div
        className="graph-progress"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={progress.percent}
      >
        <div
          className="graph-progress-bar"
          style={{ width: `${progress.percent}%` }}
        />
      </div>

      <ol className="graph-list">
        {graph.components.map((component, index) => (
          <li
            key={component.id}
            className={`graph-step ${componentStateClass(component.state)}`}
          >
            <div className="graph-step-head">
              <span className="graph-step-index">{index + 1}</span>
              <span className="graph-step-desc">{component.description}</span>
              <span className={`component-badge ${componentStateClass(component.state)}`}>
                {COMPONENT_STATE_LABELS[component.state]}
              </span>
            </div>

            {component.depends_on.length > 0 && (
              <div className="graph-step-meta">
                Depends on{" "}
                {component.depends_on
                  .map((id) => dependencyLabel(graph, id))
                  .join(", ")}
              </div>
            )}

            {component.error && (
              <div className="graph-step-error">{component.error}</div>
            )}

            {(component.state === "running" ||
              component.state === "verifying" ||
              component.state === "retrying") &&
              component.acceptance_criteria.length > 0 && (
                <details className="graph-step-details">
                  <summary>Acceptance criteria</summary>
                  <ul className="plan-list">
                    {component.acceptance_criteria.map((criterion, criteriaIndex) => (
                      <li key={criteriaIndex}>{criterion}</li>
                    ))}
                  </ul>
                </details>
              )}
          </li>
        ))}
      </ol>
    </div>
  );
}

function FileGroups({
  files,
  emptyLabel,
}: {
  files: ChangedFiles;
  emptyLabel: string;
}) {
  const total = changedFilesCount(files);
  if (total === 0) {
    return <p className="muted-note">{emptyLabel}</p>;
  }

  const groups: Array<{
    key: keyof ChangedFiles;
    label: string;
  }> = [
    { key: "created", label: "Created" },
    { key: "modified", label: "Modified" },
    { key: "deleted", label: "Deleted" },
  ];

  return (
    <div className="file-groups">
      {groups.map((group) => {
        const paths = files[group.key];
        if (paths.length === 0) return null;
        return (
          <div key={group.key} className={`file-group file-group-${group.key}`}>
            <div className="file-group-title">
              {group.label} ({paths.length})
            </div>
            <ul className="file-pill-list">
              {paths.map((path) => (
                <li key={path} className={`file-pill ${group.key}`} title={path}>
                  {path}
                </li>
              ))}
            </ul>
          </div>
        );
      })}
    </div>
  );
}

function VerificationList({
  verifications,
}: {
  verifications: VerificationResult[];
}) {
  if (verifications.length === 0) {
    return (
      <p className="muted-note">
        No verification command was configured for this task.
      </p>
    );
  }

  return (
    <div className="verification-list">
      {verifications.map((verification, index) => (
        <details key={index} className="verification-row">
          <summary>
            <span
              className={`verification-badge ${verificationStatusClass(
                verification.status,
              )}`}
            >
              {VERIFICATION_STATUS_LABELS[verification.status]}
            </span>
            <span className="verification-command">{verification.command}</span>
            {verification.exit_code !== null && (
              <span className="verification-exit">
                exit {verification.exit_code}
              </span>
            )}
          </summary>
          {verification.output.trim() ? (
            <pre className="terminal-pre">{verification.output}</pre>
          ) : (
            <p className="muted-note">No verification output was captured.</p>
          )}
        </details>
      ))}
    </div>
  );
}

function IterationCard({
  record,
  sequence,
  isLatest,
}: {
  record: IterationRecord;
  sequence: number;
  isLatest: boolean;
}) {
  const verdict = effectiveVerdict(record);
  const review = record.reviewer_response;
  const executor = record.executor_result;
  const filesChanged = changedFilesCount(record.changed_files);
  const verificationFailed = verificationBlocksCompletion(record);

  return (
    <details className="iteration-card" open={isLatest}>
      <summary className="iteration-summary">
        <span className="iteration-number">Run {sequence}</span>
        <span className="iteration-attempt">attempt {record.iteration}</span>
        <span className={`verdict-badge ${verdictClass(verdict)}`}>
          {verdict ? REVIEW_VERDICT_LABELS[verdict] : "No review"}
        </span>
        <span className="iteration-meta">{filesChanged} files changed</span>
        {executor && (
          <span className="iteration-meta">
            {executor.timed_out
              ? "timed out"
              : `exit ${executor.exit_code ?? "n/a"}`}{" "}
            · {formatDuration(executor.duration_ms)}
          </span>
        )}
        {verificationFailed && (
          <span className="iteration-warning">verification failed</span>
        )}
      </summary>

      <div className="iteration-body">
        <div className="iteration-section">
          <div className="iteration-section-title">Executor</div>
          {executor ? (
            <>
              <div className="executor-meta">
                <span>Exit code: {executor.exit_code ?? "n/a"}</span>
                <span>Duration: {formatDuration(executor.duration_ms)}</span>
                {executor.timed_out && <span className="text-danger">Timed out</span>}
              </div>
              {executor.stdout.trim() && (
                <>
                  <div className="output-label">stdout</div>
                  <pre className="terminal-pre">{executor.stdout}</pre>
                </>
              )}
              {executor.stderr.trim() && (
                <>
                  <div className="output-label">stderr</div>
                  <pre className="terminal-pre terminal-pre-error">
                    {executor.stderr}
                  </pre>
                </>
              )}
              {!executor.stdout.trim() && !executor.stderr.trim() && (
                <p className="muted-note">The executor produced no output.</p>
              )}
            </>
          ) : (
            <p className="muted-note">No executor result was recorded.</p>
          )}
        </div>

        <div className="iteration-section">
          <div className="iteration-section-title">Changed files</div>
          <FileGroups
            files={record.changed_files}
            emptyLabel="No changed files were detected for this iteration."
          />
          {record.diff_summary && (
            <details className="diff-details">
              <summary>Diff summary</summary>
              <pre className="terminal-pre">{record.diff_summary}</pre>
            </details>
          )}
        </div>

        <div className="iteration-section">
          <div className="iteration-section-title">Verification</div>
          <VerificationList verifications={record.verification_results} />
          {verificationFailed && review?.verdict === "complete" && (
            <p className="iteration-warning">
              Verification did not pass, so this iteration was treated as
              needing changes even though the reviewer reported complete.
            </p>
          )}
        </div>

        <div className="iteration-section">
          <div className="iteration-section-title">Reviewer</div>
          {review ? (
            <>
              <div className="reviewer-head">
                <span className={`verdict-badge ${verdictClass(review.verdict)}`}>
                  {REVIEW_VERDICT_LABELS[review.verdict]}
                </span>
              </div>
              <p className="reviewer-findings">{review.findings}</p>
              {review.unresolved_issues.length > 0 && (
                <>
                  <div className="output-label">Unresolved issues</div>
                  <ul className="unresolved-list">
                    {review.unresolved_issues.map((issue, index) => (
                      <li key={index}>{issue}</li>
                    ))}
                  </ul>
                </>
              )}
              {review.suggested_fix && (
                <div className="suggested-fix">
                  <span>Suggested fix</span>
                  <p>{review.suggested_fix}</p>
                </div>
              )}
              {review.verdict === "needs_changes" && (
                <p className="muted-note">
                  Correction requested — the next attempt includes this feedback.
                </p>
              )}
            </>
          ) : (
            <p className="muted-note">The reviewer has not evaluated this attempt yet.</p>
          )}
        </div>

        <details className="diff-details">
          <summary>Coding prompt sent to the executor</summary>
          <pre className="terminal-pre">{record.coding_prompt}</pre>
        </details>
      </div>
    </details>
  );
}

function IterationTimeline({ task }: { task: TaskRecord }) {
  if (task.iterations.length === 0) return null;

  return (
    <div className="iteration-timeline">
      <div className="plan-section-title">
        Execution history ({task.iterations.length}{" "}
        {task.iterations.length === 1 ? "attempt" : "attempts"})
      </div>
      {[...task.iterations].reverse().map((record, index) => (
        <IterationCard
          key={`${record.iteration}-${record.started_at}`}
          record={record}
          sequence={task.iterations.length - index}
          isLatest={index === 0}
        />
      ))}
    </div>
  );
}

function TaskResultCard({ task }: { task: TaskRecord }) {
  const changed = collectTaskChangedFiles(task);
  const lastIteration = task.iterations[task.iterations.length - 1] ?? null;
  const review = lastIteration?.reviewer_response ?? null;
  const finalVerdict = lastIteration ? effectiveVerdict(lastIteration) : null;
  const verificationResults = task.iterations.flatMap(
    (iteration) => iteration.verification_results,
  );
  const failedVerifications = verificationResults.filter(
    (verification) => verification.status !== "passed",
  );
  const progress = graphProgress(task.task_graph);

  return (
    <div className={`task-result ${taskResultClass(task)}`}>
      <div className="task-result-head">
        <div className="task-result-title">{taskResultTitle(task)}</div>
        <span className={`task-state ${taskStateClass(task.state)}`}>
          {TASK_STATE_LABELS[task.state]}
        </span>
      </div>

      {task.error && <p className="task-result-detail">{task.error}</p>}

      <div className="result-stats">
        {task.task_graph ? (
          <div className="result-stat">
            <strong>
              {progress.done}/{progress.total}
            </strong>
            <span>steps complete</span>
          </div>
        ) : null}
        <div className="result-stat">
          <strong>{task.iterations.length}</strong>
          <span>attempts recorded</span>
        </div>
        <div className="result-stat">
          <strong>{changedFilesCount(changed)}</strong>
          <span>files changed</span>
        </div>
        <div className="result-stat">
          <strong>{verificationResults.length}</strong>
          <span>verification runs</span>
        </div>
        {failedVerifications.length > 0 && (
          <div className="result-stat result-stat-error">
            <strong>{failedVerifications.length}</strong>
            <span>verification problems</span>
          </div>
        )}
      </div>

      {review && (
        <div className="result-reviewer">
          <div className="output-label">Final review decision</div>
          <div className="reviewer-head">
            <span className={`verdict-badge ${verdictClass(finalVerdict)}`}>
              {finalVerdict
                ? REVIEW_VERDICT_LABELS[finalVerdict]
                : REVIEW_VERDICT_LABELS[review.verdict]}
            </span>
          </div>
          {finalVerdict && finalVerdict !== review.verdict && (
            <p className="muted-note">
              Verification did not pass, so this attempt was treated as needing
              changes even though the reviewer reported{" "}
              {REVIEW_VERDICT_LABELS[review.verdict].toLowerCase()}.
            </p>
          )}
          <p className="reviewer-findings">{review.findings}</p>
          {review.unresolved_issues.length > 0 && (
            <ul className="unresolved-list">
              {review.unresolved_issues.map((issue, index) => (
                <li key={index}>{issue}</li>
              ))}
            </ul>
          )}
          {review.suggested_fix && (
            <div className="suggested-fix">
              <span>Suggested fix</span>
              <p>{review.suggested_fix}</p>
            </div>
          )}
        </div>
      )}

      {verificationResults.length > 0 && (
        <>
          <div className="plan-section-title">Verification summary</div>
          <div className="verification-summary">
            {verificationResults.map((verification, index) => (
              <span
                key={index}
                className={`verification-badge ${verificationStatusClass(
                  verification.status,
                )}`}
                title={verification.command}
              >
                {VERIFICATION_STATUS_LABELS[verification.status]} ·{" "}
                {verification.command}
              </span>
            ))}
          </div>
        </>
      )}

      <div className="plan-section-title">Changed files</div>
      <FileGroups
        files={changed}
        emptyLabel="No changed files were recorded for this task."
      />

      <div className="result-next-step">
        <span>Next step</span>
        <p>{taskNextStep(task)}</p>
      </div>
    </div>
  );
}

function HistoryDetail({ task }: { task: TaskRecord }) {
  const changed = collectTaskChangedFiles(task);
  const progress = graphProgress(task.task_graph);

  return (
    <div className="history-detail">
      <div className="history-detail-head">
        <span className={`task-state ${taskStateClass(task.state)}`}>
          {TASK_STATE_LABELS[task.state]}
        </span>
        <span className="history-id">{shortId(task.id)}</span>
      </div>
      <h3>{task.user_task}</h3>
      <div className="history-meta">
        <span>{projectName(task.project_root)}</span>
        <span>{formatTimestamp(task.created_at)}</span>
        {task.task_graph ? (
          <span>
            {progress.done}/{progress.total} steps
          </span>
        ) : (
          <span>
            iteration {task.iteration}/{task.max_iterations}
          </span>
        )}
      </div>

      {task.error && <div className="inline-error">{task.error}</div>}

      <div className="history-section">
        <div className="plan-section-title">Outcome</div>
        <p className="history-outcome">{taskNextStep(task)}</p>
      </div>

      {task.plan && (
        <div className="history-section">
          <div className="plan-section-title">Plan</div>
          <p className="history-outcome">{task.plan.summary}</p>
          {task.plan.steps.length > 0 && !task.task_graph && (
            <ol className="plan-list">
              {task.plan.steps.map((step, index) => (
                <li key={index}>{step}</li>
              ))}
            </ol>
          )}
        </div>
      )}

      {task.task_graph && (
        <div className="history-section">
          <div className="plan-section-title">Steps</div>
          <ol className="history-graph-list">
            {task.task_graph.components.map((component) => (
              <li key={component.id} className="history-graph-step">
                <span className={`component-badge ${componentStateClass(component.state)}`}>
                  {COMPONENT_STATE_LABELS[component.state]}
                </span>
                <span>{component.description}</span>
              </li>
            ))}
          </ol>
        </div>
      )}

      <div className="history-section">
        <div className="plan-section-title">
          Attempts ({task.iterations.length})
        </div>
        {task.iterations.length === 0 ? (
          <p className="muted-note">No execution attempts were recorded.</p>
        ) : (
          <div className="history-attempts">
            {task.iterations.map((iteration, index) => {
              const verdict = effectiveVerdict(iteration);
              return (
                <div
                  key={`${iteration.iteration}-${iteration.started_at}`}
                  className="history-attempt"
                >
                  <span>Run {index + 1}</span>
                  <span>attempt {iteration.iteration}</span>
                  <span className={`verdict-badge ${verdictClass(verdict)}`}>
                    {verdict ? REVIEW_VERDICT_LABELS[verdict] : "No review"}
                  </span>
                  <span>{changedFilesCount(iteration.changed_files)} files</span>
                  <span>
                    {iteration.verification_results.length === 0
                      ? "no verification"
                      : iteration.verification_results
                          .map((v) => VERIFICATION_STATUS_LABELS[v.status])
                          .join(", ")}
                  </span>
                </div>
              );
            })}
          </div>
        )}
      </div>

      <div className="history-section">
        <div className="plan-section-title">Changed files</div>
        <FileGroups
          files={changed}
          emptyLabel="No changed files were recorded for this task."
        />
      </div>
    </div>
  );
}

function TaskHistoryModal({
  isOpen,
  onClose,
  onRetry,
  onApprove,
  onReject,
  onCancel,
}: {
  isOpen: boolean;
  onClose: () => void;
  onRetry: (taskId: string) => Promise<void>;
  onApprove: (taskId: string) => Promise<void>;
  onReject: (taskId: string, reason?: string | null) => Promise<void>;
  onCancel: (taskId: string) => Promise<void>;
}) {
  const [tasks, setTasks] = useState<TaskRecord[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [retrying, setRetrying] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const history = await invoke<TaskRecord[]>("list_task_history");
      setTasks(history);
      setSelectedId((current) => {
        if (current && history.some((task) => task.id === current)) {
          return current;
        }
        return history[0]?.id ?? null;
      });
    } catch (e) {
      setError(describeError(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (isOpen) {
      void load();
    }
  }, [isOpen, load]);

  if (!isOpen) return null;

  const selected =
    tasks.find((task) => task.id === selectedId) ?? tasks[0] ?? null;

  return (
    <div className="config-modal-backdrop" onClick={onClose}>
      <div
        className="config-modal history-modal"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="config-modal-header">
          <h2>Task History</h2>
          <div className="history-header-actions">
            <button
              className="secondary-button"
              onClick={load}
              disabled={loading}
            >
              {loading ? "Loading..." : "Refresh"}
            </button>
            <button className="icon-button small" onClick={onClose} title="Close">
              ×
            </button>
          </div>
        </div>

        <div className="history-layout">
          <div className="history-list">
            {error && <div className="inline-error">{error}</div>}
            {!error && loading && tasks.length === 0 && (
              <p className="muted-note">Loading task history...</p>
            )}
            {!error && !loading && tasks.length === 0 && (
              <div className="history-empty">
                <strong>No tasks yet</strong>
                <span>Completed and failed tasks will appear here.</span>
              </div>
            )}
            {tasks.map((task) => (
              <button
                type="button"
                key={task.id}
                className={`history-item ${
                  selected?.id === task.id ? "selected" : ""
                }`}
                onClick={() => setSelectedId(task.id)}
              >
                <span className="history-item-top">
                  <span className={`task-state ${taskStateClass(task.state)}`}>
                    {TASK_STATE_LABELS[task.state]}
                  </span>
                  <span className="history-task-time">
                    {formatTimestamp(task.created_at)}
                  </span>
                </span>
                <span className="history-task-text">{task.user_task}</span>
                <span className="history-task-meta">
                  {task.task_graph
                    ? `${graphProgress(task.task_graph).done}/${
                        graphProgress(task.task_graph).total
                      } steps`
                    : `iteration ${task.iteration}/${task.max_iterations}`}
                  {task.iterations.length > 0
                    ? ` · ${task.iterations.length} attempts`
                    : ""}
                </span>
              </button>
            ))}
          </div>

          <div className="history-detail-pane">
            {selected ? (
              <>
                <HistoryDetail task={selected} />
                <div className="history-retry-row">
                  {selected.state === "awaiting_approval" && (
                    <>
                      <button
                        type="button"
                        className="primary-button"
                        disabled={retrying}
                        onClick={async () => {
                          setRetrying(true);
                          setError(null);
                          try {
                            await onApprove(selected.id);
                            await load();
                          } catch (actionError) {
                            setError(describeError(actionError));
                          } finally {
                            setRetrying(false);
                          }
                        }}
                      >
                        Approve plan
                      </button>
                      <button
                        type="button"
                        className="secondary-button"
                        disabled={retrying}
                        onClick={async () => {
                          setRetrying(true);
                          setError(null);
                          try {
                            await onReject(selected.id, null);
                            await load();
                          } catch (actionError) {
                            setError(describeError(actionError));
                          } finally {
                            setRetrying(false);
                          }
                        }}
                      >
                        Reject plan
                      </button>
                    </>
                  )}
                  {!isTerminalTask(selected) && (
                    <button
                      type="button"
                      className="danger-button"
                      disabled={retrying}
                      onClick={async () => {
                        setRetrying(true);
                        setError(null);
                        try {
                          await onCancel(selected.id);
                          await load();
                        } catch (actionError) {
                          setError(describeError(actionError));
                        } finally {
                          setRetrying(false);
                        }
                      }}
                    >
                      Cancel task
                    </button>
                  )}
                  {isTerminalTask(selected) && (
                    <button
                      type="button"
                      className="primary-button"
                      disabled={retrying}
                      onClick={async () => {
                        setRetrying(true);
                        setError(null);
                        try {
                          await onRetry(selected.id);
                          await load();
                        } catch (retryError) {
                          setError(describeError(retryError));
                        } finally {
                          setRetrying(false);
                        }
                      }}
                    >
                      {retrying
                        ? "Working..."
                        : selected.plan
                          ? "Retry with saved plan"
                          : "Retry task"}
                    </button>
                  )}
                  <span className="muted-note">
                    {isTerminalTask(selected)
                      ? "Retries create a new task and reuse the saved plan when one exists."
                      : "Each task owns an independent runtime, so several can be active at once."}
                  </span>
                </div>
              </>
            ) : (
              <p className="muted-note">
                Select a task to inspect its plan, attempts, and result.
              </p>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}


function SessionsModal({
  isOpen,
  onClose,
  projects,
  profiles,
  cliPresets,
  activeProjectId,
  onChanged,
  onConfigureProviders,
  onTaskCreated,
}: {
  isOpen: boolean;
  onClose: () => void;
  projects: ProjectRecord[];
  profiles: ProviderProfile[];
  cliPresets: CliAgentPreset[];
  activeProjectId: string | null;
  onChanged: () => Promise<void>;
  onConfigureProviders: () => void;
  onTaskCreated: (task: TaskRecord) => void;
}) {
  const [sessions, setSessions] = useState<SessionRecord[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [kind, setKind] = useState<SessionKind>("prompt");
  const [title, setTitle] = useState("");
  const [projectId, setProjectId] = useState(activeProjectId ?? "");
  const [profileId, setProfileId] = useState("");
  const [presetId, setPresetId] = useState("claude");
  const [executable, setExecutable] = useState("");
  const [message, setMessage] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [sending, setSending] = useState(false);
  const [cancellingId, setCancellingId] = useState<string | null>(null);
  const [planningConversation, setPlanningConversation] = useState(false);
  const [sessionProviderStatus, setSessionProviderStatus] =
    useState<ProviderStatus | null>(null);

  const load = useCallback(async () => {
    try {
      const list = await invoke<SessionRecord[]>("list_sessions", {
        projectId: null,
      });
      setSessions(list);
      setSelectedId((current) =>
        current && list.some((session) => session.id === current)
          ? current
          : (list[0]?.id ?? null),
      );
    } catch (loadError) {
      setError(describeError(loadError));
    }
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    setProjectId(activeProjectId ?? "");
    void load();
  }, [isOpen, activeProjectId, load]);

  useEffect(() => {
    if (!isOpen) return;
    let disposed = false;
    let unlisten: UnlistenFn | null = null;
    listen<SessionRecord>("session_update", (event) => {
      setSessions((previous) => {
        const next = previous.filter(
          (session) => session.id !== event.payload.id,
        );
        return [event.payload, ...next].sort((a, b) =>
          b.updated_at.localeCompare(a.updated_at),
        );
      });
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    let disposed = false;
    let unlisten: UnlistenFn | null = null;
    listen<{
      session_id: string;
      message_id: string;
      turn_id: string;
      kind: string;
      delta?: string | null;
      content?: string | null;
      error?: string | null;
    }>("session_stream", (event) => {
      const p = event.payload;
      setSessions((previous) =>
        previous.map((session) => {
          if (session.id !== p.session_id) return session;
          const messages = [...session.messages];
          const idx = messages.findIndex((m) => m.id === p.message_id);
          if (p.kind === "started") {
            if (idx === -1) {
              messages.push({
                id: p.message_id,
                role: "assistant",
                content: "",
                created_at: new Date().toISOString(),
                duration_ms: null,
                exit_code: null,
                error: null,
              });
            }
          } else if (p.kind === "delta") {
            const delta = p.delta ?? "";
            if (idx >= 0) {
              messages[idx] = {
                ...messages[idx],
                content: messages[idx].content + delta,
              };
            } else {
              messages.push({
                id: p.message_id,
                role: "assistant",
                content: delta,
                created_at: new Date().toISOString(),
                duration_ms: null,
                exit_code: null,
                error: null,
              });
            }
          } else if (p.kind === "completed") {
            if (idx >= 0) {
              messages[idx] = {
                ...messages[idx],
                content: p.content ?? messages[idx].content,
              };
            }
          } else if (p.kind === "failed" || p.kind === "cancelled") {
            if (idx >= 0) {
              messages[idx] = {
                ...messages[idx],
                content: p.content ?? messages[idx].content,
                error: p.kind === "failed" ? (p.error ?? null) : messages[idx].error,
              };
            }
          }
          const stillBusy =
            p.kind === "started" || p.kind === "delta"
              ? session.busy
              : false;
          return { ...session, messages, busy: stillBusy };
        }),
      );
    }).then((fn) => {
      if (disposed) fn();
      else unlisten = fn;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [isOpen]);

  const selected =
    sessions.find((session) => session.id === selectedId) ?? null;

  useEffect(() => {
    setSessionProviderStatus(null);
    if (!isOpen || !selected || selected.kind !== "prompt") {
      return;
    }
    let disposed = false;
    invoke<ProviderStatus>("get_session_provider_status", {
      sessionId: selected.id,
    })
      .then((status) => {
        if (!disposed) setSessionProviderStatus(status);
      })
      .catch(() => {
        if (!disposed) setSessionProviderStatus(null);
      });
    return () => {
      disposed = true;
    };
  }, [isOpen, selected]);

  // Keep the "Cancelling…" label only until the backend confirms the turn
  // actually stopped.
  useEffect(() => {
    if (!cancellingId) return;
    const target = sessions.find((session) => session.id === cancellingId);
    if (!target || !target.busy) {
      setCancellingId(null);
    }
  }, [sessions, cancellingId]);

  if (!isOpen) return null;

  const promptNeedsCredential =
    selected?.kind === "prompt" &&
    sessionProviderStatus !== null &&
    Boolean(sessionProviderStatus.cloud_endpoint) &&
    !isLocalEndpointUrl(sessionProviderStatus.cloud_endpoint ?? "") &&
    !sessionProviderStatus.has_api_key;

  const hasUserRequirement =
    selected?.messages.some(
      (entry) =>
        entry.role === "user" &&
        entry.error === null &&
        entry.content.trim().length > 0,
    ) ?? false;
  const canPlanConversation =
    selected?.kind === "prompt" &&
    Boolean(selected.project_id) &&
    hasUserRequirement &&
    !selected.busy;

  async function planConversation() {
    if (!selected || planningConversation) return;
    setPlanningConversation(true);
    setError(null);
    try {
      const created = await invoke<TaskRecord>("start_task_from_session", {
        sessionId: selected.id,
        objective: null,
      });
      onTaskCreated(created);
    } catch (planError) {
      setError(describeError(planError));
    } finally {
      setPlanningConversation(false);
    }
  }

  async function createSession() {
    setCreating(true);
    setError(null);
    try {
      const created = await invoke<SessionRecord>("create_session", {
        options: {
          kind,
          title: title.trim() || null,
          project_id: projectId || null,
          provider_profile_id: profileId || null,
          cli_preset_id: kind === "cli" ? presetId || null : null,
          cli_executable: kind === "cli" ? executable.trim() || null : null,
          cli_args: null,
        },
      });
      setSessions((previous) => [created, ...previous]);
      setSelectedId(created.id);
      setTitle("");
      await onChanged();
    } catch (createError) {
      setError(describeError(createError));
    } finally {
      setCreating(false);
    }
  }

  async function sendMessage() {
    const text = message.trim();
    if (!selected || !text || sending || selected.busy) return;
    setSending(true);
    setError(null);
    setMessage("");
    try {
      const updated = await invoke<SessionRecord>("send_session_message", {
        sessionId: selected.id,
        content: text,
      });
      setSessions((previous) =>
        previous.map((session) =>
          session.id === updated.id ? updated : session,
        ),
      );
    } catch (sendError) {
      setError(describeError(sendError));
      setMessage(text);
      await load();
    } finally {
      setSending(false);
    }
  }

  async function cancelTurn() {
    if (!selected || !selected.busy) return;
    if (selected.kind !== "cli" && selected.kind !== "prompt") return;
    const targetId = selected.id;
    setCancellingId(targetId);
    setError(null);
    try {
      const cmd =
        selected.kind === "cli"
          ? "cancel_cli_session_turn"
          : "cancel_prompt_session_turn";
      await invoke<SessionRecord>(cmd, {
        sessionId: targetId,
      });
    } catch (cancelError) {
      setCancellingId(null);
      setError(describeError(cancelError));
    }
  }

  async function deleteSession(sessionId: string) {
    try {
      await invoke("delete_session", { sessionId });
      setSessions((previous) =>
        previous.filter((session) => session.id !== sessionId),
      );
      if (selectedId === sessionId) setSelectedId(null);
      await onChanged();
    } catch (deleteError) {
      setError(describeError(deleteError));
    }
  }

  return (
    <div className="config-modal-backdrop" onClick={onClose}>
      <div
        className="config-modal sessions-modal"
        onClick={(event) => event.stopPropagation()}
      >
        <div className="config-modal-header">
          <h2>Independent Sessions</h2>
          <button className="icon-button small" onClick={onClose} title="Close">
            ×
          </button>
        </div>

        <div className="sessions-layout">
          <div className="sessions-list">
            {sessions.map((session) => (
              <button
                key={session.id}
                type="button"
                className={`session-item ${
                  selectedId === session.id ? "selected" : ""
                }`}
                onClick={() => setSelectedId(session.id)}
              >
                <span className="session-item-title">
                  {session.kind === "cli" ? "⌘" : "✳"} {session.title}
                </span>
                <small>
                  {session.busy
                    ? "working…"
                    : `${session.messages.length} messages`}
                </small>
              </button>
            ))}
            {sessions.length === 0 && (
              <p className="muted-note">No sessions yet. Create one below.</p>
            )}

            <div className="session-create">
              <div className="form-row">
                <label>
                  Type
                  <select
                    value={kind}
                    onChange={(event) =>
                      setKind(event.target.value as SessionKind)
                    }
                  >
                    <option value="prompt">Prompt AI</option>
                    <option value="cli">CLI coding agent</option>
                  </select>
                </label>
                <label>
                  Project
                  <select
                    value={projectId}
                    onChange={(event) => setProjectId(event.target.value)}
                  >
                    <option value="">No project</option>
                    {projects.map((project) => (
                      <option key={project.id} value={project.id}>
                        {project.name}
                      </option>
                    ))}
                  </select>
                </label>
              </div>
              <label>
                Title
                <input
                  value={title}
                  onChange={(event) => setTitle(event.target.value)}
                  placeholder="Optional title"
                />
              </label>
              {kind === "prompt" ? (
                <label>
                  Provider profile
                  <select
                    value={profileId}
                    onChange={(event) => setProfileId(event.target.value)}
                  >
                    <option value="">Project default</option>
                    {profiles.map((profile) => (
                      <option key={profile.id} value={profile.id}>
                        {profile.name}
                      </option>
                    ))}
                  </select>
                </label>
              ) : (
                <>
                  <label>
                    CLI agent
                    <select
                      value={presetId}
                      onChange={(event) => setPresetId(event.target.value)}
                    >
                      {cliPresets.map((preset) => (
                        <option key={preset.id} value={preset.id}>
                          {preset.name}
                          {preset.available ? "" : " (not found)"}
                        </option>
                      ))}
                    </select>
                  </label>
                  <label>
                    Executable override
                    <input
                      value={executable}
                      onChange={(event) => setExecutable(event.target.value)}
                      placeholder="Use preset default"
                    />
                  </label>
                </>
              )}
              <button
                type="button"
                className="primary-button full-width"
                onClick={() => void createSession()}
                disabled={creating}
              >
                {creating ? "Creating..." : "Create session"}
              </button>
            </div>
          </div>

          <div className="session-detail">
            {selected ? (
              <>
                <div className="session-detail-header">
                  <div>
                    <strong>{selected.title}</strong>
                    <small>
                      {selected.kind === "cli"
                        ? `CLI · ${selected.cli_executable ?? "no executable"}`
                        : `Prompt · ${
                            profiles.find(
                              (profile) =>
                                profile.id === selected.provider_profile_id,
                            )?.name ?? "default profile"
                          }`}
                    </small>
                  </div>
                  <div className="session-detail-actions">
                    {selected.kind === "prompt" && (
                      <button
                        type="button"
                        className="primary-button compact"
                        onClick={() => void planConversation()}
                        disabled={!canPlanConversation || planningConversation}
                        title={
                          !selected.project_id
                            ? "Create a Prompt session with a project selected before planning the conversation."
                            : !hasUserRequirement
                              ? "Send a message describing what you want before planning."
                              : "Create a task from this conversation. The plan still needs approval."
                        }
                      >
                        {planningConversation
                          ? "Creating task…"
                          : "Plan this conversation"}
                      </button>
                    )}
                    {(selected.kind === "cli" || selected.kind === "prompt") &&
                      selected.busy && (
                      <button
                        type="button"
                        className="secondary-button compact"
                        onClick={() => void cancelTurn()}
                        disabled={cancellingId === selected.id}
                        title="Stop the running session."
                      >
                        {cancellingId === selected.id ? "Cancelling…" : "Cancel"}
                      </button>
                    )}
                    <button
                      type="button"
                      className="secondary-button compact"
                      onClick={() => void deleteSession(selected.id)}
                      disabled={planningConversation}
                    >
                      Delete
                    </button>
                  </div>
                </div>

                {selected.kind === "prompt" && (
                  <p className="muted-note session-handoff-note">
                    Planning uses this conversation as context and creates a task
                    through the existing planner. You review and approve the plan
                    before any CLI work starts.
                  </p>
                )}

                <div className="session-messages">
                  {selected.messages.length === 0 ? (
                    <p className="muted-note">
                      {selected.kind === "cli"
                        ? "Send a prompt to run this coding agent in the project folder."
                        : "Ask anything — this session keeps its own conversation history."}
                    </p>
                  ) : (
                    selected.messages.map((entry) => (
                      <div
                        key={entry.id}
                        className={`session-message ${entry.role}`}
                      >
                        <div className="message-label">
                          {entry.role === "user"
                            ? "YOU"
                            : entry.role === "assistant"
                              ? "ASSISTANT"
                              : "SYSTEM"}
                        </div>
                        <p>{entry.content}</p>
                        {entry.duration_ms !== null && (
                          <small>
                            {(entry.duration_ms / 1000).toFixed(1)}s
                            {entry.exit_code !== null
                              ? ` · exit ${entry.exit_code}`
                              : ""}
                          </small>
                        )}
                      </div>
                    ))
                  )}
                  {selected.busy && (
                    <div className="session-busy">
                      {cancellingId === selected.id
                        ? "Cancelling…"
                        : "Waiting for response…"}
                    </div>
                  )}
                </div>

                {error && <div className="inline-error">{error}</div>}

                {promptNeedsCredential && (
                  <div className="config-warning">
                    No API key is available for this Prompt session's provider
                    profile. Keys are memory-only and are cleared on restart.{" "}
                    <button
                      type="button"
                      className="secondary-button compact"
                      onClick={onConfigureProviders}
                    >
                      Open Provider Profiles
                    </button>{" "}
                    to re-enter it, or set PLANNER_API_KEY / OPENAI_API_KEY /
                    GEMINI_API_KEY. Your conversation will be kept.
                  </div>
                )}

                <div className="session-composer">
                  <textarea
                    value={message}
                    onChange={(event) => setMessage(event.target.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter" && !event.shiftKey) {
                        event.preventDefault();
                        void sendMessage();
                      }
                    }}
                    placeholder="Message this session…"
                    rows={3}
                  />
                  <button
                    type="button"
                    className="send-button"
                    onClick={() => void sendMessage()}
                    disabled={
                      !message.trim() ||
                      sending ||
                      selected.busy ||
                      planningConversation
                    }
                  >
                    {sending ? "…" : "↑"}
                  </button>
                </div>
              </>
            ) : (
              <p className="muted-note">
                Select or create a session to start an independent conversation.
              </p>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}


function App() {
  const [sampleProject, setSampleProject] = useState(false);
  const [projectPath, setProjectPath] = useState<string | null>(null);
  const [expandedPaths, setExpandedPaths] = useState<Set<string>>(
    new Set(),
  );
  const [dirChildren, setDirChildren] = useState<Record<string, FileEntry[]>>({});
  const [dirLoading, setDirLoading] = useState<Record<string, boolean>>({});
  const [dirErrors, setDirErrors] = useState<Record<string, string>>({});
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [rootExpanded, setRootExpanded] = useState(true);

  const [terminalOpen, setTerminalOpen] = useState(true);
  const [activeTerminal, setActiveTerminal] = useState("TERMINAL");
  // Mounted lazily the first time the SHELL tab is opened, then kept mounted
  // (hidden) so background sessions survive tab/panel switches.
  const [shellMounted, setShellMounted] = useState(false);
  const [prompt, setPrompt] = useState("");
  const [messages, setMessages] = useState<Message[]>([]);
  const [notice, setNotice] = useState("");

  const [task, setTask] = useState<TaskRecord | null>(null);
  const [taskLogs, setTaskLogs] = useState<string[]>([]);
  const [executionEvents, setExecutionEvents] = useState<ExecutionEvent[]>([]);
  const [providerStatus, setProviderStatus] = useState<ProviderStatus | null>(
    null,
  );
  const [pendingAction, setPendingAction] = useState<PendingAction | null>(null);
  const [configModalOpen, setConfigModalOpen] = useState(false);
  const [historyOpen, setHistoryOpen] = useState(false);
  const [notificationsOpen, setNotificationsOpen] = useState(false);
  const [activityOpen, setActivityOpen] = useState(false);
  const [unreadNotifications, setUnreadNotifications] = useState(0);
  const [taskReportOpen, setTaskReportOpen] = useState(false);
  const [taskReport, setTaskReport] = useState<TaskReport | null>(null);
  const [projectReportOpen, setProjectReportOpen] = useState(false);
  const [projectReport, setProjectReport] = useState<ProjectReport | null>(null);
  const [instructionsOpen, setInstructionsOpen] = useState(false);
  const [skillsOpen, setSkillsOpen] = useState(false);
  const [tokenUsageOpen, setTokenUsageOpen] = useState(false);
  const [modelPrefsOpen, setModelPrefsOpen] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);
  const [composerError, setComposerError] = useState<string | null>(null);
  const [syncError, setSyncError] = useState<string | null>(null);
  const [rejectReason, setRejectReason] = useState("");
  const [showRejectForm, setShowRejectForm] = useState(false);
  const [confirmCancel, setConfirmCancel] = useState(false);
  const [workspaceState, setWorkspaceState] = useState<WorkspaceSnapshot | null>(
    null,
  );
  const [cliPresets, setCliPresets] = useState<CliAgentPreset[]>([]);
  const [profilesOpen, setProfilesOpen] = useState(false);
  const [sessionsOpen, setSessionsOpen] = useState(false);
  const [agentsOpen, setAgentsOpen] = useState(false);
  const [taskProfileId, setTaskProfileId] = useState("");
  const [taskAgentPresetId, setTaskAgentPresetId] = useState("");
  const [taskPriority, setTaskPriority] = useState(0);
  const terminalBodyRef = useRef<HTMLDivElement | null>(null);

  const isTestMode = providerStatus?.is_test_mode ?? false;
  const taskActive = task !== null && !isTerminalTask(task);
  const awaitingApproval = task?.state === "awaiting_approval";
  const busy = pendingAction !== null;
  const problems = task ? collectTaskProblems(task) : [];
  const executorOutput = latestExecutorOutput(task);
  const activeProject =
    workspaceState?.projects.find((project) => project.root === projectPath) ??
    null;
  const profiles = workspaceState?.provider_profiles ?? [];

  const loadTaskReport = useCallback(async (taskId: string) => {
    try {
      const report = await invoke<TaskReport>("get_task_report", { taskId });
      setTaskReport(report);
      setTaskReportOpen(true);
    } catch (error) {
      console.error("Failed to load task report:", error);
    }
  }, []);

  const loadProjectReport = useCallback(async (projectId: string) => {
    try {
      const report = await invoke<ProjectReport>("get_project_report", {
        projectId,
      });
      setProjectReport(report);
      setProjectReportOpen(true);
    } catch (error) {
      console.error("Failed to load project report:", error);
    }
  }, []);

  const refreshWorkspace = useCallback(async () => {
    try {
      const snapshot = await invoke<WorkspaceSnapshot>("get_workspace");
      setWorkspaceState(snapshot);
    } catch (error) {
      console.error("Failed to read the workspace:", error);
    }
  }, []);

  const applyTask = useCallback((incoming: TaskRecord | null) => {
    if (!incoming) return;
    setTask((previous) => {
      if (!previous) return incoming;
      if (previous.id === incoming.id) {
        // Ignore stale or identical snapshots; the backend is the source of
        // truth, but event and invoke responses can arrive out of order.
        if (incoming.updated_at > previous.updated_at) return incoming;
        return previous;
      }
      // Prefer a task that still needs attention over a finished one, so a
      // queued/awaiting task can never be hidden by another task completing.
      const previousActive = !isTerminalTask(previous);
      const incomingActive = !isTerminalTask(incoming);
      if (incomingActive && !previousActive) return incoming;
      if (!incomingActive && previousActive) return previous;
      if (incomingActive && previousActive) {
        return incoming.updated_at > previous.updated_at ? incoming : previous;
      }
      return incoming.created_at > previous.created_at ? incoming : previous;
    });
  }, []);

  const refreshCurrentTask = useCallback(
    async (silent = false) => {
      try {
        const current = await invoke<TaskRecord | null>("get_task_status", {
          taskId: null,
        });
        if (current) applyTask(current);
        setSyncError(null);
      } catch (error) {
        if (!silent) setSyncError(describeError(error));
      }
    },
    [applyTask],
  );

  useEffect(() => {
    let disposed = false;
    const unlisteners: UnlistenFn[] = [];

    const subscribe = async () => {
      const [
        unlistenUpdate,
        unlistenLog,
        unlistenWorkspace,
        unlistenEvents,
        unlistenNotif,
      ] = await Promise.all([
        listen<TaskRecord>("orchestration_update", (event) => {
          applyTask(event.payload);
        }),
        listen<string>("orchestration_log", (event) => {
          setTaskLogs((previous) =>
            [...previous, event.payload].slice(-400),
          );
        }),
        listen<WorkspaceSnapshot>("workspace_update", (event) => {
          setWorkspaceState(event.payload);
        }),
        listen<ExecutionEvent>("orchestration_event", (event) => {
          setExecutionEvents((previous) => {
            const next = [...previous, event.payload];
            return next.slice(-200);
          });
        }),
        listen<NotificationRecord>("notification_created", () => {
          setUnreadNotifications((c) => c + 1);
        }),
      ]);

      if (disposed) {
        unlistenUpdate();
        unlistenLog();
        unlistenWorkspace();
        unlistenEvents();
        unlistenNotif();
        return;
      }

      unlisteners.push(
        unlistenUpdate,
        unlistenLog,
        unlistenWorkspace,
        unlistenEvents,
        unlistenNotif,
      );
    };

    subscribe().catch((error) => {
      console.error("Failed to subscribe to orchestration events:", error);
      setSyncError("Could not subscribe to live orchestration updates.");
    });

    return () => {
      disposed = true;
      unlisteners.forEach((unlisten) => unlisten());
    };
  }, [applyTask]);

  useEffect(() => {
    let disposed = false;

    const load = async () => {
      try {
        const status = await invoke<ProviderStatus>("get_provider_status");
        if (!disposed) setProviderStatus(status);
      } catch (error) {
        console.error("Failed to read provider status:", error);
      }

      try {
        const [snapshot, presets] = await Promise.all([
          invoke<WorkspaceSnapshot>("get_workspace"),
          invoke<CliAgentPreset[]>("list_cli_agent_presets"),
        ]);
        if (!disposed) {
          setWorkspaceState(snapshot);
          setCliPresets(presets);
        }
      } catch (error) {
        console.error("Failed to read the workspace:", error);
      }

      try {
        const current = await invoke<TaskRecord | null>("get_task_status", {
          taskId: null,
        });
        if (!disposed && current) applyTask(current);
      } catch (error) {
        console.error("Failed to read the current task:", error);
        if (!disposed) setSyncError(describeError(error));
      }
    };

    void load();

    return () => {
      disposed = true;
    };
  }, [applyTask]);

  // Keep the UI aligned with the backend while work is in flight. Event
  // delivery is the fast path; polling covers dropped or coalesced events.
  useEffect(() => {
    if (!taskActive) return;
    const interval = window.setInterval(() => {
      void refreshCurrentTask();
    }, 1500);
    return () => window.clearInterval(interval);
  }, [taskActive, refreshCurrentTask]);

  // Re-synchronise whenever the window regains focus or becomes visible.
  useEffect(() => {
    const resync = () => {
      void refreshCurrentTask();
    };
    const onVisibilityChange = () => {
      if (document.visibilityState === "visible") resync();
    };
    window.addEventListener("focus", resync);
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      window.removeEventListener("focus", resync);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [refreshCurrentTask]);

  useEffect(() => {
    setConfirmCancel(false);
    setShowRejectForm(false);
    setRejectReason("");
    setActionError(null);
  }, [task?.id, task?.state]);

  useEffect(() => {
    if (terminalOpen && activeTerminal === "TERMINAL") {
      const body = terminalBodyRef.current;
      if (body) body.scrollTop = body.scrollHeight;
    }
  }, [taskLogs.length, activeTerminal, terminalOpen]);

  const loadDirectory = useCallback(
    async (root: string, targetPath?: string | null) => {
      const key = targetPath || root;
      setDirLoading((prev) => ({ ...prev, [key]: true }));
      setDirErrors((prev) => {
        const next = { ...prev };
        delete next[key];
        return next;
      });

      try {
        const entries = await invoke<FileEntry[]>("list_directory", {
          projectRoot: root,
          directoryPath: targetPath || null,
        });

        setDirChildren((prev) => ({ ...prev, [key]: entries }));
      } catch (error) {
        console.error("Failed to list directory:", error);
        const msg =
          typeof error === "string"
            ? error
            : error instanceof Error
            ? error.message
            : "Failed to load directory";
        setDirErrors((prev) => ({ ...prev, [key]: msg }));
      } finally {
        setDirLoading((prev) => ({ ...prev, [key]: false }));
      }
    },
    [],
  );

  function handleToggleDir(path: string) {
    setExpandedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
        if (projectPath && (!dirChildren[path] || dirErrors[path])) {
          loadDirectory(projectPath, path);
        }
      }
      return next;
    });
  }

  function handleRetryDir(path: string) {
    if (projectPath) {
      loadDirectory(projectPath, path === projectPath ? null : path);
    }
  }

  function handleSelectFile(entry: FileEntry) {
    setSelectedPath(entry.path);
    showNotice(`Selected file: ${entry.name}`);
  }

  function refreshExplorer() {
    if (!projectPath) {
      showNotice("Open a project to refresh.");
      return;
    }
    loadDirectory(projectPath, null);
    expandedPaths.forEach((path) => {
      if (path !== projectPath) {
        loadDirectory(projectPath, path);
      }
    });
    showNotice("Refreshed workspace.");
  }

  function appendMessage(role: Message["role"], content: string) {
    setMessages((previous) => [...previous, { role, content }]);
  }

  async function sendMessage() {
    const text = prompt.trim();
    setComposerError(null);

    if (!text) {
      setComposerError("Describe the task before sending.");
      return;
    }
    if (!projectPath) {
      setComposerError("Open a project folder before starting a task.");
      return;
    }
    if (busy) return;

    const agentPreset = cliPresets.find(
      (preset) => preset.id === taskAgentPresetId,
    );

    setPendingAction("start");
    appendMessage("user", text);
    setPrompt("");

    try {
      const started = await invoke<TaskRecord>("start_task", {
        userTask: text,
        projectRoot: projectPath,
        options: {
          project_id: activeProject?.id ?? null,
          provider_profile_id: taskProfileId || null,
          cli_executable: agentPreset?.default_executable || null,
          cli_args: agentPreset ? agentPreset.default_args : null,
          prompt_delivery: agentPreset?.prompt_delivery ?? null,
          priority: taskPriority,
        },
      });
      applyTask(started);
      setTaskLogs([]);
      appendMessage(
        "assistant",
        started.state === "queued"
          ? `Task ${shortId(started.id)} queued. It starts automatically when a task slot frees up.`
          : `Task ${shortId(started.id)} handed to the orchestrator. The planner is drafting a plan for you to approve.`,
      );
      void refreshWorkspace();
    } catch (error) {
      const message = describeError(error);
      setComposerError(message);
      appendMessage("assistant", `Could not start the task: ${message}`);
      setPrompt(text);
      // The backend may still hold an active task the UI did not know about.
      void refreshCurrentTask(true);
    } finally {
      setPendingAction(null);
    }
  }

  async function handleRetryTask(taskId: string) {
    setActionError(null);
    try {
      const retried = await invoke<TaskRecord>("retry_task", { taskId });
      applyTask(retried);
      await refreshWorkspace();
      showNotice(
        retried.state === "executing"
          ? "Task retried with its saved plan."
          : "Task queued to run again.",
      );
    } catch (error) {
      setActionError(`Could not retry the task: ${describeError(error)}`);
    }
  }

  function handleSessionTaskCreated(created: TaskRecord) {
    applyTask(created);
    setTaskLogs([]);
    setSessionsOpen(false);
    void refreshWorkspace();
    showNotice(
      `Task ${shortId(created.id)} created from the Prompt conversation. Review and approve its plan before execution.`,
    );
  }

  async function handleApproveTask(taskId: string) {
    setActionError(null);
    setPendingAction("approve");
    try {
      const approved = await invoke<TaskRecord>("approve_plan", { taskId });
      applyTask(approved);
      await refreshWorkspace();
      appendMessage("assistant", "Plan approved from history.");
      setShowRejectForm(false);
      setRejectReason("");
    } catch (error) {
      setActionError(`Could not approve from history: ${describeError(error)}`);
    } finally {
      setPendingAction(null);
    }
  }

  async function handleRejectTask(taskId: string, reason?: string | null) {
    setActionError(null);
    setPendingAction("reject");
    try {
      const rejected = await invoke<TaskRecord>("reject_plan", {
        taskId,
        reason: reason?.trim() || null,
      });
      applyTask(rejected);
      await refreshWorkspace();
      appendMessage(
        "assistant",
        reason?.trim()
          ? `Plan rejected from history: ${reason.trim()}`
          : "Plan rejected from history.",
      );
      setShowRejectForm(false);
      setRejectReason("");
    } catch (error) {
      setActionError(`Could not reject the plan from history: ${describeError(error)}`);
    } finally {
      setPendingAction(null);
    }
  }

  async function handleCancelTask(taskId: string) {
    setActionError(null);
    setPendingAction("cancel");
    try {
      const cancelled = await invoke<TaskRecord>("cancel_task", { taskId });
      applyTask(cancelled);
      await refreshWorkspace();
      appendMessage("assistant", "Task cancelled from history.");
      setShowRejectForm(false);
      setRejectReason("");
    } catch (error) {
      setActionError(`Could not cancel from history: ${describeError(error)}`);
    } finally {
      setPendingAction(null);
    }
  }

  function switchProject(project: ProjectRecord) {
    setProjectPath(project.root);
    setSampleProject(false);
    setExpandedPaths(new Set([project.root]));
    setDirChildren({});
    setDirErrors({});
    setSelectedPath(null);
    setRootExpanded(true);
    setTaskProfileId(project.provider_profile_id ?? "");
    loadDirectory(project.root, null);
    showNotice(`Switched to ${project.name}`);
  }

  async function handleApprove() {
    if (!task || busy) return;
    setPendingAction("approve");
    setActionError(null);
    try {
      applyTask(await invoke<TaskRecord>("approve_plan", { taskId: task.id }));
      appendMessage(
        "assistant",
        "Plan approved. The executor is now working through the plan.",
      );
    } catch (error) {
      setActionError(`Could not approve the plan: ${describeError(error)}`);
    } finally {
      setPendingAction(null);
    }
  }

  async function handleReject() {
    if (!task || busy) return;
    setPendingAction("reject");
    setActionError(null);
    try {
      const reason = rejectReason.trim() || null;
      applyTask(
        await invoke<TaskRecord>("reject_plan", {
          taskId: task.id,
          reason,
        }),
      );
      setShowRejectForm(false);
      setRejectReason("");
      appendMessage("assistant", "Plan rejected. The task was cancelled.");
    } catch (error) {
      setActionError(`Could not reject the plan: ${describeError(error)}`);
    } finally {
      setPendingAction(null);
    }
  }

  async function handleCancel() {
    if (!task || busy) return;
    setPendingAction("cancel");
    setActionError(null);
    try {
      applyTask(await invoke<TaskRecord>("cancel_task", { taskId: task.id }));
      appendMessage("assistant", "Task cancelled. No further work will run.");
    } catch (error) {
      setActionError(`Could not cancel the task: ${describeError(error)}`);
    } finally {
      setConfirmCancel(false);
      setPendingAction(null);
    }
  }

  function showNotice(text: string) {
    setNotice(text);
  }

  async function openProjectFolder() {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: "Open Project Folder",
      });

      if (typeof selected === "string") {
        setProjectPath(selected);
        setSampleProject(false);
        setExpandedPaths(new Set([selected]));
        setDirChildren({});
        setDirErrors({});
        setSelectedPath(null);
        setRootExpanded(true);

        const folderName =
          selected.split(/[\\/]/).filter(Boolean).pop() ?? selected;

        // Register the folder as a workspace project (ignore "already added").
        try {
          await invoke<ProjectRecord>("add_project", {
            root: selected,
            name: folderName,
          });
          await refreshWorkspace();
        } catch (error) {
          const message = describeError(error);
          if (!message.includes("already added")) {
            console.error("Failed to register project:", error);
          }
        }

        showNotice(`Selected folder: ${folderName}`);
        loadDirectory(selected, null);
      }
    } catch (error) {
      console.error("Failed to open folder:", error);
      showNotice("Could not open the folder picker.");
    }
  }

  return (
    <main className="app-shell">
      <header className="topbar">
        <div className="brand">
          <div className="brand-mark">P</div>
          <span>
            Personal<span className="brand-muted">AICodingApp</span>
          </span>
        </div>

        <div className="workspace-status">
          <span className="status-dot" />
          {projectPath
            ? projectPath.split(/[\\/]/).filter(Boolean).pop()
            : sampleProject
              ? "Sample Project"
              : "No folder opened"}
        </div>

        <div className="topbar-actions">
          <button
            className="secondary-button"
            onClick={() => setSessionsOpen(true)}
            title="Independent AI and CLI sessions"
          >
            Sessions
          </button>
          <button
            className="secondary-button"
            onClick={() => setProfilesOpen(true)}
            title="Manage provider profiles"
          >
            Profiles
          </button>
          <button
            className="secondary-button"
            onClick={() => setAgentsOpen(true)}
            title="Manage detected and registered CLI coding agents"
          >
            Agents
          </button>
          <button
            className="secondary-button"
            onClick={() => setInstructionsOpen(true)}
            title="Manage global and project instructions"
          >
            Instructions
          </button>
          <button
            className="secondary-button"
            onClick={() => setSkillsOpen(true)}
            title="Manage reusable task-specific skills"
          >
            Skills
          </button>
          <button
            className="secondary-button"
            onClick={() => setHistoryOpen(true)}
            title="Browse past tasks"
          >
            History
          </button>
          <button
            className="secondary-button"
            onClick={() => setNotificationsOpen(true)}
            title="View notifications"
          >
            Notifications {unreadNotifications > 0 && <span style={{ background: "#ff8a8a", color: "#1a1d24", padding: "1px 6px", borderRadius: 10, fontSize: 10, fontWeight: 600, marginLeft: 6 }}>{unreadNotifications}</span>}
          </button>
          <button
            className="secondary-button"
            onClick={() => setActivityOpen(true)}
            title="View activity feed"
          >
            Activity
          </button>
          <button
            className="icon-button"
            title="Search"
            onClick={() =>
              showNotice(
                "Search will be available when project indexing is implemented.",
              )
            }
          >
            ⌕
          </button>
                    <button
            className="secondary-button"
            title="Model preferences (backend-owned)"
            onClick={() => setModelPrefsOpen(true)}
          >
            Models
          </button>
<button
            className="icon-button"
            title="Token Usage"
            onClick={() => setTokenUsageOpen(true)}
          >
            📊
          </button>
          <button
            className="icon-button"
            title="Settings"
            onClick={() => setConfigModalOpen(true)}
          >
            ⚙
          </button>
          <button className="secondary-button" onClick={openProjectFolder}>
            Open Folder
          </button>
        </div>
      </header>

      <div className="workspace">
        <aside className="explorer-panel">
          <div className="panel-heading">
            <span>EXPLORER</span>
            <button
              className="icon-button small"
              title="Refresh"
              onClick={refreshExplorer}
            >
              ↻
            </button>
          </div>

          <div className="section-label">PROJECTS</div>

          <div className="project-list">
            {(workspaceState?.projects ?? []).map((project) => {
              const profile = profiles.find(
                (candidate) => candidate.id === project.provider_profile_id,
              );
              return (
                <button
                  key={project.id}
                  type="button"
                  className={`project-item ${
                    project.root === projectPath ? "active" : ""
                  }`}
                  title={project.root}
                  onClick={() => switchProject(project)}
                >
                  <span className="project-name">
                    {project.pinned ? "★ " : ""}
                    {project.name}
                  </span>
                  <small>{profile ? profile.name : "Default provider"}</small>
                </button>
              );
            })}
            {workspaceState && workspaceState.projects.length === 0 && (
              <p className="muted-note project-empty">
                No projects yet. Open a folder to add one.
              </p>
            )}
            <button
              type="button"
              className="project-add"
              onClick={openProjectFolder}
            >
              ＋ Add project
            </button>
            {workspaceState && (
              <div className="workspace-runtime-note">
                {workspaceState.active_task_ids.length} active ·{" "}
                {workspaceState.queued_task_ids.length} queued ·{" "}
                {workspaceState.settings.max_concurrent_tasks} slot
                {workspaceState.settings.max_concurrent_tasks === 1 ? "" : "s"}
              </div>
            )}
          </div>

          <div className="section-label">WORKSPACE</div>

          <div className="explorer-body">
            {projectPath ? (
              <div className="file-tree">
                <button
                  type="button"
                  className="tree-root-header"
                  onClick={() => setRootExpanded((prev) => !prev)}
                  title={projectPath}
                >
                  <span className="tree-arrow">
                    {rootExpanded ? "▾" : "▸"}
                  </span>
                  <span className="tree-icon">
                    {rootExpanded ? "📂" : "📁"}
                  </span>
                  <span className="tree-name">
                    {projectPath.split(/[\\/]/).filter(Boolean).pop() ??
                      projectPath}
                  </span>
                </button>

                {rootExpanded && (
                  <>
                    {dirLoading[projectPath] && !dirChildren[projectPath] && (
                      <div
                        className="tree-status-sub"
                        style={{ paddingLeft: "24px" }}
                      >
                        Loading files...
                      </div>
                    )}

                    {dirErrors[projectPath] && (
                      <div
                        className="tree-status-sub tree-status-error"
                        style={{ paddingLeft: "24px" }}
                      >
                        <span>{dirErrors[projectPath]}</span>
                        <button
                          type="button"
                          onClick={() => handleRetryDir(projectPath)}
                        >
                          Retry
                        </button>
                      </div>
                    )}

                    {!dirLoading[projectPath] &&
                      !dirErrors[projectPath] &&
                      dirChildren[projectPath] &&
                      dirChildren[projectPath].length === 0 && (
                      <div
                        className="tree-status-sub"
                        style={{ paddingLeft: "24px" }}
                      >
                        (empty folder)
                      </div>
                    )}

                    {dirChildren[projectPath]?.map((entry) => (
                      <FileTreeItem
                        key={entry.path}
                        entry={entry}
                        depth={1}
                        projectRoot={projectPath}
                        expandedPaths={expandedPaths}
                        dirChildren={dirChildren}
                        dirLoading={dirLoading}
                        dirErrors={dirErrors}
                        selectedPath={selectedPath}
                        onToggleDir={handleToggleDir}
                        onSelectFile={handleSelectFile}
                        onRetryDir={handleRetryDir}
                      />
                    ))}
                  </>
                )}
              </div>
            ) : sampleProject ? (
              <div className="file-tree">
                <div className="tree-root">▾ 📁 sample-project</div>
                <div className="tree-item">📁 src</div>
                <div className="tree-item indent">
                  TS&nbsp; App.tsx
                </div>
                <div className="tree-item indent">
                  CSS&nbsp; App.css
                </div>
                <div className="tree-item indent">
                  TS&nbsp; main.tsx
                </div>
                <div className="tree-item">📁 public</div>
                <div className="tree-item">◇ package.json</div>
                <div className="tree-item">◇ README.md</div>
              </div>
            ) : (
              <div className="empty-explorer">
                <div className="empty-icon">▱</div>
                <p>No folder opened</p>
                <span>
                  Open a project to see its files here.
                </span>
                <button
                  className="primary-button full-width"
                  onClick={() => setSampleProject(true)}
                >
                  Load Sample Project
                </button>
              </div>
            )}
          </div>

          <div className="explorer-footer">
            <button
              onClick={() =>
                showNotice(
                  "File creation will be enabled after filesystem access is implemented.",
                )
              }
            >
              ＋ New File
            </button>
            <button onClick={refreshExplorer}>↻ Refresh</button>
          </div>
        </aside>

        <section className="main-panel">
          <div className="editor-tabs">
            <div className="editor-tab active">⌂ Welcome</div>
            <div className="editor-tab-spacer" />
            <button
              className="icon-button small"
              title="Toggle terminal"
              onClick={() => setTerminalOpen(!terminalOpen)}
            >
              ▤
            </button>
          </div>

          <div className="welcome-content">
            <div className="welcome-kicker">YOUR DEVELOPMENT WORKSPACE</div>
            <h1>
              Build something <span>remarkable.</span>
            </h1>
            <p className="welcome-subtitle">
              Your workspace for building software with an AI coding partner.
            </p>

            <div className="quick-actions">
              <button
                className="action-card"
                onClick={() =>
                  showNotice(
                    "The editor and file creation flow are coming in a later milestone.",
                  )
                }
              >
                <span className="action-icon blue">＋</span>
                <strong>New File</strong>
                <span>Create a source file</span>
              </button>
              <button className="action-card" onClick={openProjectFolder}>
                <span className="action-icon purple">▱</span>
                <strong>Open Directory</strong>
                <span>Open an existing project</span>
              </button>
              <button
                className="action-card"
                onClick={() =>
                  showNotice(
                    "Repository cloning will be implemented in a later milestone.",
                  )
                }
              >
                <span className="action-icon green">⑂</span>
                <strong>Clone Repository</strong>
                <span>Start from a Git repository</span>
              </button>
              <button
                className="action-card"
                onClick={() => document.getElementById("ai-prompt")?.focus()}
              >
                <span className="action-icon orange">✳</span>
                <strong>Ask AI Assistant</strong>
                <span>Plan your next change</span>
              </button>
            </div>

            {notice && (
              <div className="notice" role="status">
                <span>{notice}</span>
                <button
                  onClick={() => setNotice("")}
                  aria-label="Dismiss notification"
                >
                  ×
                </button>
              </div>
            )}

            <div className="getting-started">
              <div className="getting-started-title">GETTING STARTED</div>
              <div className="getting-row">
                <span className="step-number">1</span>
                <span>
                  <strong>Open your project</strong>
                  <small>
                    Choose the codebase you want to work on.
                  </small>
                </span>
              </div>
              <div className="getting-row">
                <span className="step-number">2</span>
                <span>
                  <strong>Describe your goal</strong>
                  <small>
                    Tell your AI assistant what you want to build.
                  </small>
                </span>
              </div>
              <div className="getting-row">
                <span className="step-number">3</span>
                <span>
                  <strong>Review every change</strong>
                  <small>Stay in control of edits and command execution.</small>
                </span>
              </div>
            </div>

            <div className="prototype-note">
              <span>ⓘ</span> The AI panel drives the Rust orchestrator: plan → approval → coding agent → verification → review. Live task progress, verification results, reviewer decisions, and history are shown here; file editing is not connected yet.
            </div>
          </div>

          <section
            className={
              terminalOpen ? "terminal-panel" : "terminal-panel terminal-panel-hidden"
            }
          >
              <div className="terminal-header">
                <div className="terminal-tabs">
                  {[
                    { id: "TERMINAL", label: "TERMINAL" },
                    { id: "SHELL", label: "SHELL" },
                    { id: "OUTPUT", label: "OUTPUT" },
                    {
                      id: "PROBLEMS",
                      label: `PROBLEMS (${problems.length})`,
                    },
                  ].map((tab) => (
                    <button
                      key={tab.id}
                      className={
                        activeTerminal === tab.id
                          ? "terminal-tab selected"
                          : "terminal-tab"
                      }
                      onClick={() => {
                        if (tab.id === "SHELL") setShellMounted(true);
                        setActiveTerminal(tab.id);
                      }}
                    >
                      {tab.label}
                    </button>
                  ))}
                </div>
                <button
                  className="icon-button small"
                  title="Close terminal"
                  onClick={() => setTerminalOpen(false)}
                >
                  ×
                </button>
              </div>

              {activeTerminal === "TERMINAL" && (
                <div
                  className="terminal-body terminal-body-scroll"
                  ref={terminalBodyRef}
                >
                  {taskLogs.length === 0 ? (
                    <div className="terminal-empty">
                      No orchestration output yet. Start a task to see live
                      planning, execution, verification, and review events.
                    </div>
                  ) : (
                    <div className="terminal-log">
                      {taskLogs.map((line, index) => (
                        <div key={index} className="terminal-log-line">
                          {line}
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}

              {activeTerminal === "OUTPUT" && (
                <div className="terminal-body terminal-body-scroll">
                  {executorOutput ? (
                    <pre className="terminal-pre terminal-pre-output">
                      {executorOutput}
                    </pre>
                  ) : (
                    <div className="terminal-empty">
                      No executor output yet. Output from the latest attempt
                      appears here after the coding agent finishes.
                    </div>
                  )}
                </div>
              )}

              {activeTerminal === "PROBLEMS" && (
                <div className="terminal-body terminal-body-scroll">
                  {problems.length === 0 ? (
                    <div className="terminal-empty">
                      No problems reported. Failed verifications, reviewer
                      issues, and task errors appear here.
                    </div>
                  ) : (
                    <div className="problem-list">
                      {problems.map((problem, index) => (
                        <div key={index} className="problem-item">
                          <div className="problem-title">{problem.title}</div>
                          <pre className="problem-detail">{problem.detail}</pre>
                        </div>
                      ))}
                    </div>
                  )}
                </div>
              )}

              {shellMounted && (
                <div
                  className={
                    activeTerminal === "SHELL"
                      ? "terminal-shell-pane"
                      : "terminal-shell-pane hidden"
                  }
                >
                  <Suspense
                    fallback={
                      <div className="terminal-shell-loading">
                        Loading terminal…
                      </div>
                    }
                  >
                    <TerminalWorkspace
                      cwd={projectPath}
                      active={terminalOpen && activeTerminal === "SHELL"}
                    />
                  </Suspense>
                </div>
              )}
            </section>
        </section>

        <aside className="ai-panel">
          <div className="ai-header">
            <div>
              <span className="ai-spark">✳</span>
              <strong>AI Assistant</strong>
            </div>
            <div className="ai-header-badges">
              {isTestMode && (
                <span className="beta-tag test-mode">TEST MODE</span>
              )}
              <span className="beta-tag">PREVIEW</span>
            </div>
          </div>

          <div className="chat-area">
            {messages.length === 0 ? (
              <div className="chat-empty">
                <div className="ai-orb">✳</div>
                <h2>What are we building?</h2>
                <p>
                  Describe a task, ask a question, or plan your next feature.
                </p>
                <div className="suggestion-list">
                  {[
                    "Explain this codebase",
                    "Help me plan a new feature",
                    "Find a bug in my code",
                  ].map((suggestion) => (
                    <button
                      key={suggestion}
                      onClick={() => setPrompt(suggestion)}
                    >
                      <span>↳</span>
                      {suggestion}
                      <span className="suggestion-arrow">↗</span>
                    </button>
                  ))}
                </div>
              </div>
            ) : (
              <div className="message-list">
                {messages.map((message, index) => (
                  <div key={index} className={`message ${message.role}`}>
                    <div className="message-label">
                      {message.role === "user"
                        ? "YOU"
                        : "AI ASSISTANT"}
                    </div>
                    <p>{message.content}</p>
                  </div>
                ))}
              </div>
            )}

            {task && (
              <div className="task-panel">
                <div className="task-panel-head">
                  <span
                    className={`task-state ${taskStateClass(task.state)}`}
                  >
                    {TASK_STATE_LABELS[task.state]}
                  </span>
                  <span className="task-progress">
                    {taskProgressLabel(task)}
                  </span>
                  {taskActive && !confirmCancel && (
                    <button
                      type="button"
                      className="task-cancel"
                      onClick={() => setConfirmCancel(true)}
                      disabled={busy}
                    >
                      Cancel
                    </button>
                  )}
                  {taskActive && confirmCancel && (
                    <span className="task-cancel-confirm">
                      <span>Cancel this task?</span>
                      <button
                        type="button"
                        className="danger-button compact"
                        onClick={handleCancel}
                        disabled={busy}
                      >
                        {pendingAction === "cancel" ? "Cancelling..." : "Yes, cancel"}
                      </button>
                      <button
                        type="button"
                        className="secondary-button compact"
                        onClick={() => setConfirmCancel(false)}
                        disabled={busy}
                      >
                        Keep running
                      </button>
                    </span>
                  )}
                </div>

                <div className="task-meta">
                  Task {shortId(task.id)} · {projectName(task.project_root)}
                  {task.task_graph
                    ? ` · ${task.task_graph.components.length} steps`
                    : ` · max ${task.max_iterations} iterations`}
                </div>

                <ExecutionMonitor
                  task={task}
                  events={executionEvents.filter((e) => e.task_id === task.id)}
                />

                {syncError && (
                  <div className="inline-error sync-error">
                    <span>State sync failed: {syncError}</span>
                    <button
                      type="button"
                      className="secondary-button compact"
                      onClick={() => void refreshCurrentTask()}
                    >
                      Retry
                    </button>
                  </div>
                )}

                {actionError && (
                  <div className="inline-error">{actionError}</div>
                )}

                {task.error && !isTerminalTask(task) && (
                  <div className="inline-error">{task.error}</div>
                )}

                {task.plan && (
                  <PlanCard
                    plan={task.plan}
                    awaitingApproval={awaitingApproval}
                    busy={busy}
                    pendingAction={pendingAction}
                    showRejectForm={showRejectForm}
                    rejectReason={rejectReason}
                    onApprove={handleApprove}
                    onStartReject={() => setShowRejectForm(true)}
                    onCancelReject={() => {
                      setShowRejectForm(false);
                      setRejectReason("");
                    }}
                    onReject={handleReject}
                    onRejectReasonChange={setRejectReason}
                    showSteps={!task.task_graph}
                  />
                )}

                {task.task_graph && <GraphCard graph={task.task_graph} />}

                {isTerminalTask(task) && <TaskResultCard task={task} />}

                <IterationTimeline task={task} />

                {taskLogs.length > 0 && (
                  <details className="task-log-details">
                    <summary>
                      Orchestrator log ({taskLogs.length} lines)
                    </summary>
                    <div className="task-logs">
                      {taskLogs.map((line, index) => (
                        <div key={index} className="task-log-line">
                          {line}
                        </div>
                      ))}
                    </div>
                  </details>
                )}
              </div>
            )}
          </div>

          <div className="composer">
            <label htmlFor="ai-prompt">MESSAGE</label>
            <textarea
              id="ai-prompt"
              value={prompt}
              onChange={(event) => {
                setPrompt(event.target.value);
                if (composerError) setComposerError(null);
              }}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
                  void sendMessage();
                }
              }}
              placeholder={
                projectPath
                  ? "Describe the change you want the orchestrator to make..."
                  : "Open a project folder to start a task..."
              }
              rows={4}
            />

            {composerError && (
              <div className="composer-error">{composerError}</div>
            )}
            {!projectPath && (
              <button
                type="button"
                className="secondary-button composer-open-folder"
                onClick={openProjectFolder}
              >
                Open project folder
              </button>
            )}

            <div className="composer-options">
              <label>
                Provider
                <select
                  value={taskProfileId}
                  onChange={(event) => setTaskProfileId(event.target.value)}
                >
                  <option value="">Project default</option>
                  {profiles.map((profile) => (
                    <option key={profile.id} value={profile.id}>
                      {profile.name}
                      {profile.project_id ? " (project)" : ""}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Agent
                <select
                  value={taskAgentPresetId}
                  onChange={(event) =>
                    setTaskAgentPresetId(event.target.value)
                  }
                >
                  <option value="">Profile default</option>
                  {cliPresets.map((preset) => (
                    <option key={preset.id} value={preset.id}>
                      {preset.name}
                      {preset.available ? "" : " (not found)"}
                    </option>
                  ))}
                </select>
              </label>
              <label>
                Priority
                <select
                  value={taskPriority}
                  onChange={(event) =>
                    setTaskPriority(Number(event.target.value))
                  }
                >
                  <option value={0}>Normal</option>
                  <option value={1}>Low</option>
                  <option value={5}>High</option>
                  <option value={9}>Urgent</option>
                </select>
              </label>
            </div>

            <div className="composer-footer">
              <span>
                {pendingAction === "start"
                  ? "Starting task..."
                  : !projectPath
                    ? "No project folder opened."
                    : workspaceState?.settings.scheduler_paused
                      ? "Scheduler paused — new tasks wait in the queue."
                      : "Shift + Enter for a new line"}
              </span>
              <button
                className="send-button"
                onClick={() => void sendMessage()}
                disabled={!prompt.trim() || busy}
                aria-label="Send message"
              >
                {pendingAction === "start" ? "…" : "↑"}
              </button>
            </div>
            <div className="ai-disclaimer">
              {isTestMode
                ? "TEST MODE — the orchestrator is running its simulated test providers. No cloud AI and no coding-agent CLI are used."
                : providerStatus
                  ? `Planner: ${providerStatus.planner_type} · Executor: ${providerStatus.executor_type} · Max iterations: ${providerStatus.max_iterations}`
                  : "Orchestrator status unavailable."}
            </div>
          </div>
        </aside>
      </div>

      <footer className="statusbar">
        <div>
          <span className="statusbar-dot" />
          {task ? TASK_STATE_LABELS[task.state] : "Ready"}
          {syncError && (
            <>
              <span className="status-separator"> | </span>
              <span className="status-sync-error">out of sync</span>
            </>
          )}
          <span className="status-separator"> | </span>
          {isTestMode
            ? "Test mode"
            : providerStatus
              ? `${providerStatus.planner_type} + ${providerStatus.executor_type}`
              : "Prototype mode"}
        </div>
        <div>
          UTF-8 <span className="status-separator"> | </span> TypeScript
          React{" "}
          <span className="status-separator"> | </span> Tauri
        </div>
      </footer>

      <ProviderConfigModal
        isOpen={configModalOpen}
        onClose={() => setConfigModalOpen(false)}
        providerStatus={providerStatus}
        onSaved={(status) => setProviderStatus(status)}
      />

      <TaskHistoryModal
        isOpen={historyOpen}
        onClose={() => setHistoryOpen(false)}
        onRetry={handleRetryTask}
        onApprove={handleApproveTask}
        onReject={handleRejectTask}
        onCancel={handleCancelTask}
      />

      <TokenUsageModal
        isOpen={tokenUsageOpen}
        onClose={() => setTokenUsageOpen(false)}
        projects={workspaceState?.projects ?? []}
      />

      <ModelPreferencesModal
        isOpen={modelPrefsOpen}
        onClose={() => setModelPrefsOpen(false)}
      />

      <NotificationPanel
        isOpen={notificationsOpen}
        onClose={() => setNotificationsOpen(false)}
        projects={workspaceState?.projects ?? []}
        activeProjectId={activeProject?.id ?? null}
        onNavigateToTask={(taskId, _projectId) => {
          setNotificationsOpen(false);
          void loadTaskReport(taskId);
        }}
      />

      <ActivityFeed
        isOpen={activityOpen}
        onClose={() => setActivityOpen(false)}
        projects={workspaceState?.projects ?? []}
        activeProjectId={activeProject?.id ?? null}
        onNavigateToTask={(taskId, _projectId) => {
          setActivityOpen(false);
          void loadTaskReport(taskId);
        }}
      />

      <TaskTimeline
        isOpen={!!taskReport && !!taskReport.task_id}
        onClose={() => { setTaskReport(null); setTaskReportOpen(false); }}
        taskId={taskReport?.task_id ?? ""}
        projects={workspaceState?.projects ?? []}
        onNavigateToTask={(taskId, _projectId) => {
          void loadTaskReport(taskId);
        }}
      />

      <TaskReportModal
        isOpen={taskReportOpen}
        onClose={() => { setTaskReportOpen(false); setTaskReport(null); }}
        taskId={taskReport?.task_id ?? ""}
        projects={workspaceState?.projects ?? []}
        onRefresh={() => taskReport && void loadTaskReport(taskReport.task_id)}
      />

      <ProjectReportModal
        isOpen={projectReportOpen}
        onClose={() => { setProjectReportOpen(false); setProjectReport(null); }}
        projectId={projectReport?.project_id ?? ""}
        projects={workspaceState?.projects ?? []}
        onRefresh={() => projectReport && void loadProjectReport(projectReport.project_id)}
      />

      <ProfilesModal
        isOpen={profilesOpen}
        onClose={() => setProfilesOpen(false)}
        projects={workspaceState?.projects ?? []}
        profiles={profiles}
        onChanged={async () => {
          await refreshWorkspace();
          try {
            setProviderStatus(
              await invoke<ProviderStatus>("get_provider_status"),
            );
          } catch (error) {
            console.error("Failed to refresh provider status:", error);
          }
        }}
      />

      <CodingAgentsModal
        isOpen={agentsOpen}
        onClose={() => setAgentsOpen(false)}
      />

      <SessionsModal
        isOpen={sessionsOpen}
        onClose={() => setSessionsOpen(false)}
        projects={workspaceState?.projects ?? []}
        profiles={profiles}
        cliPresets={cliPresets}
        activeProjectId={activeProject?.id ?? null}
        onChanged={refreshWorkspace}
        onConfigureProviders={() => {
          setSessionsOpen(false);
          setProfilesOpen(true);
        }}
        onTaskCreated={handleSessionTaskCreated}
      />

      {instructionsOpen && (
        <div className="config-modal-backdrop" onClick={() => setInstructionsOpen(false)}>
          <div className="config-modal" style={{ width: "min(1200px, 96vw)", maxWidth: "min(1200px, 96vw)", maxHeight: "90vh", overflow: "auto" }} onClick={(e) => e.stopPropagation()}>
            <InstructionManager
              projectRoot={projectPath}
              projectId={activeProject?.id ?? null}
              onClose={() => setInstructionsOpen(false)}
            />
          </div>
        </div>
      )}

      {skillsOpen && (
        <div className="config-modal-backdrop" onClick={() => setSkillsOpen(false)}>
          <div className="config-modal" style={{ width: "min(1200px, 96vw)", maxHeight: "90vh", overflow: "auto" }} onClick={(e) => e.stopPropagation()}>
            <SkillManager
              projectRoot={projectPath}
              onClose={() => setSkillsOpen(false)}
            />
          </div>
        </div>
      )}
    </main>
  );
}

export default App;
