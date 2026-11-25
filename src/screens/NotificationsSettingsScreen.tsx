import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

interface NotificationsSettingsScreenProps {
}

/**
 * Notifications settings screen — design.md §22.4 / §24.7.
 *
 * Configure notification behavior: categories, severities, unread-only
 * filtering, and bulk actions. Every button uses the token-driven Button
 * component.
 */
export function NotificationsSettingsScreen({}: NotificationsSettingsScreenProps) {
  const [categories, setCategories] = useState<Record<string, boolean>>({
    task_submitted: true,
    task_queued: true,
    planning_started: true,
    execution_started: true,
    verification_started: true,
    review_completed: true,
    task_completed: true,
    task_failed: true,
    provider_failure: true,
    system: true,
  });
  const [severities, setSeverities] = useState<Record<string, boolean>>({
    info: true,
    success: true,
    warning: true,
    error: true,
  });
  const [unreadOnly, setUnreadOnly] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSave = async () => {
    setLoading(true);
    setError(null);
    try {
      // Notification preferences are persisted in a later milestone
      setLoading(false);
    } catch (e) {
      setLoading(false);
      setError(typeof e === "string" ? e : "Failed to save notification settings");
    }
  };

  const handleMarkAllRead = async () => {
    try {
      await invoke("mark_all_notifications_read", { projectId: null });
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to mark all as read");
    }
  };

  const handleClearAll = async () => {
    if (!window.confirm("Clear all notifications? This cannot be undone.")) return;
    try {
      await invoke("clear_notifications", { projectId: null });
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to clear notifications");
    }
  };

  return (
    <div className="notifications-settings-screen" role="main" aria-label="Notifications Settings">
      <header className="notifications-settings-head">
        <div>
          <h2 className="notifications-settings-title">Notifications</h2>
          <p className="notifications-settings-sub">Configure notification behavior.</p>
        </div>
        <div className="notifications-settings-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={handleMarkAllRead}
            disabled={loading}
            title="Mark all as read"
          >
            Mark All Read
          </Button>
          <Button
            role="danger"
            size="sm"
            onClick={handleClearAll}
            disabled={loading}
            title="Clear all"
          >
            Clear All
          </Button>
          <Button
            role="primary"
            size="sm"
            onClick={handleSave}
            disabled={loading}
            title={loading ? "Saving…" : "Save settings"}
          >
            {loading ? "Saving…" : "Save"}
          </Button>
        </div>
      </header>

      {error && (
        <div className="notifications-settings-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="notifications-settings-section">
        <h3 className="notifications-settings-section-title">Categories</h3>
        <div className="notifications-settings-category-grid">
          {Object.entries(categories).map(([category, enabled]) => (
            <label
              key={category}
              className="notifications-settings-category-item"
            >
              <input
                type="checkbox"
                checked={enabled}
                onChange={(e) =>
                  setCategories((prev) => ({ ...prev, [category]: e.target.checked }))
                }
                disabled={loading}
              />
              {category.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase())}
            </label>
          ))}
        </div>
      </div>

      <div className="notifications-settings-section">
        <h3 className="notifications-settings-section-title">Severities</h3>
        <div className="notifications-settings-category-grid">
          {Object.entries(severities).map(([severity, enabled]) => (
            <label
              key={severity}
              className="notifications-settings-category-item"
            >
              <input
                type="checkbox"
                checked={enabled}
                onChange={(e) =>
                  setSeverities((prev) => ({ ...prev, [severity]: e.target.checked }))
                }
                disabled={loading}
              />
              {severity.charAt(0).toUpperCase() + severity.slice(1)}
            </label>
          ))}
        </div>
      </div>

      <div className="notifications-settings-section notifications-settings-section-toggle">
        <label className="notifications-settings-toggle-label">
          <input
            type="checkbox"
            checked={unreadOnly}
            onChange={(e) => setUnreadOnly(e.target.checked)}
            disabled={loading}
          />
          Show unread only by default
        </label>
        <span className="notifications-settings-toggle-desc">
          The Notification Center will filter to unread items on open.
        </span>
      </div>
    </div>
  );
}

export default NotificationsSettingsScreen;
