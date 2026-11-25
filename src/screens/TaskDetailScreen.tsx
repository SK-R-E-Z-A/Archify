import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

type TaskRecord = {
  id: string;
  user_task: string;
  project_root: string;
  state: string;
  iteration: number;
  max_iterations: number;
  created_at: string;
  updated_at: string;
  error: string | null;
  project_id: string | null;
  agent_name: string | null;
  priority: number;
  recovery_attempts: number;
};

type TaskGraph = {
  components: Array<{
    id: string;
    description: string;
    state: string;
    depends_on: string[];
    error: string | null;
  }>;
};

interface TaskDetailScreenProps {
  taskId: string;
}

/**
 * Task Detail screen — design.md §32.
 *
 * Full detail view of a single task: graph, iterations, changed files,
 * verification results, and review decisions. Every button uses the
 * token-driven Button component.
 */
export function TaskDetailScreen({ taskId }: TaskDetailScreenProps) {
  const [task, setTask] = useState<TaskRecord | null>(null);
  const [graph, setGraph] = useState<TaskGraph | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [t, g] = await Promise.all([
        invoke<TaskRecord>("get_task_status", { taskId }),
        invoke<TaskGraph>("get_task_timeline", { task_id: taskId }),
      ]);
      setTask(t ?? null);
      setGraph(g ?? null);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load task detail");
    } finally {
      setLoading(false);
    }
  }, [taskId]);

  useEffect(() => {
    void load();
  }, [load]);

  const handleCancel = async () => {
    if (!task) return;
    if (!window.confirm("Cancel this task? This cannot be undone.")) return;
    try {
      await invoke("cancel_task", { taskId: task.id });
      void load();
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to cancel task");
    }
  };

  const handleRetry = async () => {
    if (!task) return;
    try {
      await invoke("retry_task", { taskId: task.id });
      void load();
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to retry task");
    }
  };

  if (error) {
    return (
      <div className="task-detail-screen" role="main" aria-label="Task Detail">
        <header className="task-detail-head">
          <div>
            <h2 className="task-detail-title">Task Detail</h2>
            <p className="task-detail-sub">Task #{taskId}</p>
          </div>
        </header>
        <div className="task-detail-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => { setError(null); void load(); }} title="Retry">
            Retry
          </Button>
        </div>
      </div>
    );
  }

  if (loading || !task) {
    return (
      <div className="task-detail-screen" role="main" aria-label="Task Detail">
        <header className="task-detail-head">
          <div>
            <h2 className="task-detail-title">Task Detail</h2>
            <p className="task-detail-sub">Task #{taskId}</p>
          </div>
        </header>
        <div className="task-detail-empty">
          <span className="task-detail-empty-icon">⟳</span>
          <p className="task-detail-empty-title">Loading task…</p>
        </div>
      </div>
    );
  }

  const isTerminal = ["completed", "failed", "cancelled"].includes(task.state);

  const graphJsx = graph ? (
    <div className="task-detail-section">
      <h3 className="task-detail-section-title">Execution Graph</h3>
      <div className="task-detail-graph">
        {graph.components.map((component, index) => (
          <div
            key={component.id}
            className={`task-detail-graph-step ${component.state}`}
          >
            <span className="task-detail-graph-step-index">{index + 1}</span>
            <span className="task-detail-graph-step-desc">{component.description}</span>
            <span className="task-detail-graph-step-state">
              {component.state.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase())}
            </span>
            {component.error && (
              <span className="task-detail-graph-step-error">{component.error}</span>
            )}
          </div>
        ))}
      </div>
    </div>
  ) : null;

  return (
    <div className="task-detail-screen" role="main" aria-label="Task Detail">
      <header className="task-detail-head">
        <div>
          <h2 className="task-detail-title">{task.user_task}</h2>
          <span className={`task-detail-state ${task.state}`}>
            {task.state.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase())}
          </span>
        </div>
        <div className="task-detail-actions">
          {!isTerminal && (
            <Button
              role="danger"
              size="sm"
              onClick={handleCancel}
              title="Cancel task"
            >
              Cancel
            </Button>
          )}
          {isTerminal && (
            <Button
              role="secondary"
              size="sm"
              onClick={handleRetry}
              title="Retry task"
            >
              Retry
            </Button>
          )}
        </div>
      </header>

      <div className="task-detail-meta">
        <span className="task-detail-meta-item">
          Iteration {task.iteration} / {task.max_iterations}
        </span>
        <span className="task-detail-meta-item">
          {task.agent_name ?? "No agent"}
        </span>
        <span className="task-detail-meta-item">
          {new Date(task.updated_at).toLocaleString()}
        </span>
      </div>

      {graphJsx}

      {task.error && (
        <div className="task-detail-section task-detail-section-error">
          <h3 className="task-detail-section-title">Error</h3>
          <p className="task-detail-error-text">{task.error}</p>
        </div>
      )}
    </div>
  );
}

export default TaskDetailScreen;
