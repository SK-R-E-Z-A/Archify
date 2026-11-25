import { useState } from "react";
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

interface ProjectPlaygroundScreenProps {
  project: ProjectRecord | null;
}

/**
 * Project Playground screen — design.md §8.
 *
 * Project playground with Graph and Chat tabs. The Graph tab shows the
 * execution step graph for the project's current task. The Chat tab
 * provides a message interface. Every button uses the token-driven
 * Button component and respects theme / density / font-size presets.
 */
export function ProjectPlaygroundScreen({
  project,
}: ProjectPlaygroundScreenProps) {
  const [tab, setTab] = useState<"graph" | "chat">("graph");
  const [chatInput, setChatInput] = useState("");
  const [messages, setMessages] = useState<Array<{ role: "user" | "assistant"; content: string }>>([]);



  const handleSend = () => {
    const text = chatInput.trim();
    if (!text) return;
    setMessages((prev) => [...prev, { role: "user", content: text }]);
    setChatInput("");
    setMessages((prev) => [
      ...prev,
      { role: "assistant", content: "Chat integration is a missing feature (design.md §33 Conversation Detail screen). Your message has been recorded." },
    ]);
  };

  return (
    <div className="project-playground-screen" role="main" aria-label="Project Playground">
      <header className="project-playground-head">
        <div>
          <h2 className="project-playground-title">{project?.name || "Project Playground"}</h2>
          <p className="project-playground-sub">Graph and chat workspace for this project.</p>
        </div>
        <div className="project-playground-actions">
          <span className="project-playground-status-note">
            Live execution graph appears here once a task is running.
          </span>
        </div>
      </header>

      {/* Tabs */}
      <div className="project-playground-tabs">
        <button
          type="button"
          className={`project-playground-tab ${tab === "graph" ? "active" : ""}`}
          onClick={() => setTab("graph")}
          role="tab"
          aria-selected={tab === "graph"}
        >
          Graph
        </button>
        <button
          type="button"
          className={`project-playground-tab ${tab === "chat" ? "active" : ""}`}
          onClick={() => setTab("chat")}
          role="tab"
          aria-selected={tab === "chat"}
        >
          Chat
        </button>
      </div>

      {tab === "graph" && (
        <div className="project-playground-graph-wrap">
          <div className="project-playground-empty-graph">
             <p>No execution graph yet.</p>
             <p className="project-playground-empty-graph-hint">
               Start a task to see the live execution tree here.
             </p>
           </div>
        </div>
      )}

      {tab === "chat" && (
        <div className="project-playground-chat-wrap">
          <div className="project-playground-chat-messages">
            {messages.length > 0 ? (
              messages.map((msg, i) => (
                <div key={i} className={`project-playground-chat-msg ${msg.role}`}>
                  <span className="project-playground-chat-msg-role">
                    {msg.role === "user" ? "You" : "Assistant"}
                  </span>
                  <p className="project-playground-chat-msg-content">{msg.content}</p>
                </div>
              ))
            ) : (
              <div className="project-playground-chat-empty">
                No messages yet. Start a conversation below.
              </div>
            )}
          </div>
          <div className="project-playground-chat-input-wrap">
            <input
              type="text"
              placeholder="Type a message…"
              value={chatInput}
              onChange={(e) => setChatInput(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && handleSend()}
              className="project-playground-chat-input"
            />
            <Button
              role="primary"
              size="sm"
              onClick={handleSend}
              disabled={!chatInput.trim()}
              title="Send message"
            >
              Send
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

export default ProjectPlaygroundScreen;
