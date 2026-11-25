import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

export type ActivityRecord = {
  id: string;
  timestamp: string;
  event_type: string;
  summary: string;
  project_id: string | null;
  task_id: string | null;
  agent_id: string | null;
  session_id: string | null;
  iteration: number | null;
  outcome: string;
  dedup_key: string | null;
  metadata: Record<string, unknown> | null;
};

type ActivityFilter = {
  project_id?: string | null;
  task_id?: string | null;
  event_type?: string | null;
  outcome?: string | null;
  since?: string | null;
  until?: string | null;
  search?: string | null;
  limit?: number;
  offset?: number;
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

const OUTCOME_COLORS: Record<string, string> = {
  success: "#73c69c",
  failure: "#ff8a8a",
  cancelled: "#f5c842",
  info: "#6aa6ff",
};

const OUTCOME_LABELS: Record<string, string> = {
  success: "Success",
  failure: "Failure",
  cancelled: "Cancelled",
  info: "Info",
};

const EVENT_LABELS: Record<string, string> = {
  task_queued: "Task Queued",
  planning_started: "Planning Started",
  awaiting_approval: "Awaiting Approval",
  execution_started: "Execution Started",
  verification_started: "Verification Started",
  verification_completed: "Verification Completed",
  review_started: "Review Started",
  review_completed: "Review Completed",
  retry_started: "Retry Started",
  task_completed: "Task Completed",
  task_failed: "Task Failed",
  task_cancelled: "Task Cancelled",
  component_finished: "Component Finished",
  component_failed: "Component Failed",
  processstarted: "Process Started",
  processcompleted: "Process Completed",
  processfailed: "Process Failed",
  processtimedout: "Process Timed Out",
  stall_detected: "Stall Detected",
  processcancelled: "Process Cancelled",
};

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
  const dateStr = date.toLocaleDateString();
  return dateStr;
}

function getEventTypeLabel(eventType: string): string {
  return (
    EVENT_LABELS[eventType] ||
    eventType.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase())
  );
}

interface ProjectActivityScreenProps {
  project: ProjectRecord | null;
  onNavigateToTask?: (taskId: string, projectId: string | null) => void;
}

/**
 * Project Activity screen — design.md §23.2.
 *
 * Project-scoped activity feed showing lifecycle events for tasks,
 * agents, and automations within this project. Every button uses the
 * token-driven Button component and respects theme / density / font-size
 * presets.
 */
export function ProjectActivityScreen({
  project,
  onNavigateToTask,
}: ProjectActivityScreenProps) {
  const [activity, setActivity] = useState<ActivityRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<ActivityFilter>({
    project_id: project?.id ?? null,
    limit: 50,
    offset: 0,
  });
  const [hasMore, setHasMore] = useState(true);
  const [eventTypes, setEventTypes] = useState<string[]>([]);
  const [outcomes, setOutcomes] = useState<string[]>([]);

  const loadActivity = useCallback(async (append = false) => {
    if (!project) return;
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<ActivityRecord[]>("list_activity", {
        filter: { ...filter, project_id: project.id },
      });
      if (append) {
        setActivity((prev) => [...prev, ...list]);
      } else {
        setActivity(list);
      }
      setHasMore(list.length === (filter.limit ?? 50));
      const types = new Set<string>();
      const outs = new Set<string>();
      list.forEach((a) => {
        if (a.event_type) types.add(a.event_type);
        if (a.outcome) outs.add(a.outcome);
      });
      setEventTypes(Array.from(types).sort());
      setOutcomes(Array.from(outs).sort());
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load activity");
    } finally {
      setLoading(false);
    }
  }, [project, filter]);

  useEffect(() => {
    setFilter((f) => ({ ...f, offset: 0 }));
    void loadActivity(false);
  }, [project, loadActivity]);

  const handleLoadMore = () => {
    setFilter((f) => ({ ...f, offset: (f.offset ?? 0) + (f.limit ?? 50) }));
  };

  const handleSearch = (query: string) => {
    setFilter((f) => ({ ...f, search: query || null, offset: 0 }));
  };

  const handleClearSearch = () => {
    setFilter((f) => ({ ...f, search: null, offset: 0 }));
  };

  if (error) {
    return (
      <div className="project-activity-screen" role="main" aria-label="Project Activity">
        <header className="project-activity-head">
          <div>
            <h2 className="project-activity-title">{project?.name || "Project Activity"}</h2>
            <p className="project-activity-sub">Lifecycle events for this project.</p>
          </div>
        </header>
        <div className="project-activity-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => { setError(null); void loadActivity(false); }} title="Retry">
            Retry
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="project-activity-screen" role="main" aria-label="Project Activity">
      <header className="project-activity-head">
        <div>
          <h2 className="project-activity-title">{project?.name || "Project Activity"}</h2>
          <p className="project-activity-sub">Lifecycle events for this project.</p>
        </div>
        <div className="project-activity-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={() => void loadActivity(false)}
            disabled={loading}
            title={loading ? "Refreshing…" : "Refresh activity"}
          >
            {loading ? "Refreshing…" : "Refresh"}
          </Button>
        </div>
      </header>

      {/* Filters */}
      <div className="project-activity-filters">
        <div className="project-activity-filter-group">
          <label className="project-activity-filter-label">Event Type</label>
          <select
            value={filter.event_type ?? ""}
            onChange={(e) =>
              setFilter((f) => ({ ...f, event_type: e.target.value || null, offset: 0 }))
            }
            disabled={loading}
            className="project-activity-filter-select"
          >
            <option value="">All Types</option>
            {eventTypes.map((t) => (
              <option key={t} value={t}>{getEventTypeLabel(t)}</option>
            ))}
          </select>
        </div>
        <div className="project-activity-filter-group">
          <label className="project-activity-filter-label">Outcome</label>
          <select
            value={filter.outcome ?? ""}
            onChange={(e) =>
              setFilter((f) => ({ ...f, outcome: e.target.value || null, offset: 0 }))
            }
            disabled={loading}
            className="project-activity-filter-select"
          >
            <option value="">All Outcomes</option>
            {outcomes.map((o) => (
              <option key={o} value={o}>{OUTCOME_LABELS[o] || o}</option>
            ))}
          </select>
        </div>
        <div className="project-activity-filter-group project-activity-filter-group-wide">
          <label className="project-activity-filter-label">Search</label>
          <div className="project-activity-search">
            <input
              type="text"
              placeholder="Search summary…"
              value={filter.search ?? ""}
              onChange={(e) => handleSearch(e.target.value)}
              disabled={loading}
              className="project-activity-search-input"
            />
            {filter.search && (
              <Button
                role="ghost"
                size="sm"
                onClick={handleClearSearch}
                disabled={loading}
                title="Clear search"
              >
                ×
              </Button>
            )}
          </div>
        </div>
      </div>

      {/* Activity List */}
      <div className="project-activity-list-wrap">
        {loading && activity.length === 0 && (
          <div className="project-activity-empty">
            <span className="project-activity-empty-icon">⟳</span>
            <p className="project-activity-empty-title">Loading activity…</p>
          </div>
        )}
        {!loading && activity.length === 0 && error === null && (
          <div className="project-activity-empty">
            <span className="project-activity-empty-icon">▱</span>
            <p className="project-activity-empty-title">No activity recorded</p>
            <p className="project-activity-empty-desc">
              Activity from task lifecycle events will appear here.
            </p>
          </div>
        )}
        {!loading && activity.length > 0 && (
          <div className="project-activity-list">
            {activity.map((record) => (
              <div
                key={record.id}
                className={`project-activity-item ${record.outcome}`}
                onClick={() =>
                  record.task_id && onNavigateToTask
                    ? onNavigateToTask(record.task_id, record.project_id)
                    : null
                }
                role="button"
                tabIndex={0}
                aria-label={getEventTypeLabel(record.event_type)}
              >
                <div className="project-activity-item-meta">
                  <div
                    className="project-activity-item-dot"
                    style={{
                      backgroundColor: OUTCOME_COLORS[record.outcome] || "#6aa6ff",
                    }}
                  />
                  <span className="project-activity-item-time">
                    {formatRelativeTime(record.timestamp)}
                  </span>
                </div>
                <div className="project-activity-item-body">
                  <div className="project-activity-item-head">
                    <span className="project-activity-item-event">
                      {getEventTypeLabel(record.event_type)}
                    </span>
                    <span
                      className={`project-activity-item-outcome ${record.outcome}`}
                    >
                      {OUTCOME_LABELS[record.outcome] || record.outcome}
                    </span>
                    {record.iteration !== null && (
                      <span className="project-activity-item-iteration">
                        Iteration {record.iteration}
                      </span>
                    )}
                  </div>
                  <p className="project-activity-item-summary">{record.summary}</p>
                  <div className="project-activity-item-tags">
                    {record.task_id && (
                      <span className="project-activity-item-tag">
                        Task: {record.task_id.slice(0, 8)}…
                      </span>
                    )}
                    {record.agent_id && (
                      <span className="project-activity-item-tag">Agent: {record.agent_id}</span>
                    )}
                    {record.session_id && (
                      <span className="project-activity-item-tag">
                        Session: {record.session_id.slice(0, 8)}…
                      </span>
                    )}
                  </div>
                </div>
                {record.task_id && (
                  <Button
                    role="secondary"
                    size="sm"
                    onClick={(e) => {
                      e.stopPropagation();
                      onNavigateToTask?.(record.task_id!, record.project_id);
                    }}
                    title={`Open task ${record.task_id!.slice(0, 8)}`}
                  >
                    Open Task
                  </Button>
                )}
              </div>
            ))}
            {hasMore && (
              <div className="project-activity-load-more">
                <Button
                  role="secondary"
                  onClick={handleLoadMore}
                  disabled={loading}
                  block
                >
                  {loading ? "Loading…" : "Load More"}
                </Button>
              </div>
            )}
          </div>
        )}
      </div>
    </div>
  );
}

export default ProjectActivityScreen;
