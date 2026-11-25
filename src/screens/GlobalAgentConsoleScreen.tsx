import { useCallback, useEffect, useMemo, useState } from "react";
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
};

type ProjectRecord = {
  id: string;
  name: string;
  root: string;
};

interface GlobalAgentConsoleScreenProps {
}

/**
 * Global Agent Console — design.md §11.2.
 *
 * Cross-project view of all agents and tasks. Shows per-project agent
 * status, active tasks, and global controls. Every button uses the
 * token-driven Button component.
 */
export function GlobalAgentConsoleScreen({}: GlobalAgentConsoleScreenProps) {
  const [tasks, setTasks] = useState<TaskRecord[]>([]);
  const [projects, setProjects] = useState<ProjectRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [taskList, projectList] = await Promise.all([
        invoke<TaskRecord[]>("list_tasks", { project_id: null }),
        invoke<ProjectRecord[]>("list_projects", {}),
      ]);
      setTasks(taskList);
      setProjects(projectList);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load agent console");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const activeTasks = tasks.filter((t) =>
    !["completed", "failed", "cancelled"].includes(t.state),
  );

  const byProject = useMemo(() => {
    const map = new Map<string, TaskRecord[]>();
    tasks.forEach((t) => {
      const pid = t.project_id ?? "global";
      const arr = map.get(pid) ?? [];
      arr.push(t);
      map.set(pid, arr);
    });
    return map;
  }, [tasks]);

  return (
    <div className="global-agent-console-screen" role="main" aria-label="Global Agent Console">
      <header className="global-agent-console-head">
        <div>
          <h2 className="global-agent-console-title">Global Agent Console</h2>
          <p className="global-agent-console-sub">
            Cross-project agent and task overview.
          </p>
        </div>
        <div className="global-agent-console-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={() => void load()}
            disabled={loading}
            title={loading ? "Refreshing…" : "Refresh console"}
          >
            {loading ? "Refreshing…" : "Refresh"}
          </Button>
        </div>
      </header>

      {error && (
        <div className="global-agent-console-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="global-agent-console-summary">
        <span className="global-agent-console-summary-item">
          {tasks.length} total tasks
        </span>
        <span className="global-agent-console-summary-item active">
          {activeTasks.length} active
        </span>
        <span className="global-agent-console-summary-item projects">
          {projects.length} projects
        </span>
      </div>

      <div className="global-agent-console-list">
        {[...byProject.entries()].map(([projectId, projectTasks]) => {
          const project =
            projects.find((p) => p.id === projectId) ||
            (projectId === "global"
              ? { id: "global", name: "Global", root: "" }
              : null);
          if (!project) return null;
          return (
            <div
              key={projectId}
              className="global-agent-console-project"
            >
              <h3 className="global-agent-console-project-name">
                {project.name}
              </h3>
              <div className="global-agent-console-task-list">
                {projectTasks.map((task) => (
                  <div
                    key={task.id}
                    className={`global-agent-console-task ${task.state}`}
                  >
                    <span className="global-agent-console-task-name">
                      {task.user_task}
                    </span>
                    <span className={`global-agent-console-task-state ${task.state}`}>
                      {task.state.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase())}
                    </span>
                    <span className="global-agent-console-task-iteration">
                      {task.iteration}/{task.max_iterations}
                    </span>
                  </div>
                ))}
              </div>
            </div>
          );
        })}
      </div>

      {tasks.length === 0 && !loading && (
        <div className="global-agent-console-empty">
          <span className="global-agent-console-empty-icon">▱</span>
          <p className="global-agent-console-empty-title">No tasks</p>
          <p className="global-agent-console-empty-desc">
            Tasks will appear here as they are created across projects.
          </p>
        </div>
      )}
    </div>
  );
}

export default GlobalAgentConsoleScreen;
