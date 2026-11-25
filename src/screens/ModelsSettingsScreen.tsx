import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

type ModelInfo = {
  id: string;
  name: string;
  description: string;
  provider_id: string;
  provider_name: string;
  context_window: number | null;
  max_output_tokens: number | null;
  available: boolean;
  deprecated: boolean;
  capabilities: string[];
};

type AiResponsibility =
  | "planning"
  | "prompt_generation"
  | "code_review"
  | "coding"
  | "chat"
  | "default";

type ModelPreferences = {
  default_model: string | null;
  responsibility_overrides: Record<string, string>;
};

const RESPONSIBILITIES: { id: AiResponsibility; label: string }[] = [
  { id: "planning", label: "Planning & Reasoning" },
  { id: "prompt_generation", label: "Prompt Generation" },
  { id: "code_review", label: "Code Review" },
  { id: "coding", label: "Coding & Execution" },
  { id: "chat", label: "Chat & Conversation" },
  { id: "default", label: "Default" },
];

interface ModelsSettingsScreenProps {
}

/**
 * Models settings screen — design.md §21 / §24.5.
 *
 * Manage AI models: available models, default model, per-responsibility
 * overrides, and deprecation. Every button uses the token-driven Button
 * component.
 */
export function ModelsSettingsScreen({}: ModelsSettingsScreenProps) {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [preferences, setPreferences] = useState<ModelPreferences | null>(
    null,
  );
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadModels = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [list, prefs] = await Promise.all([
        invoke<ModelInfo[]>("list_available_models", {}),
        invoke<ModelPreferences>("get_model_preferences", {}),
      ]);
      setModels(list);
      setPreferences(prefs);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load models");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadModels();
  }, [loadModels]);

  const handleSetDefault = async (modelId: string | null) => {
    try {
      await invoke("set_default_model", { model_id: modelId });
      void loadModels();
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to set default model");
    }
  };

  const handleSelectResponsibility = async (
    responsibility: AiResponsibility,
    modelId: string,
  ) => {
    try {
      await invoke("select_model_for_responsibility", {
        responsibility,
        model_id: modelId,
      });
      void loadModels();
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to set model preference");
    }
  };

  const handleToggleDeprecated = async (modelId: string, deprecated: boolean) => {
    try {
      await invoke("mark_model_deprecated", { model_id: modelId, deprecated });
      void loadModels();
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to update model");
    }
  };

  const defaultModelId = preferences?.default_model ?? null;
  const byProvider = useMemo(() => {
    const map = new Map<string, ModelInfo[]>();
    models.forEach((m) => {
      const arr = map.get(m.provider_id) ?? [];
      arr.push(m);
      map.set(m.provider_id, arr);
    });
    return map;
  }, [models]);

  if (error) {
    return (
      <div className="models-settings-screen" role="main" aria-label="Models Settings">
        <header className="models-settings-head">
          <div>
            <h2 className="models-settings-title">Models</h2>
            <p className="models-settings-sub">AI model preferences.</p>
          </div>
        </header>
        <div className="models-settings-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => { setError(null); void loadModels(); }} title="Retry">
            Retry
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="models-settings-screen" role="main" aria-label="Models Settings">
      <header className="models-settings-head">
        <div>
          <h2 className="models-settings-title">Models</h2>
          <p className="models-settings-sub">AI model preferences.</p>
        </div>
        <div className="models-settings-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={() => void loadModels()}
            disabled={loading}
            title={loading ? "Refreshing…" : "Refresh models"}
          >
            {loading ? "Refreshing…" : "Refresh"}
          </Button>
        </div>
      </header>

      {/* Default model */}
      <div className="models-settings-section">
        <h3 className="models-settings-section-title">Default Model</h3>
        <div className="models-settings-field">
          <select
            value={defaultModelId ?? ""}
            onChange={(e) => handleSetDefault(e.target.value || null)}
            disabled={loading}
            className="models-settings-select"
          >
            <option value="">Use provider default</option>
            {models.map((m) => (
              <option key={m.id} value={m.id}>
                {m.name} ({m.provider_name})
              </option>
            ))}
          </select>
        </div>
      </div>

      {/* Responsibility overrides */}
      <div className="models-settings-section">
        <h3 className="models-settings-section-title">Responsibility Overrides</h3>
        <div className="models-settings-responsibilities">
          {RESPONSIBILITIES.map((resp) => {
            const current =
              preferences?.responsibility_overrides[resp.id] ?? null;
            return (
              <div key={resp.id} className="models-settings-responsibility">
                <span className="models-settings-responsibility-label">
                  {resp.label}
                </span>
                <select
                  value={current ?? ""}
                  onChange={(e) =>
                    handleSelectResponsibility(resp.id, e.target.value)
                  }
                  disabled={loading}
                  className="models-settings-select"
                >
                  <option value="">Inherit default</option>
                  {models.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.name} ({m.provider_name})
                    </option>
                  ))}
                </select>
              </div>
            );
          })}
        </div>
      </div>

      {/* Available models */}
      <div className="models-settings-section">
        <h3 className="models-settings-section-title">Available Models</h3>
        <div className="models-settings-models">
          {[...byProvider.entries()].map(([providerId, providerModels]) => (
            <div key={providerId} className="models-settings-provider-group">
              <span className="models-settings-provider-name">
                {providerModels[0]?.provider_name}
              </span>
              {providerModels.map((m) => (
                <div
                  key={m.id}
                  className={`models-settings-model-item ${
                    m.deprecated ? "deprecated" : ""
                  } ${defaultModelId === m.id ? "default" : ""}`}
                >
                  <div className="models-settings-model-info">
                    <span className="models-settings-model-name">{m.name}</span>
                    <span className="models-settings-model-id">{m.id}</span>
                    {m.context_window && (
                      <span className="models-settings-model-context">
                        {m.context_window} tokens
                      </span>
                    )}
                  </div>
                  <div className="models-settings-model-actions">
                    {defaultModelId !== m.id && (
                      <Button
                        role="secondary"
                        size="sm"
                        onClick={() => handleSetDefault(m.id)}
                        title="Set as default"
                      >
                        Set Default
                      </Button>
                    )}
                    <Button
                      role={
                        m.deprecated ? "secondary" : "danger"
                      }
                      size="sm"
                      onClick={() =>
                        handleToggleDeprecated(m.id, !m.deprecated)
                      }
                      title={m.deprecated ? "Restore model" : "Deprecate model"}
                    >
                      {m.deprecated ? "Restore" : "Deprecate"}
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          ))}
        </div>
        {models.length === 0 && (
          <div className="models-settings-empty">
            <span className="models-settings-empty-icon">▱</span>
            <p className="models-settings-empty-title">No models available</p>
          </div>
        )}
      </div>
    </div>
  );
}

export default ModelsSettingsScreen;
