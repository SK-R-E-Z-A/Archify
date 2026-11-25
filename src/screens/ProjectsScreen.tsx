import { useCallback, useState } from "react";
import { Button } from "../components/Button";

interface Project {
  id: string;
  name: string;
  pinned: boolean;
  profile?: string | null;
  archived?: boolean;
}

interface ProjectsScreenProps {
  projects: Project[];
  onOpenProject: (id: string) => void;
  onNewTask: (id: string) => void;
  onRename: (id: string) => void;
  onPin: (id: string) => void;
  onDuplicate: (id: string) => void;
  onArchive: (id: string) => void;
  onRemove: (id: string, deleteFiles: boolean) => void;
  onAddProject: () => void;
}

/**
 * Projects screen — design.md §4.4, §4.5, §7.
 *
 * Project list with the required per-project interactions:
 *   - Click the project → open it
 *   - Click the separate chevron → expand/collapse project navigation
 *   - Three-dot context menu: Open, New Task, Rename, Pin, Duplicate,
 *     Project Settings, Archive, Remove (destructive = confirmation)
 *
 * All buttons use the token-driven Button component.
 */
export function ProjectsScreen({
  projects,
  onOpenProject,
  onNewTask,
  onRename,
  onPin,
  onDuplicate,
  onArchive,
  onRemove,
  onAddProject,
}: ProjectsScreenProps) {
  const [menuOpen, setMenuOpen] = useState<{ projectId: string } | null>(
    null,
  );
  const [confirmDelete, setConfirmDelete] = useState<{
    projectId: string;
    deleteFiles: boolean;
  } | null>(null);

  const visibleProjects = projects.filter((p) => !p.archived);

  const openMenu = useCallback(
    (id: string) => setMenuOpen({ projectId: id }),
    [],
  );
  const closeMenu = useCallback(() => setMenuOpen(null), []);

  const handleMenuAction = useCallback(
    (id: string, action: string) => {
      closeMenu();
      switch (action) {
        case "open":
          onOpenProject(id);
          break;
        case "newTask":
          onNewTask(id);
          break;
        case "rename":
          onRename(id);
          break;
        case "pin":
          onPin(id);
          break;
        case "duplicate":
          onDuplicate(id);
          break;
        case "settings":
          // Project Settings — missing feature (design.md §21 project workspace screen)
          break;
        case "archive":
          onArchive(id);
          break;
        case "remove":
          setConfirmDelete({ projectId: id, deleteFiles: false });
          break;
        default:
          break;
      }
    },
    [closeMenu, onOpenProject, onNewTask, onRename, onPin, onDuplicate, onArchive],
  );

  const isMenuOpenFor = (id: string) =>
    menuOpen?.projectId === id;

  return (
    <div className="library-screen" role="main" aria-label="Projects">
      <header className="library-head">
        <div>
          <span className="eyebrow">REPOSITORIES</span>
          <h2 className="library-title">Projects</h2>
          <p className="library-sub">
            Manage your AI development projects. Click a project to open it;
            use the chevron to expand its navigation.
          </p>
        </div>
        <div className="library-actions">
          <Button role="secondary" onClick={onAddProject} title="Add project">
            <span className="icon">＋</span> Add project
          </Button>
        </div>
      </header>

      <div className="projects-list">
        {visibleProjects.length === 0 ? (
          <div className="projects-empty">
            <div className="projects-empty-icon">▱</div>
            <h3 className="projects-empty-title">No projects yet</h3>
            <p className="projects-empty-desc">
              Open a folder to add your first project, or create one below.
            </p>
            <Button
              role="primary"
              block
              onClick={onAddProject}
              title="Add project"
            >
              Add project
            </Button>
          </div>
        ) : (
          visibleProjects.map((project) => (
            <section
              key={project.id}
              className={`projects-item ${
                project.pinned ? "pinned" : ""
              } ${project.archived ? "archived" : ""}`}
            >
              <div className="projects-item-head">
                <button
                  type="button"
                  className="projects-item-open"
                  title={`Open ${project.name}`}
                  onClick={() => onOpenProject(project.id)}
                  aria-label={`Open ${project.name}`}
                >
                  <span className="projects-item-pin">
                    {project.pinned ? "★" : "☆"}
                  </span>
                  <span className="projects-item-name">{project.name}</span>
                  <span className="projects-item-profile">
                    {project.profile ?? "Default provider"}
                  </span>
                </button>
                <button
                  type="button"
                  className={`projects-item-chevron ${
                    isMenuOpenFor(project.id) ? "open" : ""
                  }`}
                  title="Project navigation"
                  onClick={(e) => {
                    e.stopPropagation();
                    if (isMenuOpenFor(project.id)) closeMenu();
                    else openMenu(project.id);
                  }}
                  aria-label="Project navigation"
                  aria-expanded={isMenuOpenFor(project.id)}
                >
                  ▾
                </button>
                <button
                  type="button"
                  className="projects-item-menu"
                  title="More options"
                  onClick={(e) => {
                    e.stopPropagation();
                    openMenu(project.id);
                  }}
                  aria-label={`More options for ${project.name}`}
                  aria-haspopup="menu"
                >
                  ⁝
                </button>
              </div>

              {isMenuOpenFor(project.id) && (
                <div className="projects-menu">
                  <button
                    type="button"
                    className="projects-menu-item"
                    onClick={() => handleMenuAction(project.id, "open")}
                  >
                    Open
                  </button>
                  <button
                    type="button"
                    className="projects-menu-item"
                    onClick={() => handleMenuAction(project.id, "newTask")}
                  >
                    New Task
                  </button>
                  <button
                    type="button"
                    className="projects-menu-item"
                    onClick={() => handleMenuAction(project.id, "rename")}
                  >
                    Rename
                  </button>
                  <button
                    type="button"
                    className="projects-menu-item"
                    onClick={() => handleMenuAction(project.id, "pin")}
                  >
                    {project.pinned ? "Unpin" : "Pin"}
                  </button>
                  <button
                    type="button"
                    className="projects-menu-item"
                    onClick={() => handleMenuAction(project.id, "duplicate")}
                  >
                    Duplicate
                  </button>
                  <button
                    type="button"
                    className="projects-menu-item"
                    onClick={() => handleMenuAction(project.id, "settings")}
                  >
                    Project Settings
                  </button>
                  <button
                    type="button"
                    className="projects-menu-item"
                    onClick={() => handleMenuAction(project.id, "archive")}
                  >
                    Archive
                  </button>
                  <button
                    type="button"
                    className="projects-menu-item projects-menu-item-danger"
                    onClick={() => {
                      closeMenu();
                      setConfirmDelete({
                        projectId: project.id,
                        deleteFiles: false,
                      });
                    }}
                  >
                    Remove…
                  </button>
                  {confirmDelete?.projectId === project.id && (
                    <div className="projects-confirm">
                      <span className="projects-confirm-msg">
                        Remove {project.name}?{" "}
                        {confirmDelete.deleteFiles
                          ? "Files will be deleted."
                          : "Files will be kept."}
                      </span>
                      <div className="projects-confirm-actions">
                        <Button
                          role="secondary"
                          size="sm"
                          onClick={() => setConfirmDelete(null)}
                        >
                          Cancel
                        </Button>
                        <Button
                          role="secondary"
                          size="sm"
                          onClick={() => {
                            setConfirmDelete({
                              projectId: project.id,
                              deleteFiles: !confirmDelete.deleteFiles,
                            });
                          }}
                        >
                          {confirmDelete.deleteFiles ? "Keep files" : "Delete files"}
                        </Button>
                        <Button
                          role="danger"
                          size="sm"
                          onClick={() => {
                            onRemove(project.id, confirmDelete.deleteFiles);
                            setConfirmDelete(null);
                          }}
                        >
                          Remove
                        </Button>
                      </div>
                    </div>
                  )}
                </div>
              )}
            </section>
          ))
        )}
      </div>
    </div>
  );
}

export default ProjectsScreen;
