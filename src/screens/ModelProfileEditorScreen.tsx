import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

type ModelInfo = {
  id: string;
  name: string;
  provider_id: string;
  provider_name: string;
  deprecated: boolean;
};

type AiResponsibility =
  | "planning"
  | "prompt_generation"
  | "code_review"
  | "coding"
  | "chat"
  | "default";

const RESPONSIBILITIES: { id: AiResponsibility; label: string }[] = [
  { id: "planning", label: "Planning & Reasoning" },
  { id: "prompt_generation", label: "Prompt Generation" },
  { id: "code_review", label: "Code Review" },
  { id: "coding", label: "Coding & Execution" },
  { id: "chat", label: "Chat & Conversation" },
  { id: "default", label: "Default" },
];

interface ModelProfileEditorScreenProps {
}

/**
 * Model Profile Editor screen — design.md §21.
 *
 * Edit the global model profile: default model and per-responsibility
 * model overrides. Every button uses the token-driven Button component.
 */
export function ModelProfileEditorScreen({}: ModelProfileEditorScreenProps) {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [defaultModel, setDefaultModel] = useState<string | null>(null);
  const [overrides, setOverrides] = useState<Record<string, string | null>>({});

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const [list] = await Promise.all([
        invoke<ModelInfo[]>("list_available_models", {}),
      ]);
      setModels(list);
      setDefaultModel(null);
      setOverrides({});
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load model profile");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const handleSave = async () => {
    setLoading(true);
    setError(null);
    try {
      await invoke("update_model_preferences", {
        preferences: {
          default_model: defaultModel,
          responsibility_overrides: overrides,
        },
      });
      void load();
    } catch (e) {
      setLoading(false);
      setError(typeof e === "string" ? e : "Failed to save model profile");
    }
  };

  if (error) {
    return (
      <div className="model-profile-editor-screen" role="main" aria-label="Model Profile Editor">
        <header className="model-profile-editor-head">
          <div>
            <h2 className="model-profile-editor-title">Model Profile Editor</h2>
            <p className="model-profile-editor-sub">Configure model assignments.</p>
          </div>
        </header>
        <div className="model-profile-editor-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      </div>
    );
  }

  if (loading) {
    return (
      <div className="model-profile-editor-screen" role="main" aria-label="Model Profile Editor">
        <header className="model-profile-editor-head">
          <div>
            <h2 className="model-profile-editor-title">Model Profile Editor</h2>
            <p className="model-profile-editor-sub">Configure model assignments.</p>
          </div>
        </header>
        <div className="model-profile-editor-empty">
          <span className="model-profile-editor-empty-icon">⟳</span>
          <p className="model-profile-editor-empty-title">Loading profile…</p>
        </div>
      </div>
    );
  }

  return (
    <div className="model-profile-editor-screen" role="main" aria-label="Model Profile Editor">
      <header className="model-profile-editor-head">
        <div>
          <h2 className="model-profile-editor-title">Model Profile Editor</h2>
          <p className="model-profile-editor-sub">Configure model assignments.</p>
        </div>
        <div className="model-profile-editor-actions">
          <Button
            role="primary"
            size="sm"
            onClick={handleSave}
            disabled={loading}
            title={loading ? "Saving…" : "Save profile"}
          >
            {loading ? "Saving…" : "Save Profile"}
          </Button>
        </div>
      </header>

      <div className="model-profile-editor-section">
        <h3 className="model-profile-editor-section-title">Default Model</h3>
        <div className="model-profile-editor-field">
          <select
            value={defaultModel ?? ""}
            onChange={(e) => setDefaultModel(e.target.value || null)}
            disabled={loading}
            className="model-profile-editor-select"
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

      <div className="model-profile-editor-section">
        <h3 className="model-profile-editor-section-title">Responsibility Overrides</h3>
        <div className="model-profile-editor-overrides">
          {RESPONSIBILITIES.map((resp) => (
            <div key={resp.id} className="model-profile-editor-override-row">
              <span className="model-profile-editor-override-label">{resp.label}</span>
              <select
                value={overrides[resp.id] ?? ""}
                onChange={(e) =>
                  setOverrides((prev) => ({
                    ...prev,
                    [resp.id]: e.target.value || null,
                  }))
                }
                disabled={loading}
                className="model-profile-editor-select"
              >
                <option value="">Inherit default</option>
                {models.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name} ({m.provider_name})
                  </option>
                ))}
              </select>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

export default ModelProfileEditorScreen;
