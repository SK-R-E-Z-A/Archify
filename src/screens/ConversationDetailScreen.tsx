import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

type SessionMessage = {
  id: string;
  role: string;
  content: string;
  created_at: string;
};

type SessionRecord = {
  id: string;
  title: string;
  messages: SessionMessage[];
  updated_at: string;
};

interface ConversationDetailScreenProps {
  sessionId: string;
}

/**
 * Conversation Detail screen — design.md §10.
 *
 * Full conversation view for a single session, showing messages and
 * allowing continuation. Every button uses the token-driven Button
 * component.
 */
export function ConversationDetailScreen({ sessionId }: ConversationDetailScreenProps) {
  const [session, setSession] = useState<SessionRecord | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [newMessage, setNewMessage] = useState("");

  const load = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const s = await invoke<SessionRecord>("get_session", { session_id: sessionId });
      setSession(s ?? null);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load conversation");
    } finally {
      setLoading(false);
    }
  }, [sessionId]);

  useEffect(() => {
    void load();
  }, [load]);

  const handleSend = async () => {
    const text = newMessage.trim();
    if (!text || !session) return;
    try {
      await invoke("send_session_message", {
        sessionId: session.id,
        content: text,
      });
      setNewMessage("");
      void load();
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to send message");
    }
  };

  if (error) {
    return (
      <div className="conversation-detail-screen" role="main" aria-label="Conversation Detail">
        <header className="conversation-detail-head">
          <div>
            <h2 className="conversation-detail-title">Conversation Detail</h2>
            <p className="conversation-detail-sub">Session #{sessionId}</p>
          </div>
        </header>
        <div className="conversation-detail-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => { setError(null); void load(); }} title="Retry">
            Retry
          </Button>
        </div>
      </div>
    );
  }

  if (loading || !session) {
    return (
      <div className="conversation-detail-screen" role="main" aria-label="Conversation Detail">
        <header className="conversation-detail-head">
          <div>
            <h2 className="conversation-detail-title">Conversation Detail</h2>
            <p className="conversation-detail-sub">Session #{sessionId}</p>
          </div>
        </header>
        <div className="conversation-detail-empty">
          <span className="conversation-detail-empty-icon">⟳</span>
          <p className="conversation-detail-empty-title">Loading conversation…</p>
        </div>
      </div>
    );
  }

  return (
    <div className="conversation-detail-screen" role="main" aria-label="Conversation Detail">
      <header className="conversation-detail-head">
        <div>
          <h2 className="conversation-detail-title">{session.title}</h2>
          <span className="conversation-detail-updated">
            {new Date(session.updated_at).toLocaleString()}
          </span>
        </div>
      </header>

      <div className="conversation-detail-messages">
        {session.messages.length === 0 && (
          <div className="conversation-detail-empty-messages">
            No messages yet. Start the conversation below.
          </div>
        )}
        {session.messages.map((msg) => (
          <div
            key={msg.id}
            className={`conversation-detail-message ${msg.role}`}
          >
            <span className="conversation-detail-message-role">
              {msg.role.charAt(0).toUpperCase() + msg.role.slice(1)}
            </span>
            <p className="conversation-detail-message-content">{msg.content}</p>
          </div>
        ))}
      </div>

      <div className="conversation-detail-input-wrap">
        <input
          type="text"
          placeholder="Type a message…"
          value={newMessage}
          onChange={(e) => setNewMessage(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && handleSend()}
          className="conversation-detail-input"
        />
        <Button
          role="primary"
          size="sm"
          onClick={handleSend}
          disabled={!newMessage.trim()}
          title="Send message"
        >
          Send
        </Button>
      </div>
    </div>
  );
}

export default ConversationDetailScreen;
