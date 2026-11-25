import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

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

type TaskRecord = {
  id: string;
  user_task: string;
  project_root: string;
  state: TaskState;
  plan: { summary: string; steps: string[]; acceptance_criteria: string[] } | null;
  iteration: number;
  max_iterations: number;
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

const TERMINAL_TASK_STATES: TaskState[] = [
  "completed",
  "failed",
  "cancelled",
];

function isTerminalTask(task: TaskRecord) {
  return TERMINAL_TASK_STATES.includes(task.state);
}

function formatTimestamp(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleString();
}

function formatRelativeTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffSecs = Math.floor(diffMs / 1000);
  const diffMins = Math.floor(diffSecs / 60);
  const diffHours = Math.floor(diffMins / 60);
  const diffDays = Math.floor(diffHours / 24);

  if (diffSecs < 60) return "just now";
  if (diffMins < 60) return `${diffMins}m ago`;
  if (diffHours < 24) return `${diffHours}h ago`;
  if (diffDays < 7) return `${diffDays}d ago`;
  return formatTimestamp(value);
}

function stateLabel(state: TaskState): string {
  return state
    .replace(/_/g, " ")
    .replace(/\b\w/g, (c) => c.toUpperCase());
}

interface ProjectTasksScreenProps {
  project: ProjectRecord | null;
  onOpenTask?: (task: TaskRecord) => void;
}

/**
 * Project Tasks screen — design.md §9.
 *
 * Lists all tasks for the active project, with state badges, priority,
 * and cancel/retry actions. Every button uses the token-driven Button
 * component and respects theme / density / font-size presets.
 */
export function ProjectTasksScreen({
  project,
  onOpenTask,
}: ProjectTasksScreenProps) {
  const [tasks, setTasks] = useState<TaskRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [stateFilter, setStateFilter] = useState<TaskState | "">("");

  const loadTasks = useCallback(async () => {
    if (!project) return;
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<TaskRecord[]>("list_tasks", {
        project_id: project.id,
      });
      setTasks(list);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load tasks");
    } finally {
      setLoading(false);
    }
  }, [project]);

  useEffect(() => {
    void loadTasks();
  }, [loadTasks]);

  const handleCancel = async (taskId: string) => {
    if (!window.confirm("Cancel this task? This cannot be undone.")) return;
    try {
      await invoke("cancel_task", { taskId });
      setTasks((prev) => prev.map((t) => (t.id === taskId ? { ...t, state: "cancelled" } : t)));
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to cancel task");
    }
  };

  const handleRetry = async (taskId: string) => {
    try {
      await invoke("retry_task", { taskId });
      setTasks((prev) => prev.map((t) => (t.id === taskId ? { ...t, state: "retrying" } : t)));
      void loadTasks();
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to retry task");
    }
  };

  const filtered = useMemo(() => {
    return tasks.filter((t) => stateFilter === "" || t.state === stateFilter);
  }, [tasks, stateFilter]);

  const counts = useMemo(() => {
    const result: Record<string, number> = {};
    tasks.forEach((t) => {
      result[t.state] = (result[t.state] ?? 0) + 1;
    });
    return result;
  }, [tasks]);

  if (error) {
    return (
      <div className="project-tasks-screen" role="main" aria-label="Project Tasks">
        <header className="project-tasks-head">
          <div>
            <h2 className="project-tasks-title">{project ? project.name : "Project Tasks"}</h2>
            <p className="project-tasks-sub">Tasks for this project.</p>
          </div>
        </header>
        <div className="project-tasks-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => { setError(null); void loadTasks(); }} title="Retry">
            Retry
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="project-tasks-screen" role="main" aria-label="Project Tasks">
      <header className="project-tasks-head">
        <div>
          <h2 className="project-tasks-title">{project ? project.name : "Project Tasks"}</h2>
          <p className="project-tasks-sub">Tasks for this project.</p>
        </div>
        <div className="project-tasks-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={() => void loadTasks()}
            disabled={loading}
            title={loading ? "Refreshing…" : "Refresh tasks"}
          >
            {loading ? "Refreshing…" : "Refresh"}
          </Button>
        </div>
      </header>

      {/* State filter chips */}
      <div className="project-tasks-filters">
        <button
          type="button"
          className={`project-tasks-filter ${stateFilter === "" ? "active" : ""}`}
          onClick={() => setStateFilter("")}
          role="tab"
          aria-selected={stateFilter === ""}
        >
          All
        </button>
        {["queued", "planning", "awaiting_approval", "executing", "verifying", "reviewing", "retrying", "completed", "failed", "cancelled"].map((state) => (
          <button
            key={state}
            type="button"
            className={`project-tasks-filter ${stateFilter === state ? "active" : ""} ${stateFilter === state ? "active-" + state.replace(/_/g, "-") : ""}`}
            onClick={() => setStateFilter(state as TaskState)}
            role="tab"
            aria-selected={stateFilter === state}
          >
            {stateLabel(state as TaskState)}
          </button>
        ))}
      </div>

      {/* Summary bar */}
      <div className="project-tasks-summary">
        <span className="project-tasks-summary-item">
          {tasks.length} total
        </span>
        <span className="project-tasks-summary-item project-tasks-summary-active">
          {(counts["queued"] ?? 0) + (counts["planning"] ?? 0) + (counts["executing"] ?? 0) + (counts["verifying"] ?? 0) + (counts["reviewing"] ?? 0) + (counts["retrying"] ?? 0) + (counts["awaiting_approval"] ?? 0)} active
        </span>
        <span className="project-tasks-summary-item project-tasks-summary-completed">
          {counts["completed"] ?? 0} completed
        </span>
        <span className="project-tasks-summary-item project-tasks-summary-failed">
          {counts["failed"] ?? 0} failed
        </span>
      </div>

      {/* Tasks list */}
      <div className="project-tasks-list-wrap">
        {loading && tasks.length === 0 && (
          <div className="project-tasks-empty">
            <span className="project-tasks-empty-icon">⟳</span>
            <p className="project-tasks-empty-title">Loading tasks…</p>
          </div>
        )}
        {!loading && filtered.length === 0 && error === null && (
          <div className="project-tasks-empty">
            <span className="project-tasks-empty-icon">▱</span>
            <p className="project-tasks-empty-title">No tasks</p>
            <p className="project-tasks-empty-desc">
              No tasks match the current filters.
            </p>
          </div>
        )}
        {!loading && filtered.length > 0 && (
          <div className="project-tasks-list">
            {filtered.map((task) => (
              <div
                key={task.id}
                className={`project-tasks-item ${task.state} ${isTerminalTask(task) ? "terminal" : "active"}`}
                onClick={() => onOpenTask?.(task)}
                role="button"
                tabIndex={0}
                aria-label={task.user_task}
              >
                <div className="project-tasks-item-head">
                  <div className="project-tasks-item-info">
                    <h3 className="project-tasks-item-name">{task.user_task}</h3>
                    <p className="project-tasks-item-desc">
                      {task.plan?.summary || "No plan yet"}
                    </p>
                    <div className="project-tasks-item-meta">
                      <span className={`project-tasks-item-state ${task.state}`}>
                        {stateLabel(task.state)}
                      </span>
                      <span className="project-tasks-item-iteration">
                        Iteration {task.iteration} / {task.max_iterations}
                      </span>
                      {task.agent_name && (
                        <span className="project-tasks-item-agent">
                          {task.agent_name}
                        </span>
                      )}
                      <span className="project-tasks-item-updated">
                        {formatRelativeTime(task.updated_at)}
                      </span>
                    </div>
                  </div>
                  <div className="project-tasks-item-actions">
                    {!isTerminalTask(task) && task.state === "awaiting_approval" && (
                      <Button
                        role="secondary"
                        size="sm"
                        onClick={(e) => {
                          e.stopPropagation();
                          onOpenTask?.(task);
                        }}
                        title="Review plan"
                      >
                        Review
                      </Button>
                    )}
                    {!isTerminalTask(task) && task.state !== "awaiting_approval" && (
                      <Button
                        role="danger"
                        size="sm"
                        onClick={(e) => {
                          e.stopPropagation();
                          handleCancel(task.id);
                        }}
                        title="Cancel task"
                      >
                        Cancel
                      </Button>
                    )}
                    {isTerminalTask(task) && (
                      <Button
                        role="secondary"
                        size="sm"
                        onClick={(e) => {
                          e.stopPropagation();
                          handleRetry(task.id);
                        }}
                        title="Retry task"
                      >
                        Retry
                      </Button>
                    )}
                  </div>
                </div>
                {task.error && (
                  <div className="project-tasks-item-error">
                    {task.error}
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export default ProjectTasksScreen;
