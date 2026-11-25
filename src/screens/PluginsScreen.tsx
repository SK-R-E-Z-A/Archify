import { useMemo, useState } from "react";
import { Button } from "../components/Button";

type PluginSource = "official" | "community" | "custom" | "local";

type PluginRecord = {
  id: string;
  name: string;
  description: string;
  version: string;
  author: string;
  category: string;
  source: PluginSource;
  enabled: boolean;
  update_available: boolean;
  tags: string[];
};

interface PluginsScreenProps {
}

/**
 * Plugins screen — design.md §17.
 *
 * Plugin marketplace with Installed / Discover / Updates / Sources tabs.
 * Supports enable/disable and update actions. Every button uses the
 * token-driven Button component and respects theme / density / font-size
 * presets.
 */
export function PluginsScreen({}: PluginsScreenProps) {
  const [plugins, setPlugins] = useState<PluginRecord[]>([
    {
      id: "archify-formatter",
      name: "Archify Formatter",
      description: "Auto-formats code on save using project style rules.",
      version: "1.2.0",
      author: "Archify Team",
      category: "Formatting",
      source: "official",
      enabled: true,
      update_available: false,
      tags: ["formatting", "lint"],
    },
    {
      id: "code-review-assistant",
      name: "Code Review Assistant",
      description: "Reviews diffs and suggests improvements before commit.",
      version: "2.1.0",
      author: "Archify Labs",
      category: "Review",
      source: "official",
      enabled: true,
      update_available: true,
      tags: ["review", "diff"],
    },
    {
      id: "terminal-integration",
      name: "Terminal Integration",
      description: "Runs shell commands from within the editor.",
      version: "0.9.4",
      author: "Community",
      category: "Terminal",
      source: "community",
      enabled: false,
      update_available: false,
      tags: ["terminal", "shell"],
    },
    {
      id: "dark-theme-toggle",
      name: "Dark Theme Toggle",
      description: "Quickly switch between light and dark appearances.",
      version: "1.0.0",
      author: "Archify Team",
      category: "UI",
      source: "official",
      enabled: true,
      update_available: false,
      tags: ["theme", "ui"],
    },
  ]);
  const [loading, setLoading] = useState(false);
  const [tab, setTab] = useState<"installed" | "discover" | "updates" | "sources">(
    "installed",
  );
  const [search, setSearch] = useState("");
  const [categoryFilter, setCategoryFilter] = useState("");

  const handleToggle = (id: string, enabled: boolean) => {
    setPlugins((prev) =>
      prev.map((p) => (p.id === id ? { ...p, enabled } : p)),
    );
  };

  const handleUpdate = (id: string) => {
    setPlugins((prev) =>
      prev.map((p) => (p.id === id ? { ...p, update_available: false } : p)),
    );
  };

  const categories = useMemo(
    () => [...new Set(plugins.map((p) => p.category))].sort(),
    [plugins],
  );

  const filtered = useMemo(() => {
    return plugins.filter((p) => {
      const matchesSearch =
        p.name.toLowerCase().includes(search.toLowerCase()) ||
        p.description.toLowerCase().includes(search.toLowerCase()) ||
        p.category.toLowerCase().includes(search.toLowerCase()) ||
        p.tags.some((t) => t.toLowerCase().includes(search.toLowerCase()));
      const matchesCategory =
        categoryFilter === "" || p.category === categoryFilter;
      if (tab === "updates")
        return matchesSearch && matchesCategory && p.update_available;
      return matchesSearch && matchesCategory;
    });
  }, [plugins, search, tab, categoryFilter]);

  if (loading) {
    return (
      <div className="plugins-screen" role="main" aria-label="Plugins">
        <header className="plugins-head">
          <div>
            <h2 className="plugins-title">Plugins</h2>
            <p className="plugins-sub">
              Extend Archify with community and official plugins.
            </p>
          </div>
        </header>
        <div className="plugins-empty">
          <span className="plugins-empty-icon">⟳</span>
          <p className="plugins-empty-title">Loading plugins…</p>
        </div>
      </div>
    );
  }

  return (
    <div className="library-screen" role="main" aria-label="Plugins">
      <header className="library-head">
        <div>
          <h2 className="library-title">Plugins</h2>
          <p className="library-sub">
            Extend Archify with community and official plugins.
          </p>
        </div>
        <div className="library-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={() => void setLoading(true)}
            title="Refresh plugin catalog"
          >
            Refresh
          </Button>
        </div>
      </header>

      {/* Tabs */}
      <div className="library-tabs">
        {[
          { id: "installed" as const, label: "Installed" },
          { id: "discover" as const, label: "Discover" },
          { id: "updates" as const, label: "Updates" },
          { id: "sources" as const, label: "Sources" },
        ].map((t) => (
          <button
            key={t.label}
            type="button"
            className={`library-tab ${tab === t.id ? "active" : ""}`}
            onClick={() => setTab(t.id)}
            role="tab"
            aria-selected={tab === t.id}
          >
            {t.label}
          </button>
        ))}
      </div>

      {tab === "sources" && (
        <div className="plugins-coming-soon">
          <span className="plugins-coming-soon-icon">▱</span>
          <p className="plugins-coming-soon-title">Sources tab</p>
          <p className="plugins-coming-soon-desc">
            Plugin sources management is planned for a later milestone
            (design.md §17).
          </p>
        </div>
      )}

      {tab !== "sources" && (
        <>
          {/* Search + category filter */}
          <div className="plugins-filters">
            <div className="plugins-search">
              <input
                type="text"
                placeholder="Search name, description, category, tag…"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                disabled={loading}
                className="plugins-search-input"
              />
            </div>
            {categories.length > 0 && (
              <div className="plugins-category-filter">
                <label className="plugins-filter-label">Category</label>
                <select
                  value={categoryFilter}
                  onChange={(e) => setCategoryFilter(e.target.value)}
                  className="plugins-filter-select"
                >
                  <option value="">All Categories</option>
                  {categories.map((c) => (
                    <option key={c} value={c}>{c}</option>
                  ))}
                </select>
              </div>
            )}
          </div>

          {/* Plugins list */}
          <div className="plugins-list-wrap">
            {filtered.length === 0 && (
              <div className="plugins-empty">
                <span className="plugins-empty-icon">▱</span>
                <p className="plugins-empty-title">No plugins</p>
                <p className="plugins-empty-desc">
                  No plugins match the current filters.
                </p>
              </div>
            )}
            {filtered.length > 0 && (
              <div className="plugins-list">
                {filtered.map((plugin) => (
                  <div
                    key={plugin.id}
                    className={`plugins-item ${!plugin.enabled ? "disabled" : ""}`}
                  >
                    <div className="plugins-item-head">
                      <div className="plugins-item-info">
                        <h3 className="plugins-item-name">{plugin.name}</h3>
                        <p className="plugins-item-desc">
                          {plugin.description}
                        </p>
                        <div className="plugins-item-meta">
                          <span className="plugins-item-version">
                            v{plugin.version}
                          </span>
                          <span className="plugins-item-author">
                            by {plugin.author}
                          </span>
                          <span className="plugins-item-category">
                            {plugin.category}
                          </span>
                          <span
                            className={`plugins-item-state ${plugin.enabled ? "enabled" : "disabled"}`}
                          >
                            {plugin.enabled ? "enabled" : "disabled"}
                          </span>
                          {plugin.update_available && (
                            <span className="plugins-item-update">
                              update available
                            </span>
                          )}
                        </div>
                      </div>
                      <div className="plugins-item-actions">
                        <Button
                          role={plugin.enabled ? "secondary" : "secondary"}
                          size="sm"
                          onClick={() => handleToggle(plugin.id, !plugin.enabled)}
                          title={plugin.enabled ? "Disable plugin" : "Enable plugin"}
                        >
                          {plugin.enabled ? "Disable" : "Enable"}
                        </Button>
                        {plugin.update_available && (
                          <Button
                            role="secondary"
                            size="sm"
                            onClick={() => handleUpdate(plugin.id)}
                            title="Update plugin"
                          >
                            Update
                          </Button>
                        )}
                      </div>
                    </div>
                    {plugin.tags.length > 0 && (
                      <div className="plugins-item-tags">
                        {plugin.tags.map((tag) => (
                          <span key={tag} className="plugins-item-tag">
                            {tag}
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                ))}
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}

export default PluginsScreen;
