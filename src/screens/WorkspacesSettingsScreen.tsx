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

interface WorkspacesSettingsScreenProps {
  projects: ProjectRecord[];
  activeProjectId: string | null;
  switchProject: (project: ProjectRecord) => void;
}

/**
 * Workspaces settings screen — design.md §24.3.
 *
 * Manage the list of known projects: switch, pin, archive, and remove.
 * Every button uses the token-driven Button component.
 */
export function WorkspacesSettingsScreen({
  projects,
  activeProjectId,
  switchProject,
}: WorkspacesSettingsScreenProps) {
  const [error, setError] = useState<string | null>(null);

  const handleArchive = async (projectId: string) => {
    const project = projects.find((p) => p.id === projectId);
    if (!project) return;
    if (
      !window.confirm(
        `Archive "${project.name}"? It will be hidden from the project list.`,
      )
    )
      return;
    try {
      await invoke("archive_project", { project_id: projectId, archived: true });
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to archive project");
    }
  };

  const handleRemove = async (projectId: string) => {
    const project = projects.find((p) => p.id === projectId);
    if (!project) return;
    if (
      !window.confirm(
        `Remove "${project.name}" from the workspace? This cannot be undone.`,
      )
    )
      return;
    try {
      await invoke("remove_project", { project_id: projectId });
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to remove project");
    }
  };

  return (
    <div className="workspaces-settings-screen" role="main" aria-label="Workspaces Settings">
      <header className="workspaces-settings-head">
        <div>
          <h2 className="workspaces-settings-title">Workspaces</h2>
          <p className="workspaces-settings-sub">
            Manage your known projects and workspaces.
          </p>
        </div>
      </header>

      {error && (
        <div className="workspaces-settings-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="workspaces-settings-list">
        {projects.length === 0 && (
          <div className="workspaces-settings-empty">
            <span className="workspaces-settings-empty-icon">▱</span>
            <p className="workspaces-settings-empty-title">No projects</p>
            <p className="workspaces-settings-empty-desc">
              Open a folder to add your first workspace.
            </p>
          </div>
        )}
        {projects.map((project) => (
          <div
            key={project.id}
            className={`workspaces-settings-item ${
              project.id === activeProjectId ? "active" : ""
            } ${project.archived ? "archived" : ""}`}
          >
            <div className="workspaces-settings-item-info">
              <h3 className="workspaces-settings-item-name">{project.name}</h3>
              <span className="workspaces-settings-item-path">{project.root}</span>
              <div className="workspaces-settings-item-meta">
                <span className="workspaces-settings-item-badge">
                  {project.id === activeProjectId ? "Active" : "Inactive"}
                </span>
                {project.pinned && (
                  <span className="workspaces-settings-item-badge pinned">
                    Pinned
                  </span>
                )}
                {project.archived && (
                  <span className="workspaces-settings-item-badge archived">
                    Archived
                  </span>
                )}
              </div>
            </div>
            <div className="workspaces-settings-item-actions">
              <Button
                role={
                  project.id === activeProjectId ? "secondary" : "primary"
                }
                size="sm"
                onClick={() => switchProject(project)}
                title={
                  project.id === activeProjectId
                    ? "Already active"
                    : `Switch to ${project.name}`
                }
                disabled={project.id === activeProjectId}
              >
                {project.id === activeProjectId ? "Active" : "Switch"}
              </Button>
              <Button
                role="secondary"
                size="sm"
                onClick={() => handleArchive(project.id)}
                title={
                  project.archived
                    ? "Restore project"
                    : `Archive ${project.name}`
                }
              >
                {project.archived ? "Restore" : "Archive"}
              </Button>
              <Button
                role="danger"
                size="sm"
                onClick={() => handleRemove(project.id)}
                title={`Remove ${project.name}`}
              >
                Remove
              </Button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

export default WorkspacesSettingsScreen;
