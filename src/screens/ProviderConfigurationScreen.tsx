import { useCallback, useEffect, useState } from "react";
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

interface ProviderConfigurationScreenProps {
  profileId: string;
}

/**
 * Provider Configuration screen — design.md §20.
 *
 * Full configuration view for a single provider profile: cloud endpoint,
 * model, CLI settings, verification command, and prompt delivery.
 * Every button uses the token-driven Button component.
 */
export function ProviderConfigurationScreen({ profileId }: ProviderConfigurationScreenProps) {
  const [profile, setProfile] = useState<ProviderProfile | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState("");

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const profiles = await invoke<ProviderProfile[]>("list_provider_profiles", {});
      const p = profiles.find((x) => x.id === profileId);
      setProfile(p ?? null);
      if (p) setName(p.name);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load provider profile");
    } finally {
      setLoading(false);
    }
  }, [profileId]);

  useEffect(() => {
    void load();
  }, [load]);

  const handleSave = async () => {
    if (!profile) return;
    setLoading(true);
    setError(null);
    try {
      await invoke("update_provider_profile", {
        profileId: profile.id,
        updates: { name },
      });
      setEditing(false);
      void load();
    } catch (e) {
      setLoading(false);
      setError(typeof e === "string" ? e : "Failed to save profile");
    }
  };

  const handleDelete = async () => {
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

  if (error) {
    return (
      <div className="provider-configuration-screen" role="main" aria-label="Provider Configuration">
        <header className="provider-configuration-head">
          <div>
            <h2 className="provider-configuration-title">Provider Configuration</h2>
            <p className="provider-configuration-sub">Profile #{profileId}</p>
          </div>
        </header>
        <div className="provider-configuration-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      </div>
    );
  }

  if (loading || !profile) {
    return (
      <div className="provider-configuration-screen" role="main" aria-label="Provider Configuration">
        <header className="provider-configuration-head">
          <div>
            <h2 className="provider-configuration-title">Provider Configuration</h2>
            <p className="provider-configuration-sub">Profile #{profileId}</p>
          </div>
        </header>
        <div className="provider-configuration-empty">
          <span className="provider-configuration-empty-icon">⟳</span>
          <p className="provider-configuration-empty-title">Loading profile…</p>
        </div>
      </div>
    );
  }

  return (
    <div className="provider-configuration-screen" role="main" aria-label="Provider Configuration">
      <header className="provider-configuration-head">
        <div>
          {editing ? (
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              className="provider-configuration-name-input"
              autoFocus
            />
          ) : (
            <h2 className="provider-configuration-title">{profile.name}</h2>
          )}
          <span className="provider-configuration-default">
            {profile.is_default ? "Default" : "Secondary"}
          </span>
        </div>
        <div className="provider-configuration-actions">
          {!editing && (
            <Button
              role="secondary"
              size="sm"
              onClick={() => setEditing(true)}
              title="Edit profile"
            >
              Edit
            </Button>
          )}
          {editing && (
            <>
              <Button
                role="secondary"
                size="sm"
                onClick={handleSave}
                disabled={loading}
                title="Save"
              >
                {loading ? "Saving…" : "Save"}
              </Button>
              <Button
                role="secondary"
                size="sm"
                onClick={() => { setEditing(false); setName(profile.name); }}
                title="Cancel"
              >
                Cancel
              </Button>
            </>
          )}
          <Button
            role="danger"
            size="sm"
            onClick={handleDelete}
            title="Delete profile"
          >
            Delete
          </Button>
        </div>
      </header>

      <div className="provider-configuration-section">
        <h3 className="provider-configuration-section-title">Configuration</h3>
        <div className="provider-configuration-field">
          <span className="provider-configuration-field-label">Cloud Endpoint</span>
          <span className="provider-configuration-field-value">
            {String(profile.config.cloud_endpoint ?? "Not configured")}
          </span>
        </div>
        <div className="provider-configuration-field">
          <span className="provider-configuration-field-label">Cloud Model</span>
          <span className="provider-configuration-field-value">
            {String(profile.config.cloud_model ?? "Not configured")}
          </span>
        </div>
        <div className="provider-configuration-field">
          <span className="provider-configuration-field-label">CLI Executable</span>
          <span className="provider-configuration-field-value">
            {String(profile.config.cli_executable ?? "Not configured")}
          </span>
        </div>
        <div className="provider-configuration-field">
          <span className="provider-configuration-field-label">Verification Command</span>
          <span className="provider-configuration-field-value">
            {String(profile.config.verification_command ?? "Not configured")}
          </span>
        </div>
        <div className="provider-configuration-field">
          <span className="provider-configuration-field-label">Max Iterations</span>
          <span className="provider-configuration-field-value">
            {String(profile.config.max_iterations ?? 10)}
          </span>
        </div>
        <div className="provider-configuration-field">
          <span className="provider-configuration-field-label">Prompt Delivery</span>
          <span className="provider-configuration-field-value">
            {String(profile.config.prompt_delivery ?? "Not configured")}
          </span>
        </div>
      </div>
    </div>
  );
}

export default ProviderConfigurationScreen;
