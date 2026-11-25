import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

type ProjectRecord = {
  id: string;
  name: string;
  root: string;
  provider_profile_id: string | null;
  created_at: string;
  last_opened_at: string;
  pinned: boolean;
  archived?: boolean;
};

type ProviderProfile = {
  id: string;
  name: string;
  project_id: string | null;
  is_default: boolean;
  config: {
    planner_type: string;
    executor_type: string;
    test_planner_scenario: string | null;
    test_executor_scenario: string | null;
    cloud_endpoint: string | null;
    cloud_model: string | null;
    cli_executable: string | null;
    cli_args: string[] | null;
    cli_preset: string | null;
    prompt_delivery: string;
    verification_command: string | null;
    max_iterations: number;
    ignored_dirs: string[];
  };
  created_at: string;
  updated_at: string;
};

interface ProjectAgentConsoleScreenProps {
  project: ProjectRecord | null;
  profiles: ProviderProfile[];
}

/**
 * Project Agent Console screen — design.md §11.3.
 *
 * Project-scoped agent console showing provider profiles associated with
 * this project, their configuration, and enable/default actions. Every
 * button uses the token-driven Button component and respects theme /
 * density / font-size presets.
 */
export function ProjectAgentConsoleScreen({
  project,
  profiles,
}: ProjectAgentConsoleScreenProps) {
const [error, setError] = useState<string | null>(null);

  const projectProfiles = profiles.filter((p) => p.project_id === project?.id);

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
    if (!window.confirm("Delete this provider profile? This cannot be undone.")) return;
    try {
      await invoke("delete_provider_profile", { profileId });
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to delete profile");
    }
  };

  if (error) {
    return (
      <div className="project-agent-console-screen" role="main" aria-label="Project Agent Console">
        <header className="project-agent-console-head">
          <div>
            <h2 className="project-agent-console-title">{project?.name || "Project Agent Console"}</h2>
            <p className="project-agent-console-sub">Agent profiles for this project.</p>
          </div>
        </header>
        <div className="project-agent-console-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="project-agent-console-screen" role="main" aria-label="Project Agent Console">
      <header className="project-agent-console-head">
        <div>
          <h2 className="project-agent-console-title">{project?.name || "Project Agent Console"}</h2>
          <p className="project-agent-console-sub">Agent profiles for this project.</p>
        </div>
        <div className="project-agent-console-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={() => {
              // Provider profile creation is a missing feature
              // (design.md §20 AI Connections screen).
            }}
            title="Add profile"
          >
            Add Profile
          </Button>
        </div>
      </header>

      {projectProfiles.length === 0 && (
        <div className="project-agent-console-empty">
          <span className="project-agent-console-empty-icon">▱</span>
          <p className="project-agent-console-empty-title">No agent profiles</p>
          <p className="project-agent-console-empty-desc">
            No provider profiles are associated with this project.
          </p>
        </div>
      )}

      {projectProfiles.length > 0 && (
        <div className="project-agent-console-list">
          {projectProfiles.map((profile) => (
            <div
              key={profile.id}
              className="project-agent-console-item"
            >
              <div className="project-agent-console-item-head">
                <div className="project-agent-console-item-info">
                  <h3 className="project-agent-console-item-name">{profile.name}</h3>
                  <p className="project-agent-console-item-desc">
                    {profile.is_default ? "Default profile" : "Project profile"}
                  </p>
                  <div className="project-agent-console-item-meta">
                    <span className="project-agent-console-item-planner">
                      Planner: {profile.config.planner_type}
                    </span>
                    <span className="project-agent-console-item-executor">
                      Executor: {profile.config.executor_type}
                    </span>
                    {profile.config.cloud_model && (
                      <span className="project-agent-console-item-model">
                        Model: {profile.config.cloud_model}
                      </span>
                    )}
                    {profile.config.cli_executable && (
                      <span className="project-agent-console-item-cli">
                        CLI: {profile.config.cli_executable}
                      </span>
                    )}
                  </div>
                </div>
                <div className="project-agent-console-item-actions">
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
              {profile.config.verification_command && (
                <div className="project-agent-console-item-detail">
                  <span className="project-agent-console-detail-label">Verification:</span>
                  <span className="project-agent-console-detail-value">{profile.config.verification_command}</span>
                </div>
              )}
              {profile.config.ignored_dirs.length > 0 && (
                <div className="project-agent-console-item-detail">
                  <span className="project-agent-console-detail-label">Ignored dirs:</span>
                  <span className="project-agent-console-detail-value">{profile.config.ignored_dirs.join(", ")}</span>
                </div>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export default ProjectAgentConsoleScreen;
