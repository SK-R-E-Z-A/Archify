import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

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

export type ActivityFilter = {
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

export function ActivityFeed({
  isOpen,
  onClose,
  projects,
  activeProjectId,
  onNavigateToTask,
}: {
  isOpen: boolean;
  onClose: () => void;
  projects: ProjectRecord[];
  activeProjectId: string | null;
  onNavigateToTask?: (taskId: string, projectId: string | null) => void;
}) {
  const [activity, setActivity] = useState<ActivityRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<ActivityFilter>({
    project_id: activeProjectId,
    limit: 50,
    offset: 0,
  });
  const [hasMore, setHasMore] = useState(true);
  const [eventTypes, setEventTypes] = useState<string[]>([]);
  const [outcomes, setOutcomes] = useState<string[]>([]);

  const loadActivity = useCallback(async (append = false) => {
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<ActivityRecord[]>("list_activity", {
        filter: filter,
      });
      if (append) {
        setActivity((prev) => [...prev, ...list]);
      } else {
        setActivity(list);
      }
      setHasMore(list.length === (filter.limit ?? 50));
      // Extract unique event types and outcomes
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
  }, [filter]);

  useEffect(() => {
    if (isOpen) {
      setFilter((f) => ({ ...f, offset: 0 }));
      void loadActivity(false);
    }
  }, [isOpen, filter.project_id, filter.task_id, filter.event_type, filter.outcome, filter.since, filter.until, filter.search, loadActivity]);

  const handleLoadMore = () => {
    setFilter((f) => ({ ...f, offset: (f.offset ?? 0) + (f.limit ?? 50) }));
  };

  const handleSearch = (query: string) => {
    setFilter((f) => ({ ...f, search: query || null, offset: 0 }));
  };

  const handleClearSearch = () => {
    setFilter((f) => ({ ...f, search: null, offset: 0 }));
  };

  const projectName = (id: string | null | undefined) => {
    if (!id) return "Unknown project";
    return projects.find((p) => p.id === id)?.name ?? id;
  };

  const getEventTypeLabel = (eventType: string) => {
    const labels: Record<string, string> = {
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
    return labels[eventType] || eventType.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
  };

  if (!isOpen) return null;

  return (
    <div className="config-modal-backdrop" onClick={onClose}>
      <div
        className="config-modal activity-modal"
        onClick={(e) => e.stopPropagation()}
        style={{ width: "min(1000px, 96vw)", maxHeight: "90vh", display: "flex", flexDirection: "column" }}
      >
        <div className="config-modal-header" style={{ display: "flex", justifyContent: "space-between", alignItems: "center", flexWrap: "wrap", gap: 8 }}>
          <h2 style={{ margin: 0 }}>Activity Feed</h2>
          <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <button className="secondary-button" onClick={() => void loadActivity(false)} disabled={loading}>
              {loading ? "Loading..." : "Refresh"}
            </button>
            <button className="icon-button small" onClick={onClose} title="Close">×</button>
          </div>
        </div>

        <div className="config-modal-body" style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
          {error && <div className="inline-error" role="alert" style={{ marginBottom: 12 }}>{error}</div>}

          {/* Filters */}
          <div className="activity-filters" style={{ display: "flex", flexWrap: "wrap", gap: 12, marginBottom: 12, padding: 12, background: "#1a1d24", borderRadius: 8, border: "1px solid #2f3542" }}>
            <label style={{ display: "flex", flexDirection: "column", gap: 4, minWidth: 180 }}>
              <span style={{ fontSize: 11, color: "#9aa0ad" }}>Project</span>
              <select
                value={filter.project_id ?? ""}
                onChange={(e) => setFilter((f) => ({ ...f, project_id: e.target.value || null, offset: 0 }))}
                disabled={loading}
                style={{ background: "#21252d", border: "1px solid #2f3542", borderRadius: 4, padding: "6px 8px", color: "#e3e5ec" }}
              >
                <option value="">All Projects</option>
                {projects.map((p) => (
                  <option key={p.id} value={p.id}>{p.name}</option>
                ))}
              </select>
            </label>
            <label style={{ display: "flex", flexDirection: "column", gap: 4, minWidth: 160 }}>
              <span style={{ fontSize: 11, color: "#9aa0ad" }}>Event Type</span>
              <select
                value={filter.event_type ?? ""}
                onChange={(e) => setFilter((f) => ({ ...f, event_type: e.target.value || null, offset: 0 }))}
                disabled={loading}
                style={{ background: "#21252d", border: "1px solid #2f3542", borderRadius: 4, padding: "6px 8px", color: "#e3e5ec" }}
              >
                <option value="">All Types</option>
                {eventTypes.map((t) => (
                  <option key={t} value={t}>{getEventTypeLabel(t)}</option>
                ))}
              </select>
            </label>
            <label style={{ display: "flex", flexDirection: "column", gap: 4, minWidth: 140 }}>
              <span style={{ fontSize: 11, color: "#9aa0ad" }}>Outcome</span>
              <select
                value={filter.outcome ?? ""}
                onChange={(e) => setFilter((f) => ({ ...f, outcome: e.target.value || null, offset: 0 }))}
                disabled={loading}
                style={{ background: "#21252d", border: "1px solid #2f3542", borderRadius: 4, padding: "6px 8px", color: "#e3e5ec" }}
              >
                <option value="">All Outcomes</option>
                {outcomes.map((o) => (
                  <option key={o} value={o}>{OUTCOME_LABELS[o] || o}</option>
                ))}
              </select>
            </label>
            <div style={{ display: "flex", flexDirection: "column", gap: 4, minWidth: 160 }}>
              <span style={{ fontSize: 11, color: "#9aa0ad" }}>Search</span>
              <div style={{ display: "flex", gap: 4 }}>
                <input
                  type="text"
                  placeholder="Search summary..."
                  value={filter.search ?? ""}
                  onChange={(e) => handleSearch(e.target.value)}
                  disabled={loading}
                  style={{ flex: 1, background: "#21252d", border: "1px solid #2f3542", borderRadius: 4, padding: "6px 8px", color: "#e3e5ec" }}
                />
                {filter.search && (
                  <button
                    type="button"
                    className="icon-button small"
                    onClick={handleClearSearch}
                    disabled={loading}
                    title="Clear search"
                  >
                    ×
                  </button>
                )}
              </div>
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 4, minWidth: 160 }}>
              <span style={{ fontSize: 11, color: "#9aa0ad" }}>Since</span>
              <input
                type="datetime-local"
                value={filter.since ? new Date(filter.since).toISOString().slice(0, 16) : ""}
                onChange={(e) => setFilter((f) => ({ ...f, since: e.target.value || null, offset: 0 }))}
                disabled={loading}
                style={{ background: "#21252d", border: "1px solid #2f3542", borderRadius: 4, padding: "6px 8px", color: "#e3e5ec" }}
              />
            </div>
            <div style={{ display: "flex", flexDirection: "column", gap: 4, minWidth: 160 }}>
              <span style={{ fontSize: 11, color: "#9aa0ad" }}>Until</span>
              <input
                type="datetime-local"
                value={filter.until ? new Date(filter.until).toISOString().slice(0, 16) : ""}
                onChange={(e) => setFilter((f) => ({ ...f, until: e.target.value || null, offset: 0 }))}
                disabled={loading}
                style={{ background: "#21252d", border: "1px solid #2f3542", borderRadius: 4, padding: "6px 8px", color: "#e3e5ec" }}
              />
            </div>
          </div>

          {/* Activity List */}
          <div style={{ flex: 1, overflowY: "auto", minHeight: 0 }}>
            {loading && activity.length === 0 && (
              <div style={{ padding: 24, textAlign: "center", color: "#9aa0ad" }}>Loading activity…</div>
            )}
            {!loading && activity.length === 0 && (
              <div style={{ padding: 24, textAlign: "center", color: "#9aa0ad", background: "#1a1d24", borderRadius: 8, border: "1px solid #2f3542" }}>
                <strong>No activity recorded</strong>
                <span style={{ display: "block", marginTop: 8 }}>Activity from task lifecycle events will appear here.</span>
              </div>
            )}
            {!loading && activity.length > 0 && (
              <div className="activity-list" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                {activity.map((record) => (
                  <div
                    key={record.id}
                    className="activity-item"
                    style={{
                      display: "flex",
                      gap: 12,
                      padding: 12,
                      background: "#1a1d24",
                      border: "1px solid #2f3542",
                      borderRadius: 8,
                      borderLeft: `4px solid ${OUTCOME_COLORS[record.outcome] || "#6aa6ff"}`,
                    }}
                    onClick={() => record.task_id && onNavigateToTask?.(record.task_id, record.project_id)}
                  >
                    <div style={{ width: 32, flexShrink: 0, textAlign: "center" }}>
                      <div style={{ width: 10, height: 10, borderRadius: "50%", background: OUTCOME_COLORS[record.outcome] || "#6aa6ff", margin: "0 auto 4px" }} />
                      <span style={{ fontSize: 10, color: "#9aa0ad" }}>{formatRelativeTime(record.timestamp)}</span>
                    </div>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", marginBottom: 4 }}>
                        <span className="activity-event-type" style={{ fontWeight: 600, color: "#e3e5ec", fontSize: 14 }}>{getEventTypeLabel(record.event_type)}</span>
                        <span className="activity-outcome" style={{ fontSize: 10, padding: "2px 6px", borderRadius: 4, background: `${(OUTCOME_COLORS[record.outcome] || "#6aa6ff")}22`, color: OUTCOME_COLORS[record.outcome] || "#6aa6ff", fontWeight: 500, textTransform: "capitalize" }}>
                          {OUTCOME_LABELS[record.outcome] || record.outcome}
                        </span>
                        {record.iteration !== null && (
                          <span className="activity-iteration" style={{ fontSize: 10, color: "#9aa0ad", background: "#21252d", padding: "2px 6px", borderRadius: 4 }}>
                            Iteration {record.iteration}
                          </span>
                        )}
                      </div>
                      <p className="activity-summary" style={{ margin: 0, color: "#b4b8c6", fontSize: 13, lineHeight: 1.4 }}>{record.summary}</p>
                      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginTop: 8, fontSize: 11, color: "#9aa0ad" }}>
                        {record.project_id && <span>Project: {projectName(record.project_id)}</span>}
                        {record.task_id && <span>Task: {record.task_id.slice(0, 8)}…</span>}
                        {record.agent_id && <span>Agent: {record.agent_id}</span>}
                        {record.session_id && <span>Session: {record.session_id.slice(0, 8)}…</span>}
                      </div>
                    </div>
                    {record.task_id && (
                      <button
                        type="button"
                        className="secondary-button compact"
                        onClick={(e) => {
                          e.stopPropagation();
                          if (record.task_id) {
                            onNavigateToTask?.(record.task_id, record.project_id);
                          }
                        }}
                        style={{ fontSize: 11, padding: "4px 8px", alignSelf: "flex-start" }}
                      >
                        Open Task
                      </button>
                    )}
                  </div>
                ))}
                {hasMore && (
                  <div style={{ textAlign: "center", marginTop: 8 }}>
                    <button className="secondary-button" onClick={handleLoadMore} disabled={loading}>
                      {loading ? "Loading..." : "Load More"}
                    </button>
                  </div>
                )}
              </div>
            )}
          </div>
        </div>

        <div className="config-modal-footer" style={{ display: "flex", justifyContent: "flex-end", gap: 8, paddingTop: 12, borderTop: "1px solid #2f3542" }}>
          <button className="secondary-button" onClick={onClose}>Close</button>
        </div>
      </div>
    </div>
  );
}