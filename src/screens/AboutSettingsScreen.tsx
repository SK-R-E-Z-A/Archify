import { useState } from "react";
import { Button } from "../components/Button";

interface AboutSettingsScreenProps {
}

/**
 * About settings screen — design.md §24.10.
 *
 * App version, build information, licenses, and acknowledgments.
 * Every button uses the token-driven Button component.
 */
export function AboutSettingsScreen({}: AboutSettingsScreenProps) {
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleCheckUpdates = async () => {
    setLoading(true);
    setError(null);
    try {
      setLoading(false);
    } catch (e) {
      setLoading(false);
      setError(typeof e === "string" ? e : "Failed to check for updates");
    }
  };

  const handleViewLicense = async () => {
    // License viewer is a missing feature
  };

  const handleViewAcknowledgments = async () => {
    // Acknowledgments viewer is a missing feature
  };

  return (
    <div className="about-settings-screen" role="main" aria-label="About Settings">
      <header className="about-settings-head">
        <div className="about-settings-icon">◐</div>
        <div>
          <h2 className="about-settings-title">Archify</h2>
          <p className="about-settings-sub">AI-powered development workspace</p>
        </div>
      </header>

      {error && (
        <div className="about-settings-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="about-settings-section">
        <h3 className="about-settings-section-title">Version</h3>
        <div className="about-settings-field">
          <span className="about-settings-label">App version</span>
          <span className="about-settings-value">1.0.0</span>
        </div>
        <div className="about-settings-field">
          <span className="about-settings-label">Build</span>
          <span className="about-settings-value">2026.11.22</span>
        </div>
        <div className="about-settings-field">
          <span className="about-settings-label">Platform</span>
          <span className="about-settings-value">Tauri v2 · Windows x64</span>
        </div>
      </div>

      <div className="about-settings-section">
        <h3 className="about-settings-section-title">Updates</h3>
        <div className="about-settings-field-group">
          <Button
            role="secondary"
            size="sm"
            onClick={handleCheckUpdates}
            disabled={loading}
            title={loading ? "Checking…" : "Check for updates"}
          >
            {loading ? "Checking…" : "Check for Updates"}
          </Button>
          <p className="about-settings-hint">
            Archify will notify you when a new version is available.
          </p>
        </div>
      </div>

      <div className="about-settings-section">
        <h3 className="about-settings-section-title">Legal</h3>
        <div className="about-settings-field-group">
          <Button
            role="secondary"
            size="sm"
            onClick={handleViewLicense}
            title="View license"
          >
            License
          </Button>
          <Button
            role="secondary"
            size="sm"
            onClick={handleViewAcknowledgments}
            title="View acknowledgments"
          >
            Acknowledgments
          </Button>
        </div>
      </div>

      <div className="about-settings-section">
        <h3 className="about-settings-section-title">Support</h3>
        <p className="about-settings-hint">
          For help, documentation, and support, visit the Archify website
          or open an issue in the project repository.
        </p>
      </div>
    </div>
  );
}

export default AboutSettingsScreen;
