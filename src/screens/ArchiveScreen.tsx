import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

type ProjectRecord = {
  id: string;
  name: string;
  root: string;
  archived?: boolean;
};

type SessionRecord = {
  id: string;
  title: string;
  project_id: string | null;
  updated_at: string;
};

interface ArchiveScreenProps {
  projects: ProjectRecord[];
  sessions: SessionRecord[];
  onProjectRestored: (project: ProjectRecord) => void;
}

/**
 * Archive screen — design.md §7.4.
 *
 * View archived projects and sessions. Supports restoring archived items.
 * Every button uses the token-driven Button component.
 */
export function ArchiveScreen({
  projects,
  sessions,
  onProjectRestored,
}: ArchiveScreenProps) {
  const [error, setError] = useState<string | null>(null);

  const archivedProjects = projects.filter((p) => p.archived);

  const handleRestoreProject = async (projectId: string) => {
    const project = projects.find((p) => p.id === projectId);
    if (!project) return;
    try {
      await invoke("archive_project", { project_id: projectId, archived: false });
      onProjectRestored(project);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to restore project");
    }
  };

  return (
    <div className="archive-screen" role="main" aria-label="Archive">
      <header className="archive-head">
        <div>
          <h2 className="archive-title">Archive</h2>
          <p className="archive-sub">Archived projects and sessions.</p>
        </div>
        <div className="archive-actions">
          <span className="archive-count">
            {archivedProjects.length} project{archivedProjects.length === 1 ? "" : "s"} · {sessions.length} session{sessions.length === 1 ? "" : "s"}
          </span>
        </div>
      </header>

      {error && (
        <div className="archive-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="archive-section">
        <h3 className="archive-section-title">Archived Projects ({archivedProjects.length})</h3>
        <div className="archive-list">
          {archivedProjects.length === 0 && (
            <p className="archive-empty">No archived projects.</p>
          )}
          {archivedProjects.map((project) => (
            <div
              key={project.id}
              className="archive-item"
            >
              <span className="archive-item-name">{project.name}</span>
              <span className="archive-item-path">{project.root}</span>
              <Button
                role="secondary"
                size="sm"
                onClick={() => handleRestoreProject(project.id)}
                title="Restore project"
              >
                Restore
              </Button>
            </div>
          ))}
        </div>
      </div>

      <div className="archive-section">
        <h3 className="archive-section-title">Archived Sessions ({sessions.length})</h3>
        <div className="archive-list">
          {sessions.length === 0 && (
            <p className="archive-empty">No archived sessions.</p>
          )}
          {sessions.map((session) => (
            <div key={session.id} className="archive-item">
              <span className="archive-item-name">{session.title}</span>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

export default ArchiveScreen;
