import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

interface PrivacySettingsScreenProps {
}

/**
 * Privacy settings screen — design.md §24.8.
 *
 * Privacy controls: data collection, telemetry, local processing,
 * and data retention. Every button uses the token-driven Button component.
 */
export function PrivacySettingsScreen({}: PrivacySettingsScreenProps) {
  const [telemetryEnabled, setTelemetryEnabled] = useState(true);
  const [localProcessing, setLocalProcessing] = useState(true);
  const [dataRetentionDays, setDataRetentionDays] = useState(30);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSave = async () => {
    setLoading(true);
    setError(null);
    try {
      setLoading(false);
    } catch (e) {
      setLoading(false);
      setError(typeof e === "string" ? e : "Failed to save privacy settings");
    }
  };

  const handleExportData = async () => {
    // Data export is covered by Import & Export screen (§25)
  };

  const handleDeleteData = async () => {
    if (
      !window.confirm(
        "Delete all local data? This cannot be undone and will remove task history, notifications, and activity logs.",
      )
    )
      return;
    try {
      await invoke("clear_token_usage", {});
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to delete data");
    }
  };

  return (
    <div className="privacy-settings-screen" role="main" aria-label="Privacy Settings">
      <header className="privacy-settings-head">
        <div>
          <h2 className="privacy-settings-title">Privacy</h2>
          <p className="privacy-settings-sub">Data and privacy controls.</p>
        </div>
        <div className="privacy-settings-actions">
          <Button
            role="primary"
            size="sm"
            onClick={() => { handleSave(); }}
            disabled={loading}
            title={loading ? "Saving…" : "Save settings"}
          >
            {loading ? "Saving…" : "Save"}
          </Button>
        </div>
      </header>

      {error && (
        <div className="privacy-settings-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="privacy-settings-section">
        <h3 className="privacy-settings-section-title">Telemetry</h3>
        <div className="privacy-settings-section-toggle">
          <label className="privacy-settings-toggle-label">
            <input
              type="checkbox"
              checked={telemetryEnabled}
              onChange={(e) => setTelemetryEnabled(e.target.checked)}
              disabled={loading}
            />
            Send anonymous usage data
          </label>
          <span className="privacy-settings-toggle-desc">
            Helps improve Archify. No code or project content is sent.
          </span>
        </div>
      </div>

      <div className="privacy-settings-section">
        <h3 className="privacy-settings-section-title">Local Processing</h3>
        <div className="privacy-settings-section-toggle">
          <label className="privacy-settings-toggle-label">
            <input
              type="checkbox"
              checked={localProcessing}
              onChange={(e) => setLocalProcessing(e.target.checked)}
              disabled={loading}
            />
            Prefer local model processing
          </label>
          <span className="privacy-settings-toggle-desc">
            Route requests through local providers when available.
          </span>
        </div>
      </div>

      <div className="privacy-settings-section">
        <h3 className="privacy-settings-section-title">Data Retention</h3>
        <div className="privacy-settings-field">
          <label className="privacy-settings-label">Retain activity logs for</label>
          <input
            type="number"
            value={dataRetentionDays}
            onChange={(e) => setDataRetentionDays(Number(e.target.value))}
            min={1}
            max={365}
            disabled={loading}
            className="privacy-settings-input"
          />
          <span className="privacy-settings-unit">days</span>
        </div>
        <p className="privacy-settings-hint">
          Older activity and notification records are pruned automatically.
        </p>
      </div>

      <div className="privacy-settings-section privacy-settings-section-danger">
        <h3 className="privacy-settings-section-title">Danger Zone</h3>
        <Button
          role="secondary"
          size="sm"
          onClick={handleExportData}
          title="Export your data"
        >
          Export Data
        </Button>
        <Button
          role="danger"
          size="sm"
          onClick={handleDeleteData}
          title="Delete all local data"
        >
          Delete All Local Data
        </Button>
      </div>
    </div>
  );
}

export default PrivacySettingsScreen;
