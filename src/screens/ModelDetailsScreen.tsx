import { useCallback, useEffect, useState } from "react";
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
  pricing: { input_per_token: number | null; output_per_token: number | null } | null;
};

interface ModelDetailsScreenProps {
  modelId: string;
}

/**
 * Model Details screen — design.md §21.2.
 *
 * Full detail view of a single AI model: capabilities, pricing, context
 * window, and deprecation toggle. Every button uses the token-driven
 * Button component.
 */
export function ModelDetailsScreen({ modelId }: ModelDetailsScreenProps) {
  const [model, setModel] = useState<ModelInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const models = await invoke<ModelInfo[]>("list_available_models", {});
      const m = models.find((x) => x.id === modelId);
      setModel(m ?? null);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load model details");
    } finally {
      setLoading(false);
    }
  }, [modelId]);

  useEffect(() => {
    void load();
  }, [load]);

  const handleToggleDeprecated = async () => {
    if (!model) return;
    try {
      await invoke("mark_model_deprecated", {
        model_id: model.id,
        deprecated: !model.deprecated,
      });
      setModel((prev) => (prev ? { ...prev, deprecated: !prev.deprecated } : null));
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to update model");
    }
  };

  if (error) {
    return (
      <div className="model-details-screen" role="main" aria-label="Model Details">
        <header className="model-details-head">
          <div>
            <h2 className="model-details-title">Model Details</h2>
            <p className="model-details-sub">Model #{modelId}</p>
          </div>
        </header>
        <div className="model-details-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => { setError(null); void load(); }} title="Retry">
            Retry
          </Button>
        </div>
      </div>
    );
  }

  if (loading || !model) {
    return (
      <div className="model-details-screen" role="main" aria-label="Model Details">
        <header className="model-details-head">
          <div>
            <h2 className="model-details-title">Model Details</h2>
            <p className="model-details-sub">Model #{modelId}</p>
          </div>
        </header>
        <div className="model-details-empty">
          <span className="model-details-empty-icon">⟳</span>
          <p className="model-details-empty-title">Loading model…</p>
        </div>
      </div>
    );
  }

  return (
    <div className="model-details-screen" role="main" aria-label="Model Details">
      <header className="model-details-head">
        <div>
          <h2 className="model-details-title">{model.name}</h2>
          <span className={`model-details-badge ${model.deprecated ? "deprecated" : "available"}`}>
            {model.deprecated ? "Deprecated" : "Available"}
          </span>
        </div>
        <div className="model-details-actions">
          <Button
            role={model.deprecated ? "secondary" : "danger"}
            size="sm"
            onClick={handleToggleDeprecated}
            title={model.deprecated ? "Restore model" : "Deprecate model"}
          >
            {model.deprecated ? "Restore" : "Deprecate"}
          </Button>
        </div>
      </header>

      <div className="model-details-section">
        <h3 className="model-details-section-title">Model Information</h3>
        <div className="model-details-field">
          <span className="model-details-field-label">Provider</span>
          <span className="model-details-field-value">{model.provider_name}</span>
        </div>
        <div className="model-details-field">
          <span className="model-details-field-label">Model ID</span>
          <span className="model-details-field-value">{model.id}</span>
        </div>
        <div className="model-details-field">
          <span className="model-details-field-label">Description</span>
          <span className="model-details-field-value">{model.description || "No description"}</span>
        </div>
        {model.context_window && (
          <div className="model-details-field">
            <span className="model-details-field-label">Context Window</span>
            <span className="model-details-field-value">{model.context_window} tokens</span>
          </div>
        )}
        {model.max_output_tokens && (
          <div className="model-details-field">
            <span className="model-details-field-label">Max Output Tokens</span>
            <span className="model-details-field-value">{model.max_output_tokens}</span>
          </div>
        )}
      </div>

      <div className="model-details-section">
        <h3 className="model-details-section-title">Capabilities</h3>
        <div className="model-details-capabilities">
          {model.capabilities.length > 0 ? (
            model.capabilities.map((cap) => (
              <span key={cap} className="model-details-capability">
                {cap}
              </span>
            ))
          ) : (
            <span className="model-details-capability-none">None specified</span>
          )}
        </div>
      </div>

      {model.pricing && (
        <div className="model-details-section">
          <h3 className="model-details-section-title">Pricing</h3>
          <div className="model-details-field">
            <span className="model-details-field-label">Input (per token)</span>
            <span className="model-details-field-value">
              {model.pricing.input_per_token ?? "N/A"}
            </span>
          </div>
          <div className="model-details-field">
            <span className="model-details-field-label">Output (per token)</span>
            <span className="model-details-field-value">
              {model.pricing.output_per_token ?? "N/A"}
            </span>
          </div>
        </div>
      )}
    </div>
  );
}

export default ModelDetailsScreen;
