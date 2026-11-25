import { useCallback, useState } from "react";
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

interface AgentsSettingsScreenProps {
}

/**
 * Agents settings screen — design.md §24.6.
 *
 * Manage registered AI coding agents: detect, register, enable/disable,
 * and remove. Every button uses the token-driven Button component.
 */
export function AgentsSettingsScreen({}: AgentsSettingsScreenProps) {
  const [agents, setAgents] = useState<RegisteredAgent[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadAgents = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<RegisteredAgent[]>("list_registered_agents", {});
      setAgents(list);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load agents");
    } finally {
      setLoading(false);
    }
  }, []);

  const handleDetect = async () => {
    setLoading(true);
    try {
      await invoke("detect_coding_agents", {});
      void loadAgents();
    } catch (e) {
      setLoading(false);
      setError(typeof e === "string" ? e : "Failed to detect agents");
    }
  };

  const handleRegister = async (agentId: string) => {
    try {
      await invoke("register_coding_agent", { id: agentId });
      void loadAgents();
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to register agent");
    }
  };

  const handleRemove = async (agentId: string) => {
    if (!window.confirm("Remove this agent? This cannot be undone.")) return;
    try {
      await invoke("remove_coding_agent", { id: agentId });
      void loadAgents();
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to remove agent");
    }
  };

  return (
    <div className="agents-settings-screen" role="main" aria-label="Agents Settings">
      <header className="agents-settings-head">
        <div>
          <h2 className="agents-settings-title">Agents</h2>
          <p className="agents-settings-sub">AI coding agent management.</p>
        </div>
        <div className="agents-settings-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={handleDetect}
            disabled={loading}
            title={loading ? "Detecting…" : "Detect agents"}
          >
            {loading ? "Detecting…" : "Detect Agents"}
          </Button>
          <Button
            role="secondary"
            size="sm"
            onClick={() => void loadAgents()}
            disabled={loading}
            title={loading ? "Refreshing…" : "Refresh agents"}
          >
            {loading ? "Refreshing…" : "Refresh"}
          </Button>
        </div>
      </header>

      {error && (
        <div className="agents-settings-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="agents-settings-list">
        {agents.length === 0 && (
          <div className="agents-settings-empty">
            <span className="agents-settings-empty-icon">▱</span>
            <p className="agents-settings-empty-title">No agents registered</p>
            <p className="agents-settings-empty-desc">
              Detect coding agents to register them with Archify.
            </p>
          </div>
        )}
        {agents.map((agent) => (
          <div
            key={agent.id}
            className={`agents-settings-item ${
              agent.registered ? "registered" : "unregistered"
            }`}
          >
            <div className="agents-settings-item-info">
              <h3 className="agents-settings-item-name">{agent.name}</h3>
              <span className="agents-settings-item-kind">{agent.kind}</span>
              <div className="agents-settings-item-meta">
                <span className="agents-settings-item-badge">
                  {agent.registered ? "Registered" : "Unregistered"}
                </span>
                {agent.detected && (
                  <span className="agents-settings-item-badge detected">
                    Detected
                  </span>
                )}
                {agent.executable && (
                  <span className="agents-settings-item-badge">
                    {agent.executable}
                  </span>
                )}
              </div>
            </div>
            <div className="agents-settings-item-actions">
              {!agent.registered && (
                <Button
                  role="secondary"
                  size="sm"
                  onClick={() => handleRegister(agent.id)}
                  title="Register agent"
                >
                  Register
                </Button>
              )}
              {agent.registered && (
                <Button
                  role="danger"
                  size="sm"
                  onClick={() => handleRemove(agent.id)}
                  title="Remove agent"
                >
                  Remove
                </Button>
              )}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

export default AgentsSettingsScreen;
