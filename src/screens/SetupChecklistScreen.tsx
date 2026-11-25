import { useState } from "react";
import { Button } from "../components/Button";

type ChecklistItem = {
  id: string;
  title: string;
  description: string;
  completed: boolean;
  optional: boolean;
};

interface SetupChecklistScreenProps {
}

/**
 * Setup Checklist screen — design.md §26.3.
 *
 * Checklist of recommended setup steps after first launch. Every button
 * uses the token-driven Button component.
 */
export function SetupChecklistScreen({}: SetupChecklistScreenProps) {
  const [items, setItems] = useState<ChecklistItem[]>([
    { id: "add-project", title: "Add your first project", description: "Open a folder to begin working with Archify.", completed: false, optional: false },
    { id: "configure-provider", title: "Configure an AI provider", description: "Set up your AI provider and model preferences.", completed: false, optional: false },
    { id: "create-task", title: "Create your first task", description: "Describe your goal and let Archify plan and execute it.", completed: false, optional: false },
    { id: "review-plan", title: "Review a task plan", description: "Approve or request changes to a generated plan.", completed: false, optional: true },
    { id: "enable-notifications", title: "Enable notifications", description: "Stay informed about task progress and failures.", completed: false, optional: true },
    { id: "setup-automation", title: "Set up an automation", description: "Automate repetitive workflows with triggers and actions.", completed: false, optional: true },
  ]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleToggle = async (id: string, completed: boolean) => {
    setItems((prev) =>
      prev.map((item) =>
        item.id === id ? { ...item, completed } : item,
      ),
    );
  };

  const handleMarkAllComplete = async () => {
    setLoading(true);
    setError(null);
    try {
      setItems((prev) => prev.map((item) => ({ ...item, completed: true })));
      setLoading(false);
    } catch (e) {
      setLoading(false);
      setError(typeof e === "string" ? e : "Failed to update checklist");
    }
  };

  const completedCount = items.filter((i) => i.completed).length;
  const totalCount = items.length;

  if (error) {
    return (
      <div className="setup-checklist-screen" role="main" aria-label="Setup Checklist">
        <header className="setup-checklist-head">
          <div>
            <h2 className="setup-checklist-title">Setup Checklist</h2>
            <p className="setup-checklist-sub">Recommended setup steps.</p>
          </div>
        </header>
        <div className="setup-checklist-error" role="alert">
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
    <div className="setup-checklist-screen" role="main" aria-label="Setup Checklist">
      <header className="setup-checklist-head">
        <div>
          <h2 className="setup-checklist-title">Setup Checklist</h2>
          <p className="setup-checklist-sub">
            {completedCount} of {totalCount} steps complete
          </p>
        </div>
        <div className="setup-checklist-actions">
          {completedCount < totalCount && (
            <Button
              role="secondary"
              size="sm"
              onClick={handleMarkAllComplete}
              disabled={loading}
              title="Mark all complete"
            >
              {loading ? "Marking…" : "Mark All Complete"}
            </Button>
          )}
        </div>
      </header>

      <div className="setup-checklist-list">
        {items.map((item) => (
          <div
            key={item.id}
            className={`setup-checklist-item ${item.completed ? "completed" : ""} ${item.optional ? "optional" : ""}`}
          >
            <input
              type="checkbox"
              checked={item.completed}
              onChange={(e) => handleToggle(item.id, e.target.checked)}
            />
            <div className="setup-checklist-item-content">
              <h3 className="setup-checklist-item-title">{item.title}</h3>
              <p className="setup-checklist-item-desc">{item.description}</p>
              {item.optional && (
                <span className="setup-checklist-item-optional">Optional</span>
              )}
            </div>
          </div>
        ))}
      </div>

      {completedCount === totalCount && (
        <div className="setup-checklist-complete">
          <span className="setup-checklist-complete-icon">✓</span>
          <p className="setup-checklist-complete-title">All setup steps complete!</p>
        </div>
      )}
    </div>
  );
}

export default SetupChecklistScreen;
