import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

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

type WorkspaceSettings = {
  max_concurrent_tasks: number;
  max_concurrent_sessions: number;
  scheduler_paused: boolean;
  auto_retry_interrupted: boolean;
  max_recovery_attempts: number;
};

interface WorkspaceManagerScreenProps {
  projects: ProjectRecord[];
  settings: WorkspaceSettings;
  onProjectUpdated: () => void;
  onSettingsUpdated: () => void;
}

/**
 * Workspace Manager — design.md §19.
 *
 * Central workspace administration: project lifecycle, workspace settings,
 * and maintenance actions. Every button uses the token-driven Button
 * component.
 */
export function WorkspaceManagerScreen({
  projects,
  settings,
  onProjectUpdated,
  onSettingsUpdated,
}: WorkspaceManagerScreenProps) {
  const [error, setError] = useState<string | null>(null);

  const handleAddProject = async () => {
    const root = window.prompt("Enter project folder path:");
    if (!root) return;
    try {
      await invoke("create_project", { root, name: undefined });
      onProjectUpdated();
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to add project");
    }
  };

  const handleSaveSettings = async () => {
    try {
      await invoke("update_workspace_settings", { settings });
      onSettingsUpdated();
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to save workspace settings");
    }
  };

  return (
    <div className="workspace-manager-screen" role="main" aria-label="Workspace Manager">
      <header className="workspace-manager-head">
        <div>
          <h2 className="workspace-manager-title">Workspace Manager</h2>
          <p className="workspace-manager-sub">
            Central workspace administration.
          </p>
        </div>
        <div className="workspace-manager-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={handleAddProject}
            title="Add project"
          >
            Add Project
          </Button>
        </div>
      </header>

      {error && (
        <div className="workspace-manager-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="workspace-manager-section">
        <h3 className="workspace-manager-section-title">Projects ({projects.length})</h3>
        <div className="workspace-manager-project-list">
          {projects.length === 0 && (
            <p className="workspace-manager-empty">No projects configured.</p>
          )}
          {projects.map((project) => (
            <div
              key={project.id}
              className="workspace-manager-project-item"
            >
              <span className="workspace-manager-project-name">{project.name}</span>
              <span className="workspace-manager-project-path">{project.root}</span>
            </div>
          ))}
        </div>
      </div>

      <div className="workspace-manager-section">
        <h3 className="workspace-manager-section-title">Workspace Settings</h3>
        <div className="workspace-manager-field">
          <label className="workspace-manager-label">
            Max concurrent tasks
          </label>
          <input
            type="number"
            value={settings.max_concurrent_tasks}
            onChange={(e) => {
              settings.max_concurrent_tasks = Number(e.target.value);
            }}
            min={1}
            max={16}
            className="workspace-manager-input"
          />
        </div>
        <div className="workspace-manager-field">
          <label className="workspace-manager-label">
            Max concurrent sessions
          </label>
          <input
            type="number"
            value={settings.max_concurrent_sessions}
            onChange={(e) => {
              settings.max_concurrent_sessions = Number(e.target.value);
            }}
            min={1}
            max={16}
            className="workspace-manager-input"
          />
        </div>
        <div className="workspace-manager-section-toggle">
          <label className="workspace-manager-toggle-label">
            <input
              type="checkbox"
              checked={settings.scheduler_paused}
              onChange={(e) => {
                settings.scheduler_paused = e.target.checked;
              }}
            />
            Scheduler paused
          </label>
        </div>
        <Button
          role="primary"
          size="sm"
          onClick={handleSaveSettings}
          title="Save workspace settings"
        >
          Save Settings
        </Button>
      </div>
    </div>
  );
}

export default WorkspaceManagerScreen;
