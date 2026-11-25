import { useState, useCallback, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

type ProjectRecord = {
  id: string;
  name: string;
  root: string;
  provider_profile_id: string | null;
  created_at: string;
  last_opened_at: string;
  pinned: boolean;
  archived?: boolean;
  description?: string | null;
  tags?: string[];
};

type WorkspaceSnapshot = {
  projects: ProjectRecord[];
  provider_profiles: unknown[];
  settings: unknown;
  active_project_id: string | null;
  active_task_ids: string[];
  queued_task_ids: string[];
};

type ProjectStatus = "active" | "inactive" | "missing";

function describeError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Unknown error";
}

function projectStatus(
  project: ProjectRecord,
  activeProjectId: string | null,
  missingRootIds: Set<string>,
): ProjectStatus {
  if (project.id === activeProjectId) return "active";
  if (missingRootIds.has(project.id)) return "missing";
  return "inactive";
}

function statusLabel(status: ProjectStatus): string {
  switch (status) {
    case "active":
      return "Active";
    case "missing":
      return "Missing directory";
    case "inactive":
      return "Inactive";
  }
}

export function Projects({
  onSelectProject,
}: {
  onSelectProject?: (project: ProjectRecord) => void;
}) {
  const [projects, setProjects] = useState<ProjectRecord[]>([]);
  const [activeProjectId, setActiveProjectId] = useState<string | null>(null);
  const [missingRootIds, setMissingRootIds] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [newProjectName, setNewProjectName] = useState("");
  const [newProjectRoot, setNewProjectRoot] = useState("");
  const [removingId, setRemovingId] = useState<string | null>(null);
  const [confirmRemoveId, setConfirmRemoveId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editName, setEditName] = useState("");
  const [editRoot, setEditRoot] = useState("");
  const [editDescription, setEditDescription] = useState("");
  const [editTags, setEditTags] = useState("");
  const [savingId, setSavingId] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [archivingId, setArchivingId] = useState<string | null>(null);
  const [togglingPinId, setTogglingPinId] = useState<string | null>(null);

  const loadProjects = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const snapshot: WorkspaceSnapshot = await invoke("get_workspace");
      setProjects(snapshot.projects ?? []);
      setActiveProjectId(snapshot.active_project_id ?? null);
      const missing: string[] = await invoke("detect_missing_project_roots");
      setMissingRootIds(new Set(missing ?? []));
    } catch (e) {
      setError(`Failed to load projects: ${describeError(e)}`);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadProjects();
  }, [loadProjects]);

  const visibleProjects = useCallback(() => {
    const needle = searchQuery.toLowerCase();
    return projects.filter(
      (project) =>
        needle === "" ||
        project.name.toLowerCase().includes(needle) ||
        project.root.toLowerCase().includes(needle) ||
        (project.description ?? "").toLowerCase().includes(needle) ||
        (project.tags ?? []).some((t) => t.toLowerCase().includes(needle)),
    );
  }, [projects, searchQuery]);

  const addProject = async () => {
    const name = newProjectName.trim();
    const root = newProjectRoot.trim();
    if (!name || !root || creating) return;
    setCreating(true);
    setError(null);
    try {
      await invoke<ProjectRecord>("add_project", { root, name });
      setNewProjectName("");
      setNewProjectRoot("");
      await loadProjects();
    } catch (e) {
      setError(`Failed to add project: ${describeError(e)}`);
    } finally {
      setCreating(false);
    }
  };

  const removeProject = async (id: string) => {
    const project = projects.find((p) => p.id === id);
    if (!project) return;
    setRemovingId(id);
    setError(null);
    try {
      await invoke("remove_project", { projectId: id });
      await loadProjects();
    } catch (e) {
      setError(`Failed to remove project: ${describeError(e)}`);
    } finally {
      setRemovingId(null);
    }
  };

  const updateProject = async (id: string) => {
    const project = projects.find((p) => p.id === id);
    if (!project || savingId === id) return;
    const name = editName.trim() || project.name;
    const root = editRoot.trim() || project.root;
    if (!name || !root) {
      setError("Name and folder are required.");
      return;
    }
    setSavingId(id);
    setError(null);
    try {
      const updated = await invoke<ProjectRecord>("update_project", {
        project_id: id,
        name,
        provider_profile_id: null,
        pinned: null,
        description: editDescription.trim() || null,
        tags: editTags
          .split(",")
          .map((t) => t.trim())
          .filter((t) => t.length > 0),
      });
      if (updated.root !== project.root) {
        await invoke<ProjectRecord>("update_project_root", {
          project_id: id,
          new_root: updated.root,
        });
      }
      await loadProjects();
    } catch (e) {
      setError(`Failed to save project: ${describeError(e)}`);
    } finally {
      setSavingId(null);
      setEditingId(null);
    }
  };

  const toggleArchive = async (project: ProjectRecord) => {
    if (archivingId === project.id) return;
    setArchivingId(project.id);
    setError(null);
    try {
      await invoke("archive_project", {
        project_id: project.id,
        archived: !Boolean(project.archived),
      });
      await loadProjects();
    } catch (e) {
      setError(`Failed to update project: ${describeError(e)}`);
    } finally {
      setArchivingId(null);
    }
  };

  const togglePin = async (project: ProjectRecord) => {
    if (togglingPinId === project.id) return;
    setTogglingPinId(project.id);
    setError(null);
    try {
      await invoke<ProjectRecord>("update_project", {
        project_id: project.id,
        name: null,
        provider_profile_id: null,
        pinned: !project.pinned,
        description: null,
        tags: null,
      });
      await loadProjects();
    } catch (e) {
      setError(`Failed to update project: ${describeError(e)}`);
    } finally {
      setTogglingPinId(null);
    }
  };

  const startEdit = (project: ProjectRecord) => {
    setEditingId(project.id);
    setEditName(project.name);
    setEditRoot(project.root);
    setEditDescription(project.description ?? "");
    setEditTags((project.tags ?? []).join(", "));
    setError(null);
  };

  const cancelEdit = () => {
    setEditingId(null);
    setEditName("");
    setEditRoot("");
    setEditDescription("");
    setEditTags("");
    setError(null);
  };

  const selectProject = async (project: ProjectRecord) => {
    setError(null);
    if (onSelectProject) {
      onSelectProject(project);
      return;
    }
    try {
      await invoke("select_project", { projectId: project.id });
      await loadProjects();
    } catch (e) {
      setError(`Failed to switch project: ${describeError(e)}`);
    }
  };

  if (loading) {
    return (
      <section className="screen projects-screen">
        <h2>Projects</h2>
        <p className="muted-note">Loading projects…</p>
      </section>
    );
  }

  if (error) {
    return (
      <section className="screen projects-screen">
        <h2>Projects</h2>
        <div className="config-error">{error}</div>
        <button
          className="secondary-button"
          onClick={() => {
            setError(null);
            void loadProjects();
          }}
        >
          Retry
        </button>
      </section>
    );
  }

  const list = visibleProjects();

  return (
    <section className="screen projects-screen">
      <h2>Projects</h2>

      <div className="screen-toolbar">
        <label>
          Search
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Filter by name or path"
          />
        </label>
        <button
          className="secondary-button"
          onClick={() => {
            setError(null);
            void loadProjects();
          }}
          disabled={loading}
        >
          Refresh validation
        </button>
      </div>

      {error && <div className="config-error">{error}</div>}

      {projects.length === 0 && (
        <div className="screen-empty">
          <span className="screen-empty-icon">📂</span>
          <h3>No projects yet</h3>
          <p>
            Register a project folder to get started with Archify. Opening a
            folder from the Explorer automatically registers it.
          </p>
          <div className="screen-empty-actions">
            <button
              className="primary-button"
              onClick={() => setNewProjectRoot("")}
            >
              Add your first project
            </button>
          </div>
        </div>
      )}

      {projects.length > 0 && (
        <>
          <div className="screen-list">
            {list.map((project) => {
              const status = projectStatus(
                project,
                activeProjectId,
                missingRootIds,
              );
              const isEditing = editingId === project.id;
              const isBusy =
                removingId === project.id ||
                savingId === project.id ||
                archivingId === project.id;
              return (
                <div
                  key={project.id}
                  className={`project-item project-item-${status} ${
                    isBusy ? "busy" : ""
                  }`}
                >
                  <div className="project-item-main">
                    <div className="project-name-header">
                      <span className="project-name" title={project.root}>
                        {project.name}
                      </span>
                      <span className={`status-badge status-${status}`}>
                        {statusLabel(status)}
                      </span>
                      {project.pinned && (
                        <span className="status-badge status-pinned">Pinned</span>
                      )}
                    </div>
                    <span className="project-path">{project.root}</span>
                    {project.description && (
                      <span className="project-description" title={project.description}>
                        {project.description}
                      </span>
                    )}
                    {project.tags && project.tags.length > 0 && (
                      <div className="project-tags">
                        {project.tags.map((tag, idx) => (
                          <span key={idx} className="tag-badge">
                            {tag}
                          </span>
                        ))}
                      </div>
                    )}
                    {status === "missing" && (
                      <span className="project-hint">
                        The folder on disk no longer exists. Re-point the root
                        or remove this registration.
                      </span>
                    )}
                  </div>
                  <div className="project-item-actions">
                    <button
                      className="secondary-button tiny"
                      onClick={() => selectProject(project)}
                      disabled={isBusy}
                      title="Select project"
                    >
                      Select
                    </button>
                    {isEditing ? (
                      <>
                        <input
                          type="text"
                          className="config-input"
                          value={editName}
                          onChange={(e) => setEditName(e.target.value)}
                          autoFocus
                        />
                        <input
                          type="text"
                          className="config-input"
                          value={editRoot}
                          onChange={(e) => setEditRoot(e.target.value)}
                        />
                        <input
                          type="text"
                          className="config-input"
                          placeholder="Description (optional)"
                          value={editDescription}
                          onChange={(e) => setEditDescription(e.target.value)}
                        />
                        <input
                          type="text"
                          className="config-input"
                          placeholder="Tags (comma-separated)"
                          value={editTags}
                          onChange={(e) => setEditTags(e.target.value)}
                        />
                        <button
                          className="secondary-button tiny"
                          onClick={() => updateProject(project.id)}
                          disabled={savingId === project.id}
                        >
                          {savingId === project.id ? "Saving…" : "Save"}
                        </button>
                        <button
                          className="secondary-button tiny"
                          onClick={cancelEdit}
                        >
                          Cancel
                        </button>
                      </>
                    ) : (
                      <>
                        <button
                          className="secondary-button tiny"
                          onClick={() => startEdit(project)}
                          disabled={isBusy || editingId !== null}
                          title="Edit name and folder"
                        >
                          Edit
                        </button>
                        <button
                          className="secondary-button tiny"
                          onClick={() => togglePin(project)}
                          disabled={isBusy || savingId === project.id}
                          title={project.pinned ? "Unpin project" : "Pin project"}
                        >
                          {project.pinned ? "★" : "☆"}
                        </button>
                        <button
                          className="secondary-button tiny"
                          onClick={() => toggleArchive(project)}
                          disabled={isBusy || removingId === project.id}
                          title={
                            project.archived
                              ? "Unarchive project"
                              : "Archive project"
                          }
                        >
                          {project.archived ? "Unarchive" : "Archive"}
                        </button>
                        <button
                          className="secondary-button tiny danger"
                          onClick={() => setConfirmRemoveId(project.id)}
                          disabled={isBusy || editingId !== null}
                          title="Remove registration"
                        >
                          Remove
                        </button>
                      </>
                    )}
                  </div>
                </div>
              );
            })}
          </div>

          <div className="screen-form">
            <label>Project name</label>
            <input
              type="text"
              value={newProjectName}
              onChange={(e) => setNewProjectName(e.target.value)}
              placeholder="My project"
              disabled={creating}
            />
            <label>Project folder</label>
            <input
              type="text"
              value={newProjectRoot}
              onChange={(e) => setNewProjectRoot(e.target.value)}
              placeholder="/path/to/project"
              disabled={creating}
            />
            <button
              type="button"
              className="primary-button"
              onClick={addProject}
              disabled={creating || !newProjectName.trim() || !newProjectRoot.trim()}
            >
              {creating ? "Adding…" : "Add project"}
            </button>
          </div>
        </>
      )}

      {confirmRemoveId && (
        <div className="config-modal-backdrop" onClick={() => setConfirmRemoveId(null)}>
          <div
            className="config-modal removal-confirm-modal"
            onClick={(e) => e.stopPropagation()}
          >
            <div className="config-modal-header">
              <h2>Remove project</h2>
              <button
                className="icon-button small"
                onClick={() => setConfirmRemoveId(null)}
                title="Cancel"
              >
                ×
              </button>
            </div>
            <div className="config-modal-body">
              <div className="removal-identify">
                <span className="removal-name">
                  {projects.find((p) => p.id === confirmRemoveId)?.name ??
                    confirmRemoveId}
                </span>
                <span className="removal-path">
                  {projects.find((p) => p.id === confirmRemoveId)?.root ?? ""}
                </span>
              </div>
              <p className="removal-explanation">
                This removes the project&rsquo;s registration from Archify.
                <strong>It does not delete any files or folders on disk.</strong>
                Your project code, git history, and task history are untouched.
              </p>
              <div className="removal-does-not">
                <span>After removal:</span>
                <ul>
                  <li>The project no longer appears in the project list.</li>
                  <li>It cannot receive new tasks until re-registered.</li>
                  <li>Existing task history remains available in History.</li>
                </ul>
              </div>
            </div>
            <div className="config-modal-footer">
              <button
                className="secondary-button"
                onClick={() => setConfirmRemoveId(null)}
                disabled={removingId === confirmRemoveId}
              >
                Cancel
              </button>
              <button
                className="danger-button"
                onClick={() => {
                  setConfirmRemoveId(null);
                  void removeProject(confirmRemoveId);
                }}
                disabled={removingId === confirmRemoveId || creating}
              >
                {removingId === confirmRemoveId ? "Removing…" : "Remove registration"}
              </button>
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
