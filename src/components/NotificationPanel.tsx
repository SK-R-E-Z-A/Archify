import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

export type NotificationRecord = {
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

export type NotificationFilter = {
  project_id?: string | null;
  task_id?: string | null;
  category?: string | null;
  severity?: string | null;
  unread_only?: boolean;
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

const SEVERITY_COLORS: Record<string, string> = {
  info: "#6aa6ff",
  success: "#73c69c",
  warning: "#f5c842",
  error: "#ff8a8a",
};

const SEVERITY_LABELS: Record<string, string> = {
  info: "Info",
  success: "Success",
  warning: "Warning",
  error: "Error",
};

const CATEGORY_LABELS: Record<string, string> = {
  task_submitted: "Task Submitted",
  task_queued: "Task Queued",
  planning_started: "Planning Started",
  planning_completed: "Planning Completed",
  execution_started: "Execution Started",
  agent_assigned: "Agent Assigned",
  verification_started: "Verification Started",
  verification_completed: "Verification Completed",
  verification_failed: "Verification Failed",
  review_completed: "Review Completed",
  correction_requested: "Correction Requested",
  retry_started: "Retry Started",
  retry_exhausted: "Retry Exhausted",
  task_completed: "Task Completed",
  task_failed: "Task Failed",
  task_cancelled: "Task Cancelled",
  task_interrupted: "Task Interrupted",
  task_recovered: "Task Recovered",
  task_blocked: "Task Blocked",
  provider_failure: "Provider Failure",
  project_error: "Project Error",
  task_progress: "Task Progress",
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

export function NotificationPanel({
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
  const [notifications, setNotifications] = useState<NotificationRecord[]>([]);
  const [unreadCount, setUnreadCount] = useState(0);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<NotificationFilter>({
    project_id: activeProjectId,
    unread_only: false,
    limit: 50,
    offset: 0,
  });
  const [hasMore, setHasMore] = useState(true);
  const [categories, setCategories] = useState<string[]>([]);
  const [severities, setSeverities] = useState<string[]>([]);

  const loadNotifications = useCallback(async (append = false) => {
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<NotificationRecord[]>("list_notifications", {
        filter: filter,
      });
      if (append) {
        setNotifications((prev) => [...prev, ...list]);
      } else {
        setNotifications(list);
      }
      setHasMore(list.length === (filter.limit ?? 50));
      // Extract unique categories and severities
      const cats = new Set<string>();
      const sevs = new Set<string>();
      list.forEach((n) => {
        if (n.category) cats.add(n.category);
        if (n.severity) sevs.add(n.severity);
      });
      setCategories(Array.from(cats).sort());
      setSeverities(Array.from(sevs).sort());
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load notifications");
    } finally {
      setLoading(false);
    }
  }, [filter]);

  const loadUnreadCount = useCallback(async () => {
    try {
      const count = await invoke<number>("get_notification_unread_count", {
        projectId: filter.project_id,
      });
      setUnreadCount(count);
    } catch (e) {
      console.error("Failed to load unread count:", e);
    }
  }, [filter.project_id]);

  useEffect(() => {
    if (isOpen) {
      setFilter((f) => ({ ...f, offset: 0 }));
      void loadNotifications(false);
      void loadUnreadCount();
    }
  }, [isOpen, filter.project_id, filter.category, filter.severity, filter.unread_only, loadNotifications, loadUnreadCount]);

  const handleLoadMore = () => {
    setFilter((f) => ({ ...f, offset: (f.offset ?? 0) + (f.limit ?? 50) }));
  };

  const handleMarkRead = async (id: string, read: boolean) => {
    try {
      await invoke("mark_notification_read", { id, read });
      setNotifications((prev) =>
        prev.map((n) => (n.id === id ? { ...n, read } : n))
      );
      if (read) setUnreadCount((c) => Math.max(0, c - 1));
      else setUnreadCount((c) => c + 1);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to update notification");
    }
  };

  const handleMarkAllRead = async () => {
    try {
      const count = await invoke<number>("mark_all_notifications_read", {
        projectId: filter.project_id,
      });
      setNotifications((prev) =>
        prev.map((n) =>
          filter.project_id && n.project_id !== filter.project_id ? n : { ...n, read: true }
        )
      );
      setUnreadCount((c) => Math.max(0, c - count));
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to mark all as read");
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await invoke("delete_notification", { id });
      setNotifications((prev) => prev.filter((n) => n.id !== id));
      // Note: unread count will be refreshed on next load
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to delete notification");
    }
  };

  const handleClear = async () => {
    if (!window.confirm("Clear all notifications? This cannot be undone.")) return;
    try {
      const cleared = await invoke<number>("clear_notifications", {
        projectId: filter.project_id,
      });
      setNotifications((prev) =>
        prev.filter((n) => filter.project_id && n.project_id !== filter.project_id)
      );
      setUnreadCount((c) => Math.max(0, c - cleared));
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to clear notifications");
    }
  };

  const handleNavigate = (notification: NotificationRecord) => {
    if (notification.task_id && onNavigateToTask) {
      onNavigateToTask(notification.task_id, notification.project_id);
      onClose();
    }
  };

  const projectName = (id: string | null | undefined) => {
    if (!id) return "Unknown project";
    return projects.find((p) => p.id === id)?.name ?? id;
  };

  if (!isOpen) return null;

  return (
    <div className="config-modal-backdrop" onClick={onClose}>
      <div
        className="config-modal notifications-modal"
        onClick={(e) => e.stopPropagation()}
        style={{ width: "min(900px, 96vw)", maxHeight: "90vh", display: "flex", flexDirection: "column" }}
      >
        <div className="config-modal-header" style={{ display: "flex", justifyContent: "space-between", alignItems: "center", flexWrap: "wrap", gap: 8 }}>
          <h2 style={{ margin: 0 }}>
            Notifications
            {unreadCount > 0 && (
              <span className="unread-badge" style={{ marginLeft: 8, background: "#ff8a8a", color: "#1a1d24", padding: "2px 8px", borderRadius: 12, fontSize: 12, fontWeight: 600 }}>
                {unreadCount} unread
              </span>
            )}
          </h2>
          <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
            <button className="secondary-button" onClick={() => void loadNotifications(false)} disabled={loading}>
              {loading ? "Loading..." : "Refresh"}
            </button>
            {notifications.length > 0 && (
              <button className="secondary-button" onClick={handleMarkAllRead} disabled={loading || unreadCount === 0}>
                Mark All Read
              </button>
            )}
            {notifications.length > 0 && (
              <button className="danger-button" onClick={handleClear} disabled={loading}>
                Clear All
              </button>
            )}
            <button className="icon-button small" onClick={onClose} title="Close">×</button>
          </div>
        </div>

        <div className="config-modal-body" style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
          {error && <div className="inline-error" role="alert" style={{ marginBottom: 12 }}>{error}</div>}

          {/* Filters */}
          <div className="notification-filters" style={{ display: "flex", flexWrap: "wrap", gap: 12, marginBottom: 12, padding: 12, background: "#1a1d24", borderRadius: 8, border: "1px solid #2f3542" }}>
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
              <span style={{ fontSize: 11, color: "#9aa0ad" }}>Category</span>
              <select
                value={filter.category ?? ""}
                onChange={(e) => setFilter((f) => ({ ...f, category: e.target.value || null, offset: 0 }))}
                disabled={loading}
                style={{ background: "#21252d", border: "1px solid #2f3542", borderRadius: 4, padding: "6px 8px", color: "#e3e5ec" }}
              >
                <option value="">All Categories</option>
                {categories.map((c) => (
                  <option key={c} value={c}>{CATEGORY_LABELS[c] || c}</option>
                ))}
              </select>
            </label>
            <label style={{ display: "flex", flexDirection: "column", gap: 4, minWidth: 140 }}>
              <span style={{ fontSize: 11, color: "#9aa0ad" }}>Severity</span>
              <select
                value={filter.severity ?? ""}
                onChange={(e) => setFilter((f) => ({ ...f, severity: e.target.value || null, offset: 0 }))}
                disabled={loading}
                style={{ background: "#21252d", border: "1px solid #2f3542", borderRadius: 4, padding: "6px 8px", color: "#e3e5ec" }}
              >
                <option value="">All Severities</option>
                {severities.map((s) => (
                  <option key={s} value={s}>{SEVERITY_LABELS[s] || s}</option>
                ))}
              </select>
            </label>
            <label style={{ display: "flex", alignItems: "flex-end", gap: 4, minWidth: 120 }}>
              <input
                type="checkbox"
                checked={filter.unread_only}
                onChange={(e) => setFilter((f) => ({ ...f, unread_only: e.target.checked, offset: 0 }))}
                disabled={loading}
              />
              <span style={{ fontSize: 11, color: "#9aa0ad" }}>Unread only</span>
            </label>
          </div>

          {/* Notifications List */}
          <div style={{ flex: 1, overflowY: "auto", minHeight: 0 }}>
            {loading && notifications.length === 0 && (
              <div style={{ padding: 24, textAlign: "center", color: "#9aa0ad" }}>Loading notifications…</div>
            )}
            {!loading && notifications.length === 0 && (
              <div style={{ padding: 24, textAlign: "center", color: "#9aa0ad", background: "#1a1d24", borderRadius: 8, border: "1px solid #2f3542" }}>
                <strong>No notifications</strong>
                <span style={{ display: "block", marginTop: 8 }}>Notifications from task lifecycle events will appear here.</span>
              </div>
            )}
            {!loading && notifications.length > 0 && (
              <div className="notification-list" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
                {notifications.map((notification) => (
                  <div
                    key={notification.id}
                    className={`notification-item ${!notification.read ? "unread" : ""}`}
                    style={{
                      display: "flex",
                      gap: 12,
                      padding: 12,
                      background: "#1a1d24",
                      border: "1px solid #2f3542",
                      borderRadius: 8,
                      borderLeft: `4px solid ${SEVERITY_COLORS[notification.severity] || "#6aa6ff"}`,
                      opacity: notification.read ? 0.7 : 1,
                    }}
                    onClick={() => handleNavigate(notification)}
                  >
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap", marginBottom: 4 }}>
                        <span className="notification-title" style={{ fontWeight: 600, color: "#e3e5ec", fontSize: 14 }}>{notification.title}</span>
                        <span className="notification-severity" style={{ fontSize: 10, padding: "2px 6px", borderRadius: 4, background: `${SEVERITY_COLORS[notification.severity] || "#6aa6ff"}22`, color: SEVERITY_COLORS[notification.severity] || "#6aa6ff", fontWeight: 500, textTransform: "uppercase" }}>
                          {SEVERITY_LABELS[notification.severity] || notification.severity}
                        </span>
                        <span className="notification-category" style={{ fontSize: 10, color: "#9aa0ad", background: "#21252d", padding: "2px 6px", borderRadius: 4 }}>
                          {CATEGORY_LABELS[notification.category] || notification.category}
                        </span>
                        <span className="notification-time" style={{ fontSize: 11, color: "#9aa0ad", marginLeft: "auto" }}>
                          {formatRelativeTime(notification.created_at)}
                        </span>
                      </div>
                      <p className="notification-message" style={{ margin: 0, color: "#b4b8c6", fontSize: 13, lineHeight: 1.4 }}>{notification.message}</p>
                      <div style={{ display: "flex", flexWrap: "wrap", gap: 8, marginTop: 8, fontSize: 11, color: "#9aa0ad" }}>
                        {notification.project_id && <span>Project: {projectName(notification.project_id)}</span>}
                        {notification.task_id && <span>Task: {notification.task_id.slice(0, 8)}…</span>}
                        {notification.agent_id && <span>Agent: {notification.agent_id}</span>}
                        {notification.session_id && <span>Session: {notification.session_id.slice(0, 8)}…</span>}
                      </div>
                    </div>
                    <div style={{ display: "flex", flexDirection: "column", gap: 4, alignItems: "flex-end" }}>
                      {!notification.read && (
                        <button
                          type="button"
                          className="secondary-button compact"
                          onClick={(e) => { e.stopPropagation(); handleMarkRead(notification.id, true); }}
                          style={{ fontSize: 11, padding: "4px 8px" }}
                        >
                          Mark Read
                        </button>
                      )}
                      {notification.read && (
                        <button
                          type="button"
                          className="secondary-button compact"
                          onClick={(e) => { e.stopPropagation(); handleMarkRead(notification.id, false); }}
                          style={{ fontSize: 11, padding: "4px 8px", color: "#f5c842", borderColor: "#f5c84244" }}
                        >
                          Mark Unread
                        </button>
                      )}
                      <button
                        type="button"
                        className="secondary-button compact"
                        onClick={(e) => { e.stopPropagation(); handleDelete(notification.id); }}
                        style={{ fontSize: 11, padding: "4px 8px", color: "#ff8a8a", borderColor: "#ff8a8a44" }}
                      >
                        Delete
                      </button>
                    </div>
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