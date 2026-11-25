import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

type TaskRecord = {
  id: string;
  user_task: string;
  state: string;
  error: string | null;
  updated_at: string;
  recovery_attempts: number;
};

interface RecoveryCenterScreenProps {
}

/**
 * Recovery Center — design.md §12.3.
 *
 * Review failed, stalled, and interrupted tasks. Supports retry and
 * cancellation of stalled executions. Every button uses the token-driven
 * Button component.
 */
export function RecoveryCenterScreen({}: RecoveryCenterScreenProps) {
  const [tasks, setTasks] = useState<TaskRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<TaskRecord[]>("list_tasks", {
        project_id: null,
      });
      setTasks(
        list.filter(
          (t) =>
            t.state === "failed" ||
            t.state === "cancelled" ||
            t.error != null ||
            t.recovery_attempts > 0,
        ),
      );
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load recovery tasks");
    } finally {
      setLoading(false);
    }
  }, []);

  const handleRetry = async (taskId: string) => {
    try {
      await invoke("retry_task", { taskId });
      setTasks((prev) =>
        prev.map((t) =>
          t.id === taskId ? { ...t, state: "retrying" } : t,
        ),
      );
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to retry task");
    }
  };

  const handleCancel = async (taskId: string) => {
    try {
      await invoke("cancel_task", { taskId });
      setTasks((prev) =>
        prev.map((t) =>
          t.id === taskId ? { ...t, state: "cancelled" } : t,
        ),
      );
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to cancel task");
    }
  };

  return (
    <div className="recovery-center-screen" role="main" aria-label="Recovery Center">
      <header className="recovery-center-head">
        <div>
          <h2 className="recovery-center-title">Recovery Center</h2>
          <p className="recovery-center-sub">
            Failed, stalled, and interrupted tasks.
          </p>
        </div>
        <div className="recovery-center-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={() => void load()}
            disabled={loading}
            title={loading ? "Refreshing…" : "Refresh"}
          >
            {loading ? "Refreshing…" : "Refresh"}
          </Button>
        </div>
      </header>

      {error && (
        <div className="recovery-center-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="recovery-center-list">
        {tasks.length === 0 && !loading && (
          <div className="recovery-center-empty">
            <span className="recovery-center-empty-icon">▱</span>
            <p className="recovery-center-empty-title">No recovery tasks</p>
            <p className="recovery-center-empty-desc">
              All tasks are running smoothly.
            </p>
          </div>
        )}
        {tasks.length > 0 && (
          <div className="recovery-center-items">
            {tasks.map((task) => (
              <div
                key={task.id}
                className="recovery-center-item"
              >
                <div className="recovery-center-item-info">
                  <h3 className="recovery-center-item-name">{task.user_task}</h3>
                  <span className={`recovery-center-item-state ${task.state}`}>
                    {task.state.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase())}
                  </span>
                  {task.error && (
                    <span className="recovery-center-item-error">{task.error}</span>
                  )}
                  <span className="recovery-center-item-attempts">
                    Attempt {task.recovery_attempts + 1}
                  </span>
                </div>
                <div className="recovery-center-item-actions">
                  <Button
                    role="secondary"
                    size="sm"
                    onClick={() => handleRetry(task.id)}
                    title="Retry task"
                  >
                    Retry
                  </Button>
                  <Button
                    role="danger"
                    size="sm"
                    onClick={() => handleCancel(task.id)}
                    title="Cancel task"
                  >
                    Cancel
                  </Button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export default RecoveryCenterScreen;
