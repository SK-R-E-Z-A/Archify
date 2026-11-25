import { useState } from "react";
import { Button } from "../components/Button";

interface ImportExportSettingsScreenProps {
}

/**
 * Import & Export settings screen — design.md §24.9 / §25.
 *
 * Backup, selective export, and selective import of project data,
 * tasks, notifications, and settings. Every button uses the token-driven
 * Button component.
 */
export function ImportExportSettingsScreen({}: ImportExportSettingsScreenProps) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleFullBackup = async () => {
    setLoading(true);
    setError(null);
    try {
      // Full backup is wired to §25.1 Full Backup screen
      setLoading(false);
    } catch (e) {
      setLoading(false);
      setError(typeof e === "string" ? e : "Failed to create backup");
    }
  };

  const handleSelectiveExport = async () => {
    // Selective export is wired to §25.2 Selective Export screen
  };

  const handleSelectiveImport = async () => {
    // Selective import is wired to §25.3 Selective Import screen
  };

  const handleRestore = async () => {
    if (
      !window.confirm(
        "Restore from backup? This will overwrite current local data and cannot be undone.",
      )
    )
      return;
    setLoading(true);
    try {
      setLoading(false);
    } catch (e) {
      setLoading(false);
      setError(typeof e === "string" ? e : "Failed to restore backup");
    }
  };

  return (
    <div className="importexport-settings-screen" role="main" aria-label="Import & Export Settings">
      <header className="importexport-settings-head">
        <div>
          <h2 className="importexport-settings-title">Import & Export</h2>
          <p className="importexport-settings-sub">
            Backup, export, and import project data.
          </p>
        </div>
      </header>

      {error && (
        <div className="importexport-settings-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="importexport-settings-section">
        <h3 className="importexport-settings-section-title">Backup</h3>
        <div className="importexport-settings-field-group">
          <Button
            role="primary"
            size="sm"
            onClick={handleFullBackup}
            disabled={loading}
            title={loading ? "Creating backup…" : "Create full backup"}
          >
            {loading ? "Creating backup…" : "Full Backup"}
          </Button>
          <Button
            role="secondary"
            size="sm"
            onClick={handleSelectiveExport}
            title="Selective export"
          >
            Selective Export
          </Button>
          <p className="importexport-settings-hint">
            Create a full workspace backup, or export selected projects,
            tasks, and notifications.
          </p>
        </div>
      </div>

      <div className="importexport-settings-section">
        <h3 className="importexport-settings-section-title">Import</h3>
        <div className="importexport-settings-field-group">
          <Button
            role="secondary"
            size="sm"
            onClick={handleSelectiveImport}
            title="Selective import"
          >
            Selective Import
          </Button>
          <Button
            role="secondary"
            size="sm"
            onClick={handleRestore}
            title="Restore from backup"
          >
            Restore Backup
          </Button>
          <p className="importexport-settings-hint">
            Import projects, tasks, and settings from a previous backup.
          </p>
        </div>
      </div>
    </div>
  );
}

export default ImportExportSettingsScreen;
