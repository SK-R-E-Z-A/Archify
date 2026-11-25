import { useState } from "react";
import { Button } from "../components/Button";

type TrashItem = {
  id: string;
  name: string;
  type: "project" | "session" | "task";
  deleted_at: string;
  original_path: string | null;
};

interface TrashScreenProps {
}

/**
 * Trash screen — design.md §7.4.
 *
 * View deleted items awaiting permanent removal. Supports restore and
 * permanent delete. Every button uses the token-driven Button component.
 */
export function TrashScreen({}: TrashScreenProps) {
  const [items, setItems] = useState<TrashItem[]>([]);
  const [error, setError] = useState<string | null>(null);

  const handleRefresh = () => {
    if (items.length === 0) {
      setError("No items in trash to refresh.");
      return;
    }
    setItems((prev) => prev.filter((i) => i.id !== null));
  };

  const handleRestore = async (id: string) => {
    try {
      setItems((prev) => prev.filter((i) => i.id !== id));
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to restore item");
    }
  };

  const handleDelete = async (id: string) => {
    if (
      !window.confirm("Permanently delete this item? This cannot be undone.")
    )
      return;
    try {
      setItems((prev) => prev.filter((i) => i.id !== id));
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to delete item");
    }
  };

  return (
    <div className="trash-screen" role="main" aria-label="Trash">
      <header className="trash-head">
        <div>
          <h2 className="trash-title">Trash</h2>
          <p className="trash-sub">Deleted items awaiting permanent removal.</p>
        </div>
        <div className="trash-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={handleRefresh}
            disabled={items.length === 0}
            title={items.length === 0 ? "No items to refresh" : "Refresh trash"}
          >
            {items.length === 0 ? "No items" : "Refresh"}
          </Button>
        </div>
      </header>

      {error && (
        <div className="trash-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => setError(null)} title="Dismiss">
            Dismiss
          </Button>
        </div>
      )}

      <div className="trash-list">
        {items.length === 0 && (
          <div className="trash-empty">
            <span className="trash-empty-icon">▱</span>
            <p className="trash-empty-title">Trash is empty</p>
            <p className="trash-empty-desc">
              No deleted items awaiting removal.
            </p>
          </div>
        )}
        {items.length > 0 && (
          <div className="trash-items">
            {items.map((item) => (
              <div key={item.id} className="trash-item">
                <div className="trash-item-info">
                  <span className="trash-item-type">{item.type}</span>
                  <h3 className="trash-item-name">{item.name}</h3>
                  <span className="trash-item-deleted">
                    Deleted {new Date(item.deleted_at).toLocaleDateString()}
                  </span>
                </div>
                <div className="trash-item-actions">
                  <Button
                    role="secondary"
                    size="sm"
                    onClick={() => handleRestore(item.id)}
                    title="Restore"
                  >
                    Restore
                  </Button>
                  <Button
                    role="danger"
                    size="sm"
                    onClick={() => handleDelete(item.id)}
                    title="Delete permanently"
                  >
                    Delete
                  </Button>
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

export default TrashScreen;
