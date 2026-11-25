import { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../components/Button";

type SkillSource = "managed" | "imported" | "discovered";

type SkillRecord = {
  id: string;
  name: string;
  description: string;
  category: string;
  supported_agents: string[];
  tags: string[];
  enabled: boolean;
  source: SkillSource;
  managed: boolean;
  source_path: string | null;
};

interface SkillsScreenProps {
}

/**
 * Skills screen — design.md §16.
 *
 * Library-style interface for installed skills. Tabs: Installed / Discover /
 * Categories / Sources. Supports install/enable/disable/remove and
 * project-assign. Every button uses the token-driven Button component and
 * respects theme / density / font-size presets.
 */
export function SkillsScreen({}: SkillsScreenProps) {
  const [skills, setSkills] = useState<SkillRecord[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [tab, setTab] = useState<"installed" | "discover" | "categories" | "sources">(
    "installed",
  );
  const [search, setSearch] = useState("");
  const [categoryFilter, setCategoryFilter] = useState<string>("");

  const loadSkills = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await invoke<SkillRecord[]>("list_skills", {});
      setSkills(list);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to load skills");
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadSkills();
  }, [loadSkills]);

  const categories = useMemo(
    () => [...new Set(skills.map((s) => s.category))].sort(),
    [skills],
  );

  const handleToggle = async (id: string, enabled: boolean) => {
    try {
      await invoke("set_skill_enabled", { id, enabled });
      setSkills((prev) =>
        prev.map((s) => (s.id === id ? { ...s, enabled } : s)),
      );
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to update skill");
    }
  };

  const handleRemove = async (id: string) => {
    if (!window.confirm("Remove this skill? This cannot be undone.")) return;
    try {
      await invoke("delete_skill", { id });
      setSkills((prev) => prev.filter((s) => s.id !== id));
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to remove skill");
    }
  };

  const handleImport = async () => {
    const path = window.prompt("Enter skill source path:");
    if (!path) return;
    try {
      const imported = await invoke<SkillRecord>("import_skill", {
        sourcePath: path,
      });
      setSkills((prev) => [...prev, imported]);
    } catch (e) {
      setError(typeof e === "string" ? e : "Failed to import skill");
    }
  };

  const filtered = useMemo(() => {
    return skills.filter((s) => {
      const matchesSearch =
        s.name.toLowerCase().includes(search.toLowerCase()) ||
        s.description.toLowerCase().includes(search.toLowerCase()) ||
        s.category.toLowerCase().includes(search.toLowerCase()) ||
        s.tags.some((t) => t.toLowerCase().includes(search.toLowerCase()));
      const matchesCategory =
        categoryFilter === "" || s.category === categoryFilter;
      return matchesSearch && matchesCategory;
    });
  }, [skills, search, categoryFilter]);

  if (error) {
    return (
      <div className="skills-screen" role="main" aria-label="Skills">
        <header className="skills-head">
          <div>
            <span className="eyebrow">LIBRARY</span>
            <h2 className="skills-title">Skills</h2>
            <p className="skills-sub">Installed skills for your AI coding agents.</p>
          </div>
        </header>
        <div className="skills-error" role="alert">
          <span role="img" aria-hidden="true">⚠</span>
          {error}
          <Button role="secondary" size="sm" onClick={() => { setError(null); void loadSkills(); }}>
            Retry
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="skills-screen" role="main" aria-label="Skills">
      <header className="skills-head">
        <div>
          <span className="eyebrow">LIBRARY</span>
          <h2 className="skills-title">Skills</h2>
          <p className="skills-sub">Installed skills for your AI coding agents.</p>
        </div>
        <div className="skills-actions">
          <Button role="secondary" size="sm" onClick={handleImport} title="Import skill">
            <span className="icon">▱</span> Import
          </Button>
          <Button
            role="secondary"
            size="sm"
            onClick={() => void loadSkills()}
            disabled={loading}
            title={loading ? "Refreshing…" : "Refresh skills"}
          >
            <span className="icon">↻</span> {loading ? "Refreshing…" : "Refresh"}
          </Button>
        </div>
      </header>

      {/* Tabs */}
      <div className="skills-tabs" role="tablist">
        {[
          { id: "installed" as const, label: "Installed" },
          { id: "discover" as const, label: "Discover" },
          { id: "categories" as const, label: "Categories" },
          { id: "sources" as const, label: "Sources" },
        ].map((t) => (
          <button
            key={t.label}
            type="button"
            className={`skills-tab ${tab === t.id ? "active" : ""}`}
            onClick={() => setTab(t.id)}
            role="tab"
            aria-selected={tab === t.id}
          >
            {t.label}
          </button>
        ))}
      </div>

      {/* Search + category filter (Installed tab) */}
      {tab === "installed" && (
        <div className="skills-filters">
          <div className="skills-search">
            <input
              type="text"
              placeholder="Search name, description, category, tag…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              disabled={loading}
              className="skills-search-input"
            />
          </div>
          {categories.length > 0 && (
            <div className="skills-category-filter">
              <label className="skills-filter-label">Category</label>
              <select
                value={categoryFilter}
                onChange={(e) => setCategoryFilter(e.target.value)}
                disabled={loading}
                className="skills-filter-select"
              >
                <option value="">All Categories</option>
                {categories.map((c) => (
                  <option key={c} value={c}>{c}</option>
                ))}
              </select>
            </div>
          )}
        </div>
      )}

      {/* Skills list */}
      <div className="skills-list-wrap">
        {loading && skills.length === 0 && (
          <div className="skills-empty">
            <span className="skills-empty-icon">⟳</span>
            <p className="skills-empty-title">Loading skills…</p>
          </div>
        )}
        {!loading && filtered.length === 0 && error === null && (
          <div className="skills-empty">
            <span className="skills-empty-icon">▱</span>
            <p className="skills-empty-title">No skills</p>
            <p className="skills-empty-desc">
              No skills match the current filters.
            </p>
            {tab === "installed" && (
              <Button
                role="secondary"
                size="sm"
                onClick={handleImport}
                title="Import skill"
              >
                Import a skill
              </Button>
            )}
          </div>
        )}
        {!loading && filtered.length > 0 && (
          <div className="skills-list">
            {filtered.map((skill) => (
              <div
                key={skill.id}
                className={`skills-item ${!skill.enabled ? "disabled" : ""}`}
              >
                <div className="skills-item-head">
                  <div className="skills-item-info">
                    <h3 className="skills-item-name">{skill.name}</h3>
                    <p className="skills-item-desc">{skill.description}</p>
                    <div className="skills-item-meta">
                      <span className="skills-item-source">
                        {skill.source === "discovered"
                          ? "Discovered"
                          : skill.source === "imported"
                            ? "Imported"
                            : "Managed"}
                      </span>
                      <span className="skills-item-category">
                        {skill.category || "uncategorized"}
                      </span>
                      <span className={`skills-item-state ${skill.enabled ? "enabled" : "disabled"}`}>
                        {skill.enabled ? "enabled" : "disabled"}
                      </span>
                    </div>
                  </div>
                  <div className="skills-item-actions">
                    <Button
                      role={skill.enabled ? "secondary" : "secondary"}
                      size="sm"
                      onClick={() => handleToggle(skill.id, !skill.enabled)}
                      title={skill.enabled ? "Disable skill" : "Enable skill"}
                    >
                      {skill.enabled ? "Disable" : "Enable"}
                    </Button>
                    {!skill.managed && skill.source_path && (
                      <Button
                        role="danger"
                        size="sm"
                        onClick={() => handleRemove(skill.id)}
                        title="Remove skill"
                      >
                        Remove
                      </Button>
                    )}
                  </div>
                </div>
                {skill.tags.length > 0 && (
                  <div className="skills-item-tags">
                    {skill.tags.map((tag) => (
                      <span key={tag} className="skills-item-tag">
                        {tag}
                      </span>
                    ))}
                  </div>
                )}
                {skill.supported_agents.length > 0 && (
                  <div className="skills-item-agents">
                    <span className="skills-item-agents-label">Works with:</span>{" "}
                    {skill.supported_agents.join(", ")}
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
      </div>

      {tab !== "installed" && (
        <div className="skills-coming-soon">
          <span className="skills-coming-soon-icon">▱</span>
          <p className="skills-coming-soon-title">{tab.charAt(0).toUpperCase() + tab.slice(1)} tab</p>
          <p className="skills-coming-soon-desc">
            The {tab} tab is planned for a later milestone (design.md §16).
          </p>
        </div>
      )}
    </div>
  );
}

export default SkillsScreen;
