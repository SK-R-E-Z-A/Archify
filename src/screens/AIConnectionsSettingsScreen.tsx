import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

type ProviderProfile = {
  id: string;
  name: string;
  project_id: string | null;
  is_default: boolean;
  config: Record<string, unknown>;
  created_at: string;
  updated_at: string;
};

interface AIConnectionsSettingsScreenProps {
  profiles: ProviderProfile[];
}

/**
 * AI Connections settings screen — design.md §20 / §24.4.
 *
 * Manage provider profiles and AI connection settings. Every button uses
 * the token-driven Button component.
 */
export function AIConnectionsSettingsScreen({
  profiles,
}: AIConnectionsSettingsScreenProps) {
  const [error, setError] = useState<string | null>(null);

  const handleSetDefault = async (profileId: string) => {
    try {
      await invoke("update_provider_profile", {
        profileId,
        updates: { is_default: true },
      });
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to set default profile");
    }
  };

  const handleDelete = async (profileId: string) => {
    if (
      !window.confirm("Delete this provider profile? This cannot be undone.")
    )
      return;
    try {
      await invoke("delete_provider_profile", { profileId });
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to delete profile");
    }
  };

  return (
    <div className="ai-connections-settings-screen" role="main" aria-label="AI Connections Settings">
      <header className="ai-connections-settings-head">
        <div>
          <h2 className="ai-connections-settings-title">AI Connections</h2>
          <p className="ai-connections-settings-sub">
            Manage AI provider profiles and connections.
          </p>
        </div>
        <div className="ai-connections-settings-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={() => {
              // Provider creation is a missing feature (§20 AI Connections screen)
            }}
            title="Add provider profile"
          >
            Add Profile
          </Button>
        </div>
      </header>

      {error && (
        <div className="ai-connections-settings-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="ai-connections-settings-list">
        {profiles.length === 0 && (
          <div className="ai-connections-settings-empty">
            <span className="ai-connections-settings-empty-icon">▱</span>
            <p className="ai-connections-settings-empty-title">No provider profiles</p>
            <p className="ai-connections-settings-empty-desc">
              Add a provider profile to connect AI models.
            </p>
          </div>
        )}
        {profiles.map((profile) => (
          <div
            key={profile.id}
            className={`ai-connections-settings-item ${
              profile.is_default ? "default" : ""
            }`}
          >
            <div className="ai-connections-settings-item-info">
              <h3 className="ai-connections-settings-item-name">{profile.name}</h3>
              <span className="ai-connections-settings-item-project">
                {profile.project_id ? "Project-scoped" : "Global"}
              </span>
              <div className="ai-connections-settings-item-meta">
                <span className="ai-connections-settings-item-badge default">
                  {profile.is_default ? "Default" : "Secondary"}
                </span>
              </div>
            </div>
            <div className="ai-connections-settings-item-actions">
              {!profile.is_default && (
                <Button
                  role="secondary"
                  size="sm"
                  onClick={() => handleSetDefault(profile.id)}
                  title="Set as default"
                >
                  Set Default
                </Button>
              )}
              <Button
                role="danger"
                size="sm"
                onClick={() => handleDelete(profile.id)}
                title="Delete profile"
              >
                Delete
              </Button>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

export default AIConnectionsSettingsScreen;
