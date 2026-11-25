import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

type RegisteredAgent = {
  id: string;
  name: string;
  executable: string | null;
  kind: string;
  detected: boolean;
  registered: boolean;
};

interface IndividualAgentConfigurationScreenProps {
  agentId: string;
}

/**
 * Individual Agent Configuration screen — design.md §11.4.
 *
 * Configure a single registered coding agent: enable/disable, set
 * executable path, and view detection status. Every button uses the
 * token-driven Button component.
 */
export function IndividualAgentConfigurationScreen({ agentId }: IndividualAgentConfigurationScreenProps) {
  const [agent, setAgent] = useState<RegisteredAgent | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const agents = await invoke<RegisteredAgent[]>("list_registered_agents", {});
      const a = agents.find((x) => x.id === agentId);
      setAgent(a ?? null);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load agent");
    } finally {
      setLoading(false);
    }
  }, [agentId]);

  useEffect(() => {
    void load();
  }, [load]);

  const handleRegister = async () => {
    if (!agent) return;
    try {
      await invoke("register_coding_agent", { id: agent.id });
      void load();
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to register agent");
    }
  };

  const handleRemove = async () => {
    if (!agent) return;
    if (!window.confirm("Remove this agent? This cannot be undone.")) return;
    try {
      await invoke("remove_coding_agent", { id: agent.id });
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to remove agent");
    }
  };

  if (error) {
    return (
      <div className="individual-agent-configuration-screen" role="main" aria-label="Individual Agent Configuration">
        <header className="individual-agent-configuration-head">
          <div>
            <h2 className="individual-agent-configuration-title">Agent Configuration</h2>
            <p className="individual-agent-configuration-sub">Agent #{agentId}</p>
          </div>
        </header>
        <div className="individual-agent-configuration-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      </div>
    );
  }

  if (loading || !agent) {
    return (
      <div className="individual-agent-configuration-screen" role="main" aria-label="Individual Agent Configuration">
        <header className="individual-agent-configuration-head">
          <div>
            <h2 className="individual-agent-configuration-title">Agent Configuration</h2>
            <p className="individual-agent-configuration-sub">Agent #{agentId}</p>
          </div>
        </header>
        <div className="individual-agent-configuration-empty">
          <span className="individual-agent-configuration-empty-icon">⟳</span>
          <p className="individual-agent-configuration-empty-title">Loading agent…</p>
        </div>
      </div>
    );
  }

  return (
    <div className="individual-agent-configuration-screen" role="main" aria-label="Individual Agent Configuration">
      <header className="individual-agent-configuration-head">
        <div>
          <h2 className="individual-agent-configuration-title">{agent.name}</h2>
          <span className={`individual-agent-configuration-badge ${agent.registered ? "registered" : "unregistered"}`}>
            {agent.registered ? "Registered" : "Unregistered"}
          </span>
        </div>
        <div className="individual-agent-configuration-actions">
          {!agent.registered && (
            <Button
              role="secondary"
              size="sm"
              onClick={handleRegister}
              title="Register agent"
            >
              Register
            </Button>
          )}
          {agent.registered && (
            <Button
              role="danger"
              size="sm"
              onClick={handleRemove}
              title="Remove agent"
            >
              Remove
            </Button>
          )}
        </div>
      </header>

      <div className="individual-agent-configuration-section">
        <h3 className="individual-agent-configuration-section-title">Agent Information</h3>
        <div className="individual-agent-configuration-field">
          <span className="individual-agent-configuration-field-label">Kind</span>
          <span className="individual-agent-configuration-field-value">{agent.kind}</span>
        </div>
        <div className="individual-agent-configuration-field">
          <span className="individual-agent-configuration-field-label">Executable</span>
          <span className="individual-agent-configuration-field-value">
            {agent.executable ?? "Not configured"}
          </span>
        </div>
        <div className="individual-agent-configuration-field">
          <span className="individual-agent-configuration-field-label">Detected</span>
          <span className="individual-agent-configuration-field-value">
            {agent.detected ? "Yes" : "No"}
          </span>
        </div>
        <div className="individual-agent-configuration-field">
          <span className="individual-agent-configuration-field-label">Registered</span>
          <span className="individual-agent-configuration-field-value">
            {agent.registered ? "Yes" : "No"}
          </span>
        </div>
      </div>
    </div>
  );
}

export default IndividualAgentConfigurationScreen;
