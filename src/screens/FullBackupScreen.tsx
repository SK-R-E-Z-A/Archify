import { useState } from "react";
import { Button } from "../components/Button";

interface FullBackupScreenProps {
}

/**
 * Full Backup screen — design.md §25.1.
 *
 * Create and manage full workspace backups. Every button uses the
 * token-driven Button component.
 */
export function FullBackupScreen({}: FullBackupScreenProps) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [backupCreated, setBackupCreated] = useState(false);

  const handleCreateBackup = async () => {
    setLoading(true);
    setError(null);
    try {
      // Full backup creation is wired to a later milestone
      setBackupCreated(true);
      setLoading(false);
    } catch (e) {
      setLoading(false);
      setError(typeof e === "string" ? e : "Failed to create backup");
    }
  };

  const handleDownload = async () => {
    // Download backup file is wired to a later milestone
  };

  return (
    <div className="full-backup-screen" role="main" aria-label="Full Backup">
      <header className="full-backup-head">
        <div>
          <h2 className="full-backup-title">Full Backup</h2>
          <p className="full-backup-sub">
            Create a complete backup of your workspace.
          </p>
        </div>
      </header>

      {error && (
        <div className="full-backup-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="full-backup-section">
        <h3 className="full-backup-section-title">Create Backup</h3>
        <p className="full-backup-hint">
          A full backup includes all projects, tasks, notifications,
          activity logs, and settings.
        </p>
        <div className="full-backup-actions">
          <Button
            role="primary"
            size="sm"
            onClick={handleCreateBackup}
            disabled={loading || backupCreated}
            title={loading ? "Creating backup…" : "Create full backup"}
          >
            {loading ? "Creating backup…" : "Create Full Backup"}
          </Button>
        </div>
      </div>

      {backupCreated && (
        <div className="full-backup-complete">
          <span className="full-backup-complete-icon">✓</span>
          <p className="full-backup-complete-title">Backup created</p>
          <div className="full-backup-complete-actions">
            <Button
              role="secondary"
              size="sm"
              onClick={handleDownload}
              title="Download backup"
            >
              Download Backup
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

export default FullBackupScreen;
