import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export type TaskState =
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

export type Plan = {
  summary: string;
  steps: string[];
  acceptance_criteria: string[];
};

export type ReviewerVerdict = "complete" | "needs_changes" | "blocked";

export type ReviewerResponse = {
  verdict: ReviewerVerdict;
  findings: string;
  unresolved_issues: string[];
  suggested_fix: string | null;
};

export type ExecutorResult = {
  exit_code: number | null;
  stdout: string;
  stderr: string;
  timed_out: boolean;
  duration_ms: number;
  token_usage?: {
    input_tokens: number | null;
    output_tokens: number | null;
    cache_creation_input_tokens: number | null;
    cache_read_input_tokens: number | null;
    total_cost_usd: string | null;
    model: string | null;
    format: string;
  };
};

export type VerificationStatus = "passed" | "failed" | "skipped" | "unavailable";

export type VerificationResult = {
  command: string;
  status: VerificationStatus;
  exit_code: number | null;
  output: string;
};

export type ChangedFiles = {
  created: string[];
  modified: string[];
  deleted: string[];
};

export type IterationRecord = {
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

export type ComponentState =
  | "pending"
  | "ready"
  | "running"
  | "verifying"
  | "completed"
  | "failed"
  | "retrying"
  | "skipped";

export type Component = {
  id: string;
  description: string;
  state: ComponentState;
  depends_on: string[];
  acceptance_criteria: string[];
  error: string | null;
};

export type TaskGraph = {
  components: Component[];
};

export type TaskRecord = {
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
  task_level_changes?: ChangedFiles | null;
  task_baseline?: Record<string, unknown> | null;
  execution_config?: {
    planner_type: string;
    executor_type: string;
    cloud_model: string | null;
    cli_executable: string | null;
  } | null;
  planning_context?: string | null;
  source_session_id?: string | null;
  selected_skill_ids?: string[];
  task_model_id?: string | null;
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

const STATE_COLORS: Record<TaskState, string> = {
  completed: "#73c69c",
  failed: "#ff8a8a",
  cancelled: "#f5c842",
  awaiting_approval: "#f5c842",
  planning: "#6aa6ff",
  executing: "#6aa6ff",
  verifying: "#6aa6ff",
  reviewing: "#6aa6ff",
  retrying: "#f5c842",
  queued: "#9aa0ad",
  idle: "#9aa0ad",
};

function formatTimestamp(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function formatDuration(durationMs: number): string {
  if (durationMs < 1000) return `${durationMs} ms`;
  return `${(durationMs / 1000).toFixed(1)} s`;
}

function shortId(id: string): string {
  return id.slice(0, 8);
}

function projectName(path: string): string {
  return path.split(/[\\/]/).filter(Boolean).pop() ?? path;
}

function changedFilesCount(files: ChangedFiles | null | undefined): number {
  if (!files) return 0;
  return files.created.length + files.modified.length + files.deleted.length;
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

function effectiveVerdict(iteration: IterationRecord): ReviewerVerdict | null {
  const verificationFailed = iteration.verification_results.some(
    (verification) => verification.status !== "passed"
  );
  if (verificationFailed) return "needs_changes";
  return iteration.reviewer_response?.verdict ?? null;
}

function verificationBlocksCompletion(iteration: IterationRecord): boolean {
  return iteration.verification_results.some(
    (verification) => verification.status !== "passed"
  );
}

export function TaskTimeline({
  isOpen,
  onClose,
  taskId,
  projects,
}: {
  isOpen: boolean;
  onClose: () => void;
  taskId: string;
  projects: ProjectRecord[];
  onNavigateToTask?: (taskId: string, projectId: string | null) => void;
}) {
  const [task, setTask] = useState<TaskRecord | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<"timeline" | "details" | "verification" | "review" | "files">("timeline");

  const loadTask = async () => {
    setLoading(true);
    setError(null);
    try {
      const record = await invoke<TaskRecord>("get_task_timeline", { taskId });
      if (record) {
        setTask(record);
      } else {
        setError("Task not found");
      }
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load task timeline");
    } finally {
      setLoading(false);
    }
  };

  const projectNameById = (id: string | null | undefined) => {
    if (!id) return "Unknown project";
    return projects.find((p) => p.id === id)?.name ?? id;
  };

  if (!isOpen) return null;

  if (loading && !task) {
    return (
      <div className="config-modal-backdrop" onClick={onClose}>
        <div className="config-modal" style={{ width: "min(1100px, 96vw)", maxHeight: "90vh" }} onClick={(e) => e.stopPropagation()}>
          <div className="config-modal-header">
            <h2>Task Timeline</h2>
            <button className="icon-button small" onClick={onClose} title="Close">×</button>
          </div>
          <div className="config-modal-body" style={{ padding: 24, textAlign: "center", color: "#9aa0ad" }}>
            Loading task timeline…
          </div>
        </div>
      </div>
    );
  }

  if (error && !task) {
    return (
      <div className="config-modal-backdrop" onClick={onClose}>
        <div className="config-modal" style={{ width: "min(1100px, 96vw)", maxHeight: "90vh" }} onClick={(e) => e.stopPropagation()}>
          <div className="config-modal-header">
            <h2>Task Timeline</h2>
            <button className="icon-button small" onClick={onClose} title="Close">×</button>
          </div>
          <div className="config-modal-body" style={{ padding: 24 }}>
            <div className="inline-error">{error}</div>
            <button className="primary-button" onClick={loadTask} style={{ marginTop: 12 }}>Retry</button>
          </div>
        </div>
      </div>
    );
  }

  if (!task) return null;

  const taskStateColor = STATE_COLORS[task.state] || "#6aa6ff";
  const taskLevelChanges = task.task_level_changes ?? collectTaskChangedFiles(task);

  return (
    <div className="config-modal-backdrop" onClick={onClose}>
      <div
        className="config-modal task-timeline-modal"
        onClick={(e) => e.stopPropagation()}
        style={{ width: "min(1100px, 96vw)", maxHeight: "90vh", display: "flex", flexDirection: "column" }}
      >
        <div className="config-modal-header" style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-start", flexWrap: "wrap", gap: 8 }}>
          <div>
            <h2 style={{ margin: 0, display: "flex", alignItems: "center", gap: 8 }}>
              Task Timeline
              <span
                className="task-state"
                style={{
                  background: `${taskStateColor}22`,
                  color: taskStateColor,
                  padding: "2px 8px",
                  borderRadius: 12,
                  fontSize: 12,
                  fontWeight: 600,
                  textTransform: "capitalize",
                }}
              >
                {TASK_STATE_LABELS[task.state]}
              </span>
            </h2>
            <div style={{ fontSize: 12, color: "#9aa0ad", marginTop: 4 }}>
              Task {shortId(task.id)} · {projectNameById(task.project_id)} · {projectName(task.project_root)}
            </div>
          </div>
          <div style={{ display: "flex", gap: 8 }}>
            <button className="secondary-button" onClick={loadTask} disabled={loading}>
              {loading ? "Refreshing..." : "Refresh"}
            </button>
            <button className="icon-button small" onClick={onClose} title="Close">×</button>
          </div>
        </div>

        <div className="config-modal-body" style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
          {error && <div className="inline-error" role="alert" style={{ marginBottom: 12, padding: 12 }}>{error}</div>}

          {/* Tab Navigation */}
          <div className="timeline-tabs" style={{ display: "flex", gap: 4, borderBottom: "1px solid #2f3542", marginBottom: 12, paddingBottom: 4 }}>
            {[
              { id: "timeline", label: "Timeline", count: task.iterations.length },
              { id: "details", label: "Details" },
              { id: "verification", label: "Verification", count: task.iterations.flatMap((i) => i.verification_results).length },
              { id: "review", label: "Review", count: task.iterations.filter((i) => i.reviewer_response).length },
              { id: "files", label: "Files Changed", count: changedFilesCount(taskLevelChanges) },
            ].map((tab) => (
              <button
                key={tab.id}
                type="button"
                className={`timeline-tab ${activeTab === tab.id ? "active" : ""}`}
                onClick={() => setActiveTab(tab.id as typeof activeTab)}
                style={{
                  padding: "8px 16px",
                  background: activeTab === tab.id ? "#21252d" : "transparent",
                  border: "none",
                  borderBottom: `2px solid ${activeTab === tab.id ? "#6aa6ff" : "transparent"}`,
                  color: activeTab === tab.id ? "#6aa6ff" : "#9aa0ad",
                  fontSize: 13,
                  fontWeight: 500,
                  cursor: "pointer",
                  borderRadius: "4px 4px 0 0",
                }}
              >
                {tab.label}
                {tab.count !== undefined && tab.count > 0 && (
                  <span style={{ marginLeft: 6, padding: "1px 6px", borderRadius: 10, background: "#2f3542", color: "#b4b8c6", fontSize: 11, fontWeight: 400 }}>
                    {tab.count}
                  </span>
                )}
              </button>
            ))}
          </div>

          {/* Tab Content */}
          <div style={{ flex: 1, overflowY: "auto", minHeight: 0 }}>
            {activeTab === "timeline" && <TimelineTab task={task} />}
            {activeTab === "details" && <DetailsTab task={task} />}
            {activeTab === "verification" && <VerificationTab task={task} />}
            {activeTab === "review" && <ReviewTab task={task} />}
            {activeTab === "files" && <FilesTab task={task} taskLevelChanges={taskLevelChanges} />}
          </div>
        </div>
      </div>
    </div>
  );
}

function TimelineTab({ task }: { task: TaskRecord }) {
  if (task.iterations.length === 0) {
    return (
      <div style={{ padding: 24, textAlign: "center", color: "#9aa0ad" }}>
        <strong>No execution attempts recorded</strong>
        <p style={{ marginTop: 8 }}>This task has not started executing yet.</p>
      </div>
    );
  }

  return (
    <div className="timeline-view" style={{ display: "flex", flexDirection: "column", gap: 16, padding: 4 }}>
      {/* Task Overview */}
      <div className="timeline-overview" style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16 }}>
        <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#e3e5ec" }}>Task Overview</h3>
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(200px, 1fr))", gap: 12 }}>
          <div><strong>Created</strong> <span style={{ color: "#b4b8c6" }}>{formatTimestamp(task.created_at)}</span></div>
          <div><strong>Updated</strong> <span style={{ color: "#b4b8c6" }}>{formatTimestamp(task.updated_at)}</span></div>
          <div><strong>Iterations</strong> <span style={{ color: "#b4b8c6" }}>{task.iteration} / {task.max_iterations}</span></div>
          <div><strong>Total Attempts</strong> <span style={{ color: "#b4b8c6" }}>{task.iterations.length}</span></div>
          {task.agent_name && <div><strong>Agent</strong> <span style={{ color: "#b4b8c6" }}>{task.agent_name}</span></div>}
          {task.task_model_id && <div><strong>Model</strong> <span style={{ color: "#b4b8c6" }}>{task.task_model_id}</span></div>}
          {task.recovery_attempts > 0 && <div><strong>Recovery Attempts</strong> <span style={{ color: "#f5c842" }}>{task.recovery_attempts}</span></div>}
          {task.source_session_id && <div><strong>Source Session</strong> <span style={{ color: "#b4b8c6" }}>{shortId(task.source_session_id)}</span></div>}
        </div>
      </div>

      {/* Execution Timeline */}
      <div className="timeline-items">
        <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#e3e5ec" }}>Execution Timeline</h3>
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          {[...task.iterations].reverse().map((iteration, index) => (
            <TimelineIterationCard
              key={`${iteration.iteration}-${iteration.started_at}`}
              iteration={iteration}
              sequence={task.iterations.length - index}
              isLatest={index === 0}
              totalIterations={task.iterations.length}
            />
          ))}
        </div>
      </div>
    </div>
  );
}

function TimelineIterationCard({
  iteration,
  sequence,
  isLatest,
  totalIterations,
}: {
  iteration: IterationRecord;
  sequence: number;
  isLatest: boolean;
  totalIterations: number;
}) {
  const verdict = effectiveVerdict(iteration);
  const review = iteration.reviewer_response;
  const executor = iteration.executor_result;
  const filesChanged = changedFilesCount(iteration.changed_files);
  const verificationFailed = verificationBlocksCompletion(iteration);

  const getIterationStatus = () => {
    if (verdict === "complete") return { label: "Completed", color: "#73c69c" };
    if (verdict === "needs_changes") return { label: "Needs Changes", color: "#f5c842" };
    if (verdict === "blocked") return { label: "Blocked", color: "#ff8a8a" };
    if (executor?.timed_out) return { label: "Timed Out", color: "#ff8a8a" };
    if (executor?.exit_code !== null && executor?.exit_code !== 0) return { label: "Failed", color: "#ff8a8a" };
    if (executor) return { label: "Executed", color: "#6aa6ff" };
    return { label: "Pending", color: "#9aa0ad" };
  };

  const status = getIterationStatus();

  return (
    <details className="timeline-iteration" open={isLatest} style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, overflow: "hidden" }}>
      <summary style={{ display: "flex", alignItems: "center", gap: 12, padding: 12, cursor: "pointer", background: isLatest ? "#21252d" : "transparent" }}>
        <div style={{ display: "flex", flexDirection: "column", alignItems: "center", minWidth: 60 }}>
          <span style={{ fontWeight: 700, fontSize: 16, color: "#e3e5ec" }}>{sequence}</span>
          <span style={{ fontSize: 10, color: "#9aa0ad" }}>of {totalIterations}</span>
        </div>
        <div style={{ flex: 1, borderLeft: "1px solid #2f3542", paddingLeft: 12 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
            <span className="iteration-attempt" style={{ fontWeight: 600, color: "#e3e5ec" }}>Attempt {iteration.iteration}</span>
            <span className="iteration-verdict" style={{ fontSize: 11, padding: "2px 8px", borderRadius: 12, background: `${status.color}22`, color: status.color, fontWeight: 500 }}>{status.label}</span>
            <span className="iteration-files" style={{ fontSize: 11, color: "#9aa0ad" }}>{filesChanged} files changed</span>
            {executor && (
              <span className="iteration-meta" style={{ fontSize: 11, color: "#9aa0ad" }}>
                {executor.timed_out ? "Timed out" : `Exit ${executor.exit_code ?? "n/a"}`} · {formatDuration(executor.duration_ms)}
              </span>
            )}
            {verificationFailed && <span className="iteration-warning" style={{ fontSize: 11, color: "#ff8a8a" }}>Verification failed</span>}
          </div>
          <div style={{ fontSize: 11, color: "#9aa0ad", marginTop: 4 }}>
            Started: {formatTimestamp(iteration.started_at)}
            {iteration.completed_at && ` · Completed: ${formatTimestamp(iteration.completed_at)}`}
          </div>
        </div>
        <div style={{ textAlign: "right", minWidth: 80 }}>
          {isLatest && <span style={{ fontSize: 10, color: "#6aa6ff", background: "#173322", padding: "2px 6px", borderRadius: 4 }}>Latest</span>}
        </div>
      </summary>

      <div style={{ padding: "0 12 12", borderTop: "1px solid #2f3542" }}>
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(300px, 1fr))", gap: 16 }}>
          {/* Executor */}
          <div className="timeline-section">
            <h4 style={{ margin: "0 0 8px", fontSize: 12, color: "#9aa0ad", textTransform: "uppercase", letterSpacing: "0.5px" }}>Executor</h4>
            {executor ? (
              <>
                <div style={{ display: "flex", gap: 16, fontSize: 12, color: "#b4b8c6", marginBottom: 8 }}>
                  <span>Exit: {executor.exit_code ?? "n/a"}</span>
                  <span>Duration: {formatDuration(executor.duration_ms)}</span>
                  {executor.timed_out && <span style={{ color: "#ff8a8a" }}>Timed out</span>}
                </div>
                {executor.stdout.trim() && (
                  <details>
                    <summary style={{ fontSize: 12, color: "#6aa6ff", cursor: "pointer" }}>stdout</summary>
                    <pre style={{ marginTop: 8, fontSize: 11, background: "#0d1117", padding: 8, borderRadius: 4, overflow: "auto", maxHeight: 200 }}>{executor.stdout}</pre>
                  </details>
                )}
                {executor.stderr.trim() && (
                  <details>
                    <summary style={{ fontSize: 12, color: "#ff8a8a", cursor: "pointer" }}>stderr</summary>
                    <pre style={{ marginTop: 8, fontSize: 11, background: "#2d1111", padding: 8, borderRadius: 4, overflow: "auto", maxHeight: 200 }}>{executor.stderr}</pre>
                  </details>
                )}
                {!executor.stdout.trim() && !executor.stderr.trim() && (
                  <p style={{ color: "#9aa0ad", fontSize: 12 }}>No output captured</p>
                )}
              </>
            ) : (
              <p style={{ color: "#9aa0ad", fontSize: 12 }}>No executor result</p>
            )}
          </div>

          {/* Changed Files */}
          <div className="timeline-section">
            <h4 style={{ margin: "0 0 8px", fontSize: 12, color: "#9aa0ad", textTransform: "uppercase", letterSpacing: "0.5px" }}>Changed Files ({filesChanged})</h4>
            {filesChanged > 0 ? (
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                {iteration.changed_files.created.length > 0 && (
                  <div>
                    <div style={{ fontSize: 11, color: "#73c69c", marginBottom: 4 }}>Created ({iteration.changed_files.created.length})</div>
                    <ul style={{ margin: 0, paddingLeft: 16, fontSize: 11, color: "#b4b8c6" }}>
                      {iteration.changed_files.created.map((f) => <li key={f}>{f}</li>)}
                    </ul>
                  </div>
                )}
                {iteration.changed_files.modified.length > 0 && (
                  <div>
                    <div style={{ fontSize: 11, color: "#f5c842", marginBottom: 4 }}>Modified ({iteration.changed_files.modified.length})</div>
                    <ul style={{ margin: 0, paddingLeft: 16, fontSize: 11, color: "#b4b8c6" }}>
                      {iteration.changed_files.modified.map((f) => <li key={f}>{f}</li>)}
                    </ul>
                  </div>
                )}
                {iteration.changed_files.deleted.length > 0 && (
                  <div>
                    <div style={{ fontSize: 11, color: "#ff8a8a", marginBottom: 4 }}>Deleted ({iteration.changed_files.deleted.length})</div>
                    <ul style={{ margin: 0, paddingLeft: 16, fontSize: 11, color: "#b4b8c6" }}>
                      {iteration.changed_files.deleted.map((f) => <li key={f}>{f}</li>)}
                    </ul>
                  </div>
                )}
              </div>
            ) : (
              <p style={{ color: "#9aa0ad", fontSize: 12 }}>No files changed</p>
            )}
          </div>

          {/* Verification */}
          <div className="timeline-section">
            <h4 style={{ margin: "0 0 8px", fontSize: 12, color: "#9aa0ad", textTransform: "uppercase", letterSpacing: "0.5px" }}>Verification ({iteration.verification_results.length})</h4>
            {iteration.verification_results.length > 0 ? (
              <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                {iteration.verification_results.map((v, idx) => (
                  <div key={idx} style={{ background: "#0d1117", border: "1px solid #2f3542", borderRadius: 4, padding: 8 }}>
                    <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4 }}>
                      <span style={{ fontSize: 10, padding: "2px 6px", borderRadius: 4, background: `${v.status === "passed" ? "#73c69c" : v.status === "failed" ? "#ff8a8a" : "#9aa0ad"}22`, color: v.status === "passed" ? "#73c69c" : v.status === "failed" ? "#ff8a8a" : "#9aa0ad", fontWeight: 500, textTransform: "uppercase" }}>
                        {VERIFICATION_STATUS_LABELS[v.status]}
                      </span>
                      <span style={{ fontSize: 11, color: "#b4b8c6", fontFamily: "monospace" }}>{v.command}</span>
                      {v.exit_code !== null && <span style={{ fontSize: 11, color: "#9aa0ad" }}>exit {v.exit_code}</span>}
                    </div>
                    {v.output.trim() && (
                      <pre style={{ fontSize: 10, background: "#0d1117", padding: 8, borderRadius: 4, overflow: "auto", maxHeight: 150 }}>{v.output}</pre>
                    )}
                  </div>
                ))}
              </div>
            ) : (
              <p style={{ color: "#9aa0ad", fontSize: 12 }}>No verification configured</p>
            )}
          </div>

          {/* Review */}
          <div className="timeline-section">
            <h4 style={{ margin: "0 0 8px", fontSize: 12, color: "#9aa0ad", textTransform: "uppercase", letterSpacing: "0.5px" }}>Review</h4>
            {review ? (
              <>
                <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 8 }}>
                  <span style={{ fontSize: 11, padding: "2px 8px", borderRadius: 12, background: `${review.verdict === "complete" ? "#73c69c" : review.verdict === "needs_changes" ? "#f5c842" : "#ff8a8a"}22`, color: review.verdict === "complete" ? "#73c69c" : review.verdict === "needs_changes" ? "#f5c842" : "#ff8a8a", fontWeight: 500, textTransform: "capitalize" }}>
                    {REVIEW_VERDICT_LABELS[review.verdict]}
                  </span>
                  {verificationFailed && verdict === "needs_changes" && review.verdict === "complete" && (
                    <span style={{ fontSize: 10, color: "#ff8a8a" }}>Verification failed → treated as Needs Changes</span>
                  )}
                </div>
                <p style={{ color: "#b4b8c6", fontSize: 12, margin: "0 0 8px" }}>{review.findings}</p>
                {review.unresolved_issues.length > 0 && (
                  <div>
                    <div style={{ fontSize: 11, color: "#ff8a8a", marginBottom: 4 }}>Unresolved Issues</div>
                    <ul style={{ margin: 0, paddingLeft: 16, fontSize: 11, color: "#b4b8c6" }}>
                      {review.unresolved_issues.map((issue, i) => <li key={i}>{issue}</li>)}
                    </ul>
                  </div>
                )}
                {review.suggested_fix && (
                  <details style={{ marginTop: 8 }}>
                    <summary style={{ fontSize: 11, color: "#6aa6ff", cursor: "pointer" }}>Suggested Fix</summary>
                    <p style={{ marginTop: 8, fontSize: 12, color: "#b4b8c6" }}>{review.suggested_fix}</p>
                  </details>
                )}
              </>
            ) : (
              <p style={{ color: "#9aa0ad", fontSize: 12 }}>No review yet</p>
            )}
          </div>
        </div>

        {/* Coding Prompt */}
        <details style={{ marginTop: 16 }}>
          <summary style={{ fontSize: 11, color: "#9aa0ad", cursor: "pointer" }}>Coding Prompt Sent to Executor</summary>
          <pre style={{ marginTop: 8, fontSize: 11, background: "#0d1117", padding: 12, borderRadius: 4, overflow: "auto", maxHeight: 300, whiteSpace: "pre-wrap" }}>{iteration.coding_prompt}</pre>
        </details>
      </div>
    </details>
  );
}

function DetailsTab({ task }: { task: TaskRecord }) {
  return (
    <div style={{ padding: 4, display: "flex", flexDirection: "column", gap: 16 }}>
      <div className="detail-section" style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16 }}>
        <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#e3e5ec" }}>Task Request</h3>
        <p style={{ color: "#b4b8c6", lineHeight: 1.6, whiteSpace: "pre-wrap" }}>{task.user_task}</p>
      </div>

      {task.plan && (
        <div className="detail-section" style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16 }}>
          <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#e3e5ec" }}>Plan</h3>
          <p style={{ color: "#b4b8c6", marginBottom: 12 }}>{task.plan.summary}</p>
          {task.plan.steps.length > 0 && (
            <>
              <h4 style={{ margin: "0 0 8px", fontSize: 12, color: "#9aa0ad" }}>Steps</h4>
              <ol style={{ margin: 0, paddingLeft: 20, color: "#b4b8c6" }}>
                {task.plan.steps.map((step, i) => <li key={i} style={{ marginBottom: 4 }}>{step}</li>)}
              </ol>
            </>
          )}
          {task.plan.acceptance_criteria.length > 0 && (
            <>
              <h4 style={{ margin: "12px 0 8px", fontSize: 12, color: "#9aa0ad" }}>Acceptance Criteria</h4>
              <ul style={{ margin: 0, paddingLeft: 20, color: "#b4b8c6" }}>
                {task.plan.acceptance_criteria.map((c, i) => <li key={i} style={{ marginBottom: 4 }}>{c}</li>)}
              </ul>
            </>
          )}
        </div>
      )}

      {task.task_graph && (
        <div className="detail-section" style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16 }}>
          <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#e3e5ec" }}>Execution Graph</h3>
          <ol style={{ margin: 0, paddingLeft: 20 }}>
            {task.task_graph.components.map((component) => (
              <li key={component.id} style={{ marginBottom: 8, color: "#b4b8c6" }}>
                <strong>{component.id}:</strong> {component.description}
                <span style={{ marginLeft: 8, fontSize: 11, padding: "2px 6px", borderRadius: 4, background: "#2f3542", color: "#9aa0ad" }}>
                  {COMPONENT_STATE_LABELS[component.state]}
                </span>
                {component.error && <div style={{ marginTop: 4, color: "#ff8a8a", fontSize: 12 }}>{component.error}</div>}
              </li>
            ))}
          </ol>
        </div>
      )}

      <div className="detail-section" style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, padding: 16 }}>
        <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#e3e5ec" }}>Metadata</h3>
        <div style={{ display: "grid", gridTemplateColumns: "repeat(auto-fit, minmax(200px, 1fr))", gap: 8, fontSize: 12 }}>
          <div><strong>Task ID</strong> <span style={{ color: "#b4b8c6" }}>{task.id}</span></div>
          <div><strong>Project</strong> <span style={{ color: "#b4b8c6" }}>{projectName(task.project_root)}</span></div>
          <div><strong>Project ID</strong> <span style={{ color: "#b4b8c6" }}>{task.project_id ?? "none"}</span></div>
          <div><strong>Provider Profile</strong> <span style={{ color: "#b4b8c6" }}>{task.provider_profile_id ?? "none"}</span></div>
          <div><strong>Agent</strong> <span style={{ color: "#b4b8c6" }}>{task.agent_name ?? "none"}</span></div>
          <div><strong>Model</strong> <span style={{ color: "#b4b8c6" }}>{task.task_model_id ?? "default"}</span></div>
          <div><strong>Priority</strong> <span style={{ color: "#b4b8c6" }}>{task.priority}</span></div>
          <div><strong>Max Iterations</strong> <span style={{ color: "#b4b8c6" }}>{task.max_iterations}</span></div>
          <div><strong>Recovery Attempts</strong> <span style={{ color: "#b4b8c6" }}>{task.recovery_attempts}</span></div>
          <div><strong>Created</strong> <span style={{ color: "#b4b8c6" }}>{formatTimestamp(task.created_at)}</span></div>
          <div><strong>Updated</strong> <span style={{ color: "#b4b8c6" }}>{formatTimestamp(task.updated_at)}</span></div>
          {task.queued_at && <div><strong>Queued</strong> <span style={{ color: "#b4b8c6" }}>{formatTimestamp(task.queued_at)}</span></div>}
          {task.source_session_id && <div><strong>Source Session</strong> <span style={{ color: "#b4b8c6" }}>{shortId(task.source_session_id)}</span></div>}
          {task.planning_context && (
            <details style={{ gridColumn: "1 / -1", marginTop: 8 }}>
              <summary style={{ fontSize: 11, color: "#9aa0ad", cursor: "pointer" }}>Planning Context</summary>
              <pre style={{ marginTop: 8, fontSize: 11, background: "#0d1117", padding: 8, borderRadius: 4, overflow: "auto", maxHeight: 200 }}>{task.planning_context}</pre>
            </details>
          )}
          {task.selected_skill_ids?.length && (
            <div style={{ gridColumn: "1 / -1", marginTop: 8 }}>
              <strong>Selected Skills</strong>
              <ul style={{ margin: "4px 0 0 20", color: "#b4b8c6" }}>
                {task.selected_skill_ids.map((s) => <li key={s}>{s}</li>)}
              </ul>
            </div>
          )}
        </div>
      </div>

      {task.error && (
        <div className="detail-section" style={{ background: "#2d1111", border: "1px solid #ff8a8a44", borderRadius: 8, padding: 16 }}>
          <h3 style={{ margin: "0 0 8px", fontSize: 14, color: "#ff8a8a" }}>Error</h3>
          <p style={{ color: "#ff8a8a", margin: 0 }}>{task.error}</p>
        </div>
      )}
    </div>
  );
}

function VerificationTab({ task }: { task: TaskRecord }) {
  const allVerifications = task.iterations.flatMap((i, idx) =>
    i.verification_results.map((v) => ({ ...v, iteration: idx + 1, attempt: i.iteration }))
  );

  if (allVerifications.length === 0) {
    return (
      <div style={{ padding: 24, textAlign: "center", color: "#9aa0ad" }}>
        <strong>No verification runs recorded</strong>
        <p style={{ marginTop: 8 }}>Verification commands have not been executed for this task.</p>
      </div>
    );
  }

  return (
    <div style={{ padding: 4 }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 12, flexWrap: "wrap", gap: 8 }}>
        <h3 style={{ margin: 0, fontSize: 14, color: "#e3e5ec" }}>Verification Results ({allVerifications.length})</h3>
        <div style={{ display: "flex", gap: 8 }}>
          {["passed", "failed", "skipped", "unavailable"].map((status) => {
            const count = allVerifications.filter((v) => v.status === status).length;
            const color = status === "passed" ? "#73c69c" : status === "failed" ? "#ff8a8a" : "#9aa0ad";
            return count > 0 && (
              <span style={{ fontSize: 11, padding: "2px 8px", borderRadius: 12, background: `${color}22`, color, fontWeight: 500 }}>
                {status}: {count}
              </span>
            );
          })}
        </div>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        {allVerifications.map((v, idx) => (
          <details key={`${v.iteration}-${v.attempt}-${idx}`} style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, overflow: "hidden" }}>
            <summary style={{ display: "flex", alignItems: "center", gap: 8, padding: 12, cursor: "pointer" }}>
              <span style={{ fontSize: 10, padding: "2px 6px", borderRadius: 4, background: `${v.status === "passed" ? "#73c69c" : v.status === "failed" ? "#ff8a8a" : "#9aa0ad"}22`, color: v.status === "passed" ? "#73c69c" : v.status === "failed" ? "#ff8a8a" : "#9aa0ad", fontWeight: 500, textTransform: "uppercase" }}>
                {VERIFICATION_STATUS_LABELS[v.status]}
              </span>
              <span style={{ fontSize: 12, color: "#b4b8c6", fontFamily: "monospace", flex: 1 }}>{v.command}</span>
              <span style={{ fontSize: 11, color: "#9aa0ad" }}>Iteration {v.iteration} (attempt {v.attempt})</span>
              {v.exit_code !== null && <span style={{ fontSize: 11, color: "#9aa0ad" }}>exit {v.exit_code}</span>}
            </summary>
            <div style={{ padding: "0 12 12" }}>
              {v.output.trim() ? (
                <pre style={{ fontSize: 11, background: "#0d1117", padding: 12, borderRadius: 4, overflow: "auto", maxHeight: 300 }}>{v.output}</pre>
              ) : (
                <p style={{ color: "#9aa0ad", padding: 12, margin: 0 }}>No output captured</p>
              )}
            </div>
          </details>
        ))}
      </div>
    </div>
  );
}

function ReviewTab({ task }: { task: TaskRecord }) {
  const reviews = task.iterations
    .map((i, idx) => ({ ...i, iteration: idx + 1 }))
    .filter((i) => i.reviewer_response);

  if (reviews.length === 0) {
    return (
      <div style={{ padding: 24, textAlign: "center", color: "#9aa0ad" }}>
        <strong>No review evaluations recorded</strong>
        <p style={{ marginTop: 8 }}>The reviewer has not evaluated any attempts yet.</p>
      </div>
    );
  }

  return (
    <div style={{ padding: 4, display: "flex", flexDirection: "column", gap: 12 }}>
      {reviews.map((iteration) => {
        const review = iteration.reviewer_response!;
        const verdict = effectiveVerdict(iteration);
        const verificationFailed = verificationBlocksCompletion(iteration);

        return (
          <details key={iteration.iteration} open style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, overflow: "hidden" }}>
            <summary style={{ display: "flex", alignItems: "center", gap: 8, padding: 12, cursor: "pointer" }}>
              <span style={{ fontWeight: 600, color: "#e3e5ec" }}>Iteration {iteration.iteration} Review</span>
              <span style={{ fontSize: 11, padding: "2px 8px", borderRadius: 12, background: `${review.verdict === "complete" ? "#73c69c" : review.verdict === "needs_changes" ? "#f5c842" : "#ff8a8a"}22`, color: review.verdict === "complete" ? "#73c69c" : review.verdict === "needs_changes" ? "#f5c842" : "#ff8a8a", fontWeight: 500, textTransform: "capitalize" }}>
                {REVIEW_VERDICT_LABELS[review.verdict]}
              </span>
              {verificationFailed && verdict === "needs_changes" && review.verdict === "complete" && (
                <span style={{ fontSize: 10, color: "#ff8a8a" }}>Verification failed → treated as Needs Changes</span>
              )}
              <span style={{ fontSize: 11, color: "#9aa0ad", marginLeft: "auto" }}>Started: {formatTimestamp(iteration.started_at)}</span>
            </summary>
            <div style={{ padding: "0 12 12" }}>
              <p style={{ color: "#b4b8c6", margin: "0 0 12px" }}>{review.findings}</p>
              {review.unresolved_issues.length > 0 && (
                <div style={{ marginBottom: 12 }}>
                  <div style={{ fontSize: 11, color: "#ff8a8a", marginBottom: 4 }}>Unresolved Issues</div>
                  <ul style={{ margin: 0, paddingLeft: 20, color: "#b4b8c6" }}>
                    {review.unresolved_issues.map((issue, i) => <li key={i}>{issue}</li>)}
                  </ul>
                </div>
              )}
              {review.suggested_fix && (
                <details>
                  <summary style={{ fontSize: 11, color: "#6aa6ff", cursor: "pointer" }}>Suggested Fix</summary>
                  <p style={{ marginTop: 8, color: "#b4b8c6" }}>{review.suggested_fix}</p>
                </details>
              )}
              {review.verdict === "needs_changes" && (
                <p style={{ color: "#f5c842", fontSize: 12, marginTop: 8 }}>Correction requested — the next attempt includes this feedback.</p>
              )}
            </div>
          </details>
        );
      })}
    </div>
  );
}

function FilesTab({ task: _task, taskLevelChanges }: { task: TaskRecord; taskLevelChanges: ChangedFiles }) {
  const totalFiles = changedFilesCount(taskLevelChanges);

  if (totalFiles === 0) {
    return (
      <div style={{ padding: 24, textAlign: "center", color: "#9aa0ad" }}>
        <strong>No files changed</strong>
        <p style={{ marginTop: 8 }}>This task has not modified any files.</p>
      </div>
    );
  }

  return (
    <div style={{ padding: 4 }}>
      <h3 style={{ margin: "0 0 12px", fontSize: 14, color: "#e3e5ec" }}>Files Changed ({totalFiles})</h3>
      <div style={{ display: "flex", flexDirection: "column", gap: 16 }}>
        {taskLevelChanges.created.length > 0 && (
          <div className="file-group" style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, overflow: "hidden" }}>
            <div style={{ padding: 12, background: "#173322", borderBottom: "1px solid #2f3542" }}>
              <strong style={{ color: "#73c69c" }}>Created ({taskLevelChanges.created.length})</strong>
            </div>
            <ul style={{ margin: 0, padding: 12 }}>
              {taskLevelChanges.created.map((f) => <li key={f} style={{ color: "#b4b8c6", fontSize: 12, marginBottom: 4, fontFamily: "monospace" }}>{f}</li>)}
            </ul>
          </div>
        )}
        {taskLevelChanges.modified.length > 0 && (
          <div className="file-group" style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, overflow: "hidden" }}>
            <div style={{ padding: 12, background: "#2d2510", borderBottom: "1px solid #2f3542" }}>
              <strong style={{ color: "#f5c842" }}>Modified ({taskLevelChanges.modified.length})</strong>
            </div>
            <ul style={{ margin: 0, padding: 12 }}>
              {taskLevelChanges.modified.map((f) => <li key={f} style={{ color: "#b4b8c6", fontSize: 12, marginBottom: 4, fontFamily: "monospace" }}>{f}</li>)}
            </ul>
          </div>
        )}
        {taskLevelChanges.deleted.length > 0 && (
          <div className="file-group" style={{ background: "#1a1d24", border: "1px solid #2f3542", borderRadius: 8, overflow: "hidden" }}>
            <div style={{ padding: 12, background: "#2d1111", borderBottom: "1px solid #2f3542" }}>
              <strong style={{ color: "#ff8a8a" }}>Deleted ({taskLevelChanges.deleted.length})</strong>
            </div>
            <ul style={{ margin: 0, padding: 12 }}>
              {taskLevelChanges.deleted.map((f) => <li key={f} style={{ color: "#b4b8c6", fontSize: 12, marginBottom: 4, fontFamily: "monospace" }}>{f}</li>)}
            </ul>
          </div>
        )}
      </div>
    </div>
  );
}