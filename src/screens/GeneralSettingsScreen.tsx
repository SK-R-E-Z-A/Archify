import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

type WorkspaceSettings = {
  max_concurrent_tasks: number;
  max_concurrent_sessions: number;
  scheduler_paused: boolean;
  auto_retry_interrupted: boolean;
  max_recovery_attempts: number;
};

interface GeneralSettingsScreenProps {
  settings: WorkspaceSettings;
  onSettingsUpdated: () => void;
}

/**
 * General settings screen — design.md §24.1.
 *
 * Workspace-wide settings: concurrency, scheduler, auto-retry, recovery
 * attempts. Every button uses the token-driven Button component.
 */
export function GeneralSettingsScreen({
  settings,
  onSettingsUpdated,
}: GeneralSettingsScreenProps) {
  const [maxConcurrentTasks, setMaxConcurrentTasks] = useState(
    settings.max_concurrent_tasks,
  );
  const [maxConcurrentSessions, setMaxConcurrentSessions] = useState(
    settings.max_concurrent_sessions,
  );
  const [schedulerPaused, setSchedulerPaused] = useState(
    settings.scheduler_paused,
  );
  const [autoRetryInterrupted, setAutoRetryInterrupted] = useState(
    settings.auto_retry_interrupted,
  );
  const [maxRecoveryAttempts, setMaxRecoveryAttempts] = useState(
    settings.max_recovery_attempts,
  );
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSave = async () => {
    setLoading(true);
    setError(null);
    try {
      await invoke("update_workspace_settings", {
        settings: {
          max_concurrent_tasks: maxConcurrentTasks,
          max_concurrent_sessions: maxConcurrentSessions,
          scheduler_paused: schedulerPaused,
          auto_retry_interrupted: autoRetryInterrupted,
          max_recovery_attempts: maxRecoveryAttempts,
        },
      });
      onSettingsUpdated();
      setLoading(false);
    } catch (e) {
      setLoading(false);
      setError(typeof e === "string" ? e : "Failed to save settings");
    }
  };

  if (error) {
    return (
      <div className="general-settings-screen" role="main" aria-label="General Settings">
        <header className="general-settings-head">
          <div>
            <h2 className="general-settings-title">General Settings</h2>
            <p className="general-settings-sub">Workspace-wide preferences.</p>
          </div>
        </header>
        <div className="general-settings-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="general-settings-screen" role="main" aria-label="General Settings">
      <header className="general-settings-head">
        <div>
          <h2 className="general-settings-title">General Settings</h2>
          <p className="general-settings-sub">Workspace-wide preferences.</p>
        </div>
        <div className="general-settings-actions">
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

      <div className="general-settings-section">
        <h3 className="general-settings-section-title">Task Concurrency</h3>
        <div className="general-settings-field">
          <label className="general-settings-label">Max concurrent tasks</label>
          <input
            type="number"
            value={maxConcurrentTasks}
            onChange={(e) => setMaxConcurrentTasks(Number(e.target.value))}
            min={1}
            max={16}
            disabled={loading}
            className="general-settings-input"
          />
        </div>
        <p className="general-settings-hint">
          How many tasks can run simultaneously across all projects.
        </p>
      </div>

      <div className="general-settings-section">
        <h3 className="general-settings-section-title">Session Concurrency</h3>
        <div className="general-settings-field">
          <label className="general-settings-label">Max concurrent sessions</label>
          <input
            type="number"
            value={maxConcurrentSessions}
            onChange={(e) => setMaxConcurrentSessions(Number(e.target.value))}
            min={1}
            max={16}
            disabled={loading}
            className="general-settings-input"
          />
        </div>
        <p className="general-settings-hint">
          How many AI/CLI sessions can run at the same time.
        </p>
      </div>

      <div className="general-settings-section">
        <h3 className="general-settings-section-title">Scheduler</h3>
        <div className="general-settings-section-toggle">
          <label className="general-settings-toggle-label">
            <input
              type="checkbox"
              checked={schedulerPaused}
              onChange={(e) => setSchedulerPaused(e.target.checked)}
              disabled={loading}
            />
            Pause task scheduler
          </label>
          <span className="general-settings-toggle-desc">
            When paused, no new tasks will start executing.
          </span>
        </div>
      </div>

      <div className="general-settings-section">
        <h3 className="general-settings-section-title">Recovery</h3>
        <div className="general-settings-field">
          <label className="general-settings-label">Auto-retry interrupted tasks</label>
          <select
            value={autoRetryInterrupted ? "on" : "off"}
            onChange={(e) => setAutoRetryInterrupted(e.target.value === "on")}
            disabled={loading}
            className="general-settings-select"
          >
            <option value="on">Enabled</option>
            <option value="off">Disabled</option>
          </select>
        </div>
        <div className="general-settings-field">
          <label className="general-settings-label">Max recovery attempts</label>
          <input
            type="number"
            value={maxRecoveryAttempts}
            onChange={(e) => setMaxRecoveryAttempts(Number(e.target.value))}
            min={0}
            max={10}
            disabled={loading}
            className="general-settings-input"
          />
        </div>
      </div>
    </div>
  );
}

export default GeneralSettingsScreen;
