import { useCallback, useEffect, useState } from "react";
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

type SessionKind = "prompt" | "cli";

type SessionRecord = {
  id: string;
  title: string;
  kind: SessionKind;
  project_id: string | null;
  project_root: string | null;
  provider_profile_id: string | null;
  cli_preset_id: string | null;
  cli_executable: string | null;
  cli_args: string[];
  prompt_delivery: string;
  messages: Array<{ id: string; role: string; content: string; created_at: string }>;
  created_at: string;
  updated_at: string;
};

function formatRelativeTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffSecs = Math.floor(diffMs / 1000);
  const diffMins = Math.floor(diffSecs / 60);
  const diffHours = Math.floor(diffMins / 60);
  const diffDays = Math.floor(diffHours / 24);

  if (diffSecs < 60) return "just now";
  if (diffMins < 60) return `${diffMins}m ago`;
  if (diffHours < 24) return `${diffHours}h ago`;
  if (diffDays < 7) return `${diffDays}d ago`;
  return date.toLocaleDateString();
}

interface ProjectConversationsScreenProps {
  project: ProjectRecord | null;
}

/**
 * Project Conversations screen — design.md §10.
 *
 * Project-scoped conversation list showing sessions for this project.
 * Every button uses the token-driven Button component and respects
 * theme / density / font-size presets.
 */
export function ProjectConversationsScreen({
  project,
}: ProjectConversationsScreenProps) {
  const [sessions, setSessions] = useState<SessionRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadSessions = useCallback(async () => {
    if (!project) return;
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<SessionRecord[]>("list_sessions", {
        project_id: project.id,
      });
      setSessions(list);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load sessions");
    } finally {
      setLoading(false);
    }
  }, [project]);

  useEffect(() => {
    void loadSessions();
  }, [loadSessions]);

  const handleArchive = async (sessionId: string) => {
    try {
      await invoke("archive_session", { sessionId });
      setSessions((prev) => prev.filter((s) => s.id !== sessionId));
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to archive session");
    }
  };

  const handleDelete = async (sessionId: string) => {
    if (!window.confirm("Delete this session? This cannot be undone.")) return;
    try {
      await invoke("delete_session", { sessionId });
      setSessions((prev) => prev.filter((s) => s.id !== sessionId));
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to delete session");
    }
  };

  if (error) {
    return (
      <div className="project-conversations-screen" role="main" aria-label="Project Conversations">
        <header className="project-conversations-head">
          <div>
            <h2 className="project-conversations-title">{project?.name || "Project Conversations"}</h2>
            <p className="project-conversations-sub">Conversations for this project.</p>
          </div>
        </header>
        <div className="project-conversations-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => { setError(null); void loadSessions(); }} title="Retry">
            Retry
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="project-conversations-screen" role="main" aria-label="Project Conversations">
      <header className="project-conversations-head">
        <div>
          <h2 className="project-conversations-title">{project?.name || "Project Conversations"}</h2>
          <p className="project-conversations-sub">Conversations for this project.</p>
        </div>
        <div className="project-conversations-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={() => void loadSessions()}
            disabled={loading}
            title={loading ? "Refreshing…" : "Refresh conversations"}
          >
            {loading ? "Refreshing…" : "Refresh"}
          </Button>
        </div>
      </header>

      {sessions.length === 0 && (
        <div className="project-conversations-empty">
          <span className="project-conversations-empty-icon">▱</span>
          <p className="project-conversations-empty-title">No conversations</p>
          <p className="project-conversations-empty-desc">
            Conversations for this project will appear here.
          </p>
        </div>
      )}

      {sessions.length > 0 && (
        <div className="project-conversations-list">
          {sessions.map((session) => (
            <div
              key={session.id}
              className={`project-conversations-item ${session.kind}`}
            >
              <div className="project-conversations-item-head">
                <div className="project-conversations-item-info">
                  <h3 className="project-conversations-item-title">{session.title}</h3>
                  <div className="project-conversations-item-meta">
                    <span className="project-conversations-item-kind">
                      {session.kind === "prompt" ? "Prompt" : "CLI"}
                    </span>
                    <span className="project-conversations-item-updated">
                      {formatRelativeTime(session.updated_at)}
                    </span>
                    <span className="project-conversations-item-messages">
                      {session.messages.length} message{session.messages.length === 1 ? "" : "s"}
                    </span>
                  </div>
                </div>
                <div className="project-conversations-item-actions">
                  <Button
                    role="secondary"
                    size="sm"
                    onClick={() => handleArchive(session.id)}
                    title="Archive session"
                  >
                    Archive
                  </Button>
                  <Button
                    role="danger"
                    size="sm"
                    onClick={() => handleDelete(session.id)}
                    title="Delete session"
                  >
                    Delete
                  </Button>
                </div>
              </div>
              <div className="project-conversations-item-preview">
                {session.messages.slice(0, 3).map((msg) => (
                  <div
                    key={msg.id}
                    className={`project-conversations-item-preview-msg ${msg.role}`}
                  >
                    {msg.content.length > 80
                      ? `${msg.content.slice(0, 80)}…`
                      : msg.content}
                  </div>
                ))}
                {session.messages.length > 3 && (
                  <span className="project-conversations-item-preview-more">
                    +{session.messages.length - 3} more
                  </span>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export default ProjectConversationsScreen;
