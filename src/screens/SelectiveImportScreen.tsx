import { useState } from "react";
import { Button } from "../components/Button";

interface SelectiveImportScreenProps {
}

/**
 * Selective Import screen — design.md §25.3.
 *
 * Import selected projects, tasks, and notifications from a backup file.
 * Every button uses the token-driven Button component.
 */
export function SelectiveImportScreen({}: SelectiveImportScreenProps) {
  const [includeProjects, setIncludeProjects] = useState(true);
  const [includeTasks, setIncludeTasks] = useState(true);
  const [includeNotifications, setIncludeNotifications] = useState(true);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleImport = async () => {
    setLoading(true);
    setError(null);
    try {
      // Selective import is wired to a later milestone
      setLoading(false);
    } catch (e) {
      setLoading(false);
      setError(typeof e === "string" ? e : "Failed to import");
    }
  };

  return (
    <div className="selective-import-screen" role="main" aria-label="Selective Import">
      <header className="selective-import-head">
        <div>
          <h2 className="selective-import-title">Selective Import</h2>
          <p className="selective-import-sub">
            Import from a backup file.
          </p>
        </div>
      </header>

      {error && (
        <div className="selective-import-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="selective-import-section">
        <h3 className="selective-import-section-title">Import Options</h3>
        <div className="selective-import-option">
          <label className="selective-import-toggle-label">
            <input
              type="checkbox"
              checked={includeProjects}
              onChange={(e) => setIncludeProjects(e.target.checked)}
            />
            Include projects
          </label>
        </div>
        <div className="selective-import-option">
          <label className="selective-import-toggle-label">
            <input
              type="checkbox"
              checked={includeTasks}
              onChange={(e) => setIncludeTasks(e.target.checked)}
            />
            Include task history
          </label>
        </div>
        <div className="selective-import-option">
          <label className="selective-import-toggle-label">
            <input
              type="checkbox"
              checked={includeNotifications}
              onChange={(e) => setIncludeNotifications(e.target.checked)}
            />
            Include notifications
          </label>
        </div>
      </div>

      <div className="selective-import-file">
        <input
          type="file"
          accept=".archify-backup,.json,.zip"
          className="selective-import-file-input"
        />
      </div>

      <div className="selective-import-actions">
        <Button
          role="primary"
          size="sm"
          onClick={handleImport}
          disabled={loading}
          title={loading ? "Importing…" : "Import backup"}
        >
          {loading ? "Importing…" : "Import Backup"}
        </Button>
      </div>
    </div>
  );
}

export default SelectiveImportScreen;
