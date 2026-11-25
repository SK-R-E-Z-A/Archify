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

type ProviderProfile = {
  id: string;
  name: string;
  project_id: string | null;
  is_default: boolean;
  config: Record<string, unknown>;
  created_at: string;
  updated_at: string;
};

interface ProjectSettingsScreenProps {
  project: ProjectRecord | null;
  profiles: ProviderProfile[];
  onProjectUpdated: () => void;
}

/**
 * Project Settings screen — design.md §21 project workspace settings.
 *
 * Project-scoped settings: name, provider profile, pinned state, archive,
 * and remove. Every button uses the token-driven Button component and
 * respects theme / density / font-size presets.
 */
export function ProjectSettingsScreen({
  project,
  profiles,
  onProjectUpdated,
}: ProjectSettingsScreenProps) {
  const [name, setName] = useState(project?.name ?? "");
  const [providerProfileId, setProviderProfileId] = useState(
    project?.provider_profile_id ?? "",
  );
  const [pinned, setPinned] = useState(project?.pinned ?? false);
  const [confirmRemoveName, setConfirmRemoveName] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showRemoveConfirm, setShowRemoveConfirm] = useState(false);

  const projectProfiles = profiles.filter((p) => p.project_id === project?.id);

  const handleSave = async () => {
    if (!project) return;
    setLoading(true);
    setError(null);
    try {
      await invoke("update_project", {
        project_id: project.id,
        name: name || undefined,
        provider_profile_id: providerProfileId || undefined,
        pinned,
      });
      onProjectUpdated();
      setLoading(false);
    } catch (e) {
      setLoading(false);
      setError(typeof e === "string" ? e : "Failed to save project settings");
    }
  };

  const handleArchive = async () => {
    if (!project) return;
    if (
      !window.confirm(
        `Archive "${project.name}"? It will be hidden from the project list.`,
      )
    )
      return;
    setLoading(true);
    try {
      await invoke("archive_project", { project_id: project.id });
      setLoading(false);
      onProjectUpdated();
    } catch (e) {
      setLoading(false);
      setError(typeof e === "string" ? e : "Failed to archive project");
    }
  };

  const handleRemove = async () => {
    if (!project) return;
    setShowRemoveConfirm(false);
    setLoading(true);
    try {
      await invoke("remove_project", { project_id: project.id });
      setLoading(false);
      onProjectUpdated();
    } catch (e) {
      setLoading(false);
      setError(typeof e === "string" ? e : "Failed to remove project");
    }
  };

  if (error) {
    return (
      <div className="project-settings-screen" role="main" aria-label="Project Settings">
        <header className="project-settings-head">
          <div>
            <h2 className="project-settings-title">{project?.name || "Project Settings"}</h2>
            <p className="project-settings-sub">Settings for this project.</p>
          </div>
        </header>
        <div className="project-settings-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      </div>
    );
  }

  if (!project) {
    return (
      <div className="project-settings-screen" role="main" aria-label="Project Settings">
        <header className="project-settings-head">
          <div>
            <h2 className="project-settings-title">Project Settings</h2>
            <p className="project-settings-sub">No project selected.</p>
          </div>
        </header>
        <div className="project-settings-empty">
          <span className="project-settings-empty-icon">▱</span>
          <p className="project-settings-empty-title">No project selected</p>
          <p className="project-settings-empty-desc">
            Open a project to view its settings.
          </p>
        </div>
      </div>
    );
  }

  return (
    <div className="project-settings-screen" role="main" aria-label="Project Settings">
      <header className="project-settings-head">
        <div>
          <h2 className="project-settings-title">{project.name}</h2>
          <p className="project-settings-sub">Settings for this project.</p>
        </div>
        <div className="project-settings-actions">
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

      {/* Name */}
      <div className="project-settings-section">
        <h3 className="project-settings-section-title">Name</h3>
        <input
          type="text"
          value={name}
          onChange={(e) => setName(e.target.value)}
          disabled={loading}
          className="project-settings-input"
          placeholder="Project name"
        />
      </div>

      {/* Provider profile */}
      <div className="project-settings-section">
        <h3 className="project-settings-section-title">Provider Profile</h3>
        <select
          value={providerProfileId}
          onChange={(e) => setProviderProfileId(e.target.value)}
          disabled={loading}
          className="project-settings-select"
        >
          <option value="">Default (none)</option>
          {projectProfiles.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}{p.is_default ? " (default)" : ""}
            </option>
          ))}
        </select>
        {projectProfiles.length === 0 && (
          <p className="project-settings-note">
            No provider profiles are associated with this project.
          </p>
        )}
      </div>

      {/* Pinned */}
      <div className="project-settings-section project-settings-section-toggle">
        <label className="project-settings-toggle-label">
          <input
            type="checkbox"
            checked={pinned}
            onChange={(e) => setPinned(e.target.checked)}
            disabled={loading}
          />
          Pin project
        </label>
        <span className="project-settings-toggle-desc">
          Pinned projects appear at the top of the project list.
        </span>
      </div>

      {/* Danger zone */}
      <div className="project-settings-section project-settings-section-danger">
        <h3 className="project-settings-section-title">Danger Zone</h3>
        <Button
          role="secondary"
          size="sm"
          onClick={handleArchive}
          disabled={loading}
          title="Archive project"
        >
          Archive Project
        </Button>
        {showRemoveConfirm ? (
          <div className="project-settings-remove-confirm">
            <span className="project-settings-remove-confirm-msg">
              Type "{project.name}" to confirm removal:
            </span>
            <input
              type="text"
              value={confirmRemoveName}
              onChange={(e) => setConfirmRemoveName(e.target.value)}
              className="project-settings-remove-confirm-input"
              placeholder={project.name}
              maxLength={project.name.length}
            />
            <div className="project-settings-remove-confirm-actions">
              <Button
                role="secondary"
                size="sm"
                onClick={() => setShowRemoveConfirm(false)}
              >
                Cancel
              </Button>
              <Button
                role="danger"
                size="sm"
                onClick={handleRemove}
                disabled={loading || confirmRemoveName !== project.name}
              >
                Remove Project
              </Button>
            </div>
          </div>
        ) : (
          <Button
            role="danger"
            size="sm"
            onClick={() => setShowRemoveConfirm(true)}
            title="Remove project"
          >
            Remove Project
          </Button>
        )}
      </div>
    </div>
  );
}

export default ProjectSettingsScreen;
