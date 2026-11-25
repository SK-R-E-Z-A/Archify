import { useState } from "react";
import { Button } from "../components/Button";

type ProjectRecord = {
  id: string;
  name: string;
};

interface SelectiveExportScreenProps {
  projects: ProjectRecord[];
}

/**
 * Selective Export screen — design.md §25.2.
 *
 * Choose specific projects, tasks, and notifications to export.
 * Every button uses the token-driven Button component.
 */
export function SelectiveExportScreen({ projects }: SelectiveExportScreenProps) {
  const [selectedProjectIds, setSelectedProjectIds] = useState<Set<string>>(
    new Set(),
  );
  const [includeTasks, setIncludeTasks] = useState(true);
  const [includeNotifications, setIncludeNotifications] = useState(true);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleToggleProject = (id: string) => {
    setSelectedProjectIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const handleExport = async () => {
    if (selectedProjectIds.size === 0) {
      setError("Select at least one project to export.");
      return;
    }
    setLoading(true);
    setError(null);
    try {
      // Selective export is wired to a later milestone
      setLoading(false);
    } catch (e) {
      setLoading(false);
      setError(typeof e === "string" ? e : "Failed to export");
    }
  };

  return (
    <div className="selective-export-screen" role="main" aria-label="Selective Export">
      <header className="selective-export-head">
        <div>
          <h2 className="selective-export-title">Selective Export</h2>
          <p className="selective-export-sub">
            Choose what to export from your workspace.
          </p>
        </div>
      </header>

      {error && (
        <div className="selective-export-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="selective-export-section">
        <h3 className="selective-export-section-title">Projects</h3>
        <div className="selective-export-project-list">
          {projects.length === 0 && (
            <p className="selective-export-empty">No projects available.</p>
          )}
          {projects.map((project) => (
            <div
              key={project.id}
              className="selective-export-project-item"
            >
              <input
                type="checkbox"
                checked={selectedProjectIds.has(project.id)}
                onChange={() => handleToggleProject(project.id)}
              />
              <label className="selective-export-project-label">
                {project.name}
              </label>
            </div>
          ))}
        </div>
      </div>

      <div className="selective-export-section">
        <h3 className="selective-export-section-title">Content Options</h3>
        <div className="selective-export-option">
          <label className="selective-export-toggle-label">
            <input
              type="checkbox"
              checked={includeTasks}
              onChange={(e) => setIncludeTasks(e.target.checked)}
            />
            Include task history
          </label>
        </div>
        <div className="selective-export-option">
          <label className="selective-export-toggle-label">
            <input
              type="checkbox"
              checked={includeNotifications}
              onChange={(e) => setIncludeNotifications(e.target.checked)}
            />
            Include notifications
          </label>
        </div>
      </div>

      <div className="selective-export-actions">
        <Button
          role="primary"
          size="sm"
          onClick={handleExport}
          disabled={loading || selectedProjectIds.size === 0}
          title={loading ? "Exporting…" : "Export selected"}
        >
          {loading ? "Exporting…" : "Export Selected"}
        </Button>
      </div>
    </div>
  );
}

export default SelectiveExportScreen;
