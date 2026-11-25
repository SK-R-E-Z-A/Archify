import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

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

const CATEGORY_TABS: { id: string | null; label: string }[] = [
  { id: null, label: "All" },
  { id: "unread", label: "Unread" },
  { id: "errors", label: "Errors" },
  { id: "warnings", label: "Warnings" },
  { id: "tasks", label: "Tasks" },
  { id: "automations", label: "Automations" },
  { id: "system", label: "System" },
];

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

interface NotificationScreenProps {
  projects: ProjectRecord[];
  activeProjectId: string | null;
  onNavigateToTask?: (taskId: string, projectId: string | null) => void;
}

/**
 * Notification Center — design.md §22.
 *
 * Full-page destination promoted from the NotificationPanel modal. Shows
 * notifications with category tabs (All / Unread / Errors / Warnings /
 * Tasks / Automations / System), per-item mark-read/delete actions, and
 * bulk Mark All Read / Clear All. Every button uses the token-driven
 * Button component and respects theme / density / font-size presets.
 */
export function NotificationScreen({
  projects,
  activeProjectId,
  onNavigateToTask,
}: NotificationScreenProps) {
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
  const [activeTab, setActiveTab] = useState<string | null>(null);

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
    if (activeTab === "unread") {
      setFilter((f) => ({ ...f, unread_only: true, offset: 0 }));
    } else {
      setFilter((f) => ({ ...f, unread_only: false, offset: 0 }));
    }
    void loadNotifications(false);
    void loadUnreadCount();
  }, [
    activeTab,
    activeProjectId,
    filter.project_id,
    filter.category,
    filter.severity,
    loadNotifications,
    loadUnreadCount,
  ]);

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
      setError(null);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to mark all as read");
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await invoke("delete_notification", { id });
      setNotifications((prev) => prev.filter((n) => n.id !== id));
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to delete notification");
    }
  };

  const handleClear = async () => {
    if (
      !window.confirm("Clear all notifications? This cannot be undone.")
    )
      return;
    try {
      const cleared = await invoke<number>("clear_notifications", {
        projectId: filter.project_id,
      });
      setNotifications((prev) =>
        prev.filter((n) => filter.project_id && n.project_id !== filter.project_id)
      );
      setUnreadCount((c) => Math.max(0, c - cleared));
      setError(null);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to clear notifications");
    }
  };

  const handleNavigate = (notification: NotificationRecord) => {
    if (notification.task_id && onNavigateToTask) {
      onNavigateToTask(notification.task_id, notification.project_id);
    }
  };

  const projectName = (id: string | null | undefined) => {
    if (!id) return "Unknown project";
    return projects.find((p) => p.id === id)?.name ?? id;
  };

  // Apply category-tab filtering on top of the API filter
  const filtered = notifications.filter((n) => {
    if (activeTab === "unread") return !n.read;
    if (activeTab === "errors") return n.severity === "error";
    if (activeTab === "warnings") return n.severity === "warning";
    if (activeTab === "tasks") return n.category?.startsWith("task");
    if (activeTab === "automations")
      return n.category?.startsWith("automation") || n.category?.startsWith("retry");
    if (activeTab === "system")
      return n.category?.startsWith("provider") || n.category?.startsWith("project");
    return true;
  });

  if (error) {
    return (
      <div className="notification-screen" role="main" aria-label="Notification Center">
        <header className="notification-head">
          <div>
            <h2 className="notification-title">Notification Center</h2>
            <p className="notification-sub">
              Task, agent, automation, and system alerts.
            </p>
          </div>
        </header>
        <div className="notification-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button
            role="secondary"
            size="sm"
            onClick={() => {
              setError(null);
              void loadNotifications(false);
            }}
          >
            Retry
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="notification-screen" role="main" aria-label="Notification Center">
      <header className="notification-head">
        <div>
          <h2 className="notification-title">Notification Center</h2>
          <p className="notification-sub">
            Task, agent, automation, and system alerts.
          </p>
        </div>
        <div className="notification-actions">
          {unreadCount > 0 && (
            <Button
              role="secondary"
              size="sm"
              onClick={handleMarkAllRead}
              disabled={loading || unreadCount === 0}
              title="Mark all notifications as read"
            >
              Mark All Read
            </Button>
          )}
          {notifications.length > 0 && (
            <Button
              role="danger"
              size="sm"
              onClick={handleClear}
              disabled={loading}
              title="Clear all notifications"
            >
              Clear All
            </Button>
          )}
          <Button
            role="secondary"
            size="sm"
            onClick={() => void loadNotifications(false)}
            disabled={loading}
            title={loading ? "Refreshing…" : "Refresh notifications"}
          >
            {loading ? "Refreshing…" : "Refresh"}
          </Button>
        </div>
      </header>

      <div className="notification-count">
        {unreadCount > 0 ? (
          <span className="notification-unread">
            {unreadCount} unread
          </span>
        ) : (
          <span className="notification-all-read">All caught up</span>
        )}
      </div>

      {/* Category tabs */}
      <div className="notification-tabs">
        {CATEGORY_TABS.map((tab) => (
          <button
            key={tab.label}
            type="button"
            className={`notification-tab ${activeTab === tab.id ? "active" : ""}`}
            onClick={() => setActiveTab(tab.id)}
            role="tab"
            aria-selected={activeTab === tab.id}
          >
            {tab.label}
          </button>
        ))}
      </div>

      {/* Filters */}
      <div className="notification-filters">
        <div className="notification-filter-group">
          <label className="notification-filter-label">Project</label>
          <select
            value={filter.project_id ?? ""}
            onChange={(e) =>
              setFilter((f) => ({ ...f, project_id: e.target.value || null, offset: 0 }))
            }
            disabled={loading}
            className="notification-filter-select"
          >
            <option value="">All Projects</option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>{p.name}</option>
            ))}
          </select>
        </div>
        <div className="notification-filter-group">
          <label className="notification-filter-label">Category</label>
          <select
            value={filter.category ?? ""}
            onChange={(e) =>
              setFilter((f) => ({ ...f, category: e.target.value || null, offset: 0 }))
            }
            disabled={loading}
            className="notification-filter-select"
          >
            <option value="">All Categories</option>
            {categories.map((c) => (
              <option key={c} value={c}>{CATEGORY_LABELS[c] || c}</option>
            ))}
          </select>
        </div>
        <div className="notification-filter-group">
          <label className="notification-filter-label">Severity</label>
          <select
            value={filter.severity ?? ""}
            onChange={(e) =>
              setFilter((f) => ({ ...f, severity: e.target.value || null, offset: 0 }))
            }
            disabled={loading}
            className="notification-filter-select"
          >
            <option value="">All Severities</option>
            {severities.map((s) => (
              <option key={s} value={s}>{SEVERITY_LABELS[s] || s}</option>
            ))}
          </select>
        </div>
        <div className="notification-filter-group">
          <label className="notification-filter-checkbox">
            <input
              type="checkbox"
              checked={filter.unread_only ?? false}
              onChange={(e) =>
                setFilter((f) => ({ ...f, unread_only: e.target.checked, offset: 0 }))
              }
              disabled={loading}
            />
            Unread only
          </label>
        </div>
      </div>

      {/* Notifications List */}
      <div className="notification-list-wrap">
        {loading && notifications.length === 0 && (
          <div className="notification-empty">
            <span className="notification-empty-icon">⟳</span>
            <p className="notification-empty-title">Loading notifications…</p>
          </div>
        )}
        {!loading && filtered.length === 0 && error === null && (
          <div className="notification-empty">
            <span className="notification-empty-icon">▱</span>
            <p className="notification-empty-title">No notifications</p>
            <p className="notification-empty-desc">
              Notifications from task lifecycle events will appear here.
            </p>
          </div>
        )}
        {!loading && filtered.length > 0 && (
          <div className="notification-list">
            {filtered.map((notification) => (
              <div
                key={notification.id}
                className={`notification-item ${!notification.read ? "unread" : ""}`}
                onClick={() => handleNavigate(notification)}
                role="button"
                tabIndex={0}
                aria-label={notification.title}
              >
                <div className="notification-item-body">
                  <div className="notification-item-head">
                    <span className="notification-item-title">{notification.title}</span>
                    <span
                      className={`notification-item-severity ${notification.severity}`}
                    >
                      {SEVERITY_LABELS[notification.severity] || notification.severity}
                    </span>
                    <span className="notification-item-category">
                      {CATEGORY_LABELS[notification.category] || notification.category}
                    </span>
                    <span className="notification-item-time">
                      {formatRelativeTime(notification.created_at)}
                    </span>
                  </div>
                  <p className="notification-item-message">{notification.message}</p>
                  <div className="notification-item-tags">
                    {notification.project_id && (
                      <span className="notification-item-tag">
                        Project: {projectName(notification.project_id)}
                      </span>
                    )}
                    {notification.task_id && (
                      <span className="notification-item-tag">
                        Task: {notification.task_id.slice(0, 8)}…
                      </span>
                    )}
                    {notification.agent_id && (
                      <span className="notification-item-tag">Agent: {notification.agent_id}</span>
                    )}
                    {notification.session_id && (
                      <span className="notification-item-tag">
                        Session: {notification.session_id.slice(0, 8)}…
                      </span>
                    )}
                  </div>
                </div>
                <div className="notification-item-actions">
                  {!notification.read && (
                    <Button
                      role="secondary"
                      size="sm"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleMarkRead(notification.id, true);
                      }}
                      title="Mark as read"
                    >
                      Mark Read
                    </Button>
                  )}
                  {notification.read && (
                    <Button
                      role="secondary"
                      size="sm"
                      onClick={(e) => {
                        e.stopPropagation();
                        handleMarkRead(notification.id, false);
                      }}
                      title="Mark as unread"
                    >
                      Mark Unread
                    </Button>
                  )}
                  <Button
                    role="danger"
                    size="sm"
                    onClick={(e) => {
                      e.stopPropagation();
                      handleDelete(notification.id);
                    }}
                    title="Delete"
                  >
                    Delete
                  </Button>
                </div>
              </div>
            ))}
            {hasMore && (
              <div className="notification-load-more">
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

export default NotificationScreen;
