import { useMemo, useState } from "react";
import { Button } from "../components/Button";

type AutomationStatus = "idle" | "running" | "completed" | "failed" | "paused";

type AutomationRecord = {
  id: string;
  name: string;
  description: string;
  trigger: string;
  status: AutomationStatus;
  last_run: string | null;
  next_run: string | null;
  project_id: string | null;
  tags: string[];
};

interface AutomationScreenProps {
}

/**
 * Automation workspace screen — design.md §18.
 *
 * Automation workspace with All Automations / Templates / Runs / Schedules /
 * Integrations tabs. Supports enable/disable and run actions. Every button
 * uses the token-driven Button component and respects theme / density /
 * font-size presets.
 */
export function AutomationScreen({}: AutomationScreenProps) {
  const [automations, setAutomations] = useState<AutomationRecord[]>([
    {
      id: "auto-1",
      name: "Daily Dependency Check",
      description: "Checks for outdated dependencies every morning at 9am.",
      trigger: "cron: 0 9 * * *",
      status: "idle",
      last_run: "2026-11-21T09:00:00Z",
      next_run: "2026-11-22T09:00:00Z",
      project_id: null,
      tags: ["dependencies", "cron"],
    },
    {
      id: "auto-2",
      name: "Pre-Commit Review",
      description: "Runs the review agent on every commit before push.",
      trigger: "event: pre-commit",
      status: "running",
      last_run: "2026-11-22T14:30:00Z",
      next_run: null,
      project_id: null,
      tags: ["review", "commit"],
    },
    {
      id: "auto-3",
      name: "Nightly Test Suite",
      description: "Runs the full test suite nightly and reports failures.",
      trigger: "cron: 0 2 * * *",
      status: "paused",
      last_run: "2026-11-21T02:00:00Z",
      next_run: null,
      project_id: null,
      tags: ["testing", "cron"],
    },
    {
      id: "auto-4",
      name: "Deploy on Merge",
      description: "Deploys to staging when a PR is merged to main.",
      trigger: "event: pull_request.merged",
      status: "idle",
      last_run: "2026-11-20T16:45:00Z",
      next_run: null,
      project_id: null,
      tags: ["deploy", "event"],
    },
  ]);
  const [loading, setLoading] = useState(false);
  const [tab, setTab] = useState<"all" | "templates" | "runs" | "schedules" | "integrations">(
    "all",
  );
  const [search, setSearch] = useState("");

  const handleToggle = (id: string, status: AutomationStatus) => {
    setAutomations((prev) =>
      prev.map((a) =>
        a.id === id
          ? { ...a, status: status === "running" ? "idle" : "running" }
          : a,
      ),
    );
  };

  const handleRun = (id: string) => {
    setAutomations((prev) =>
      prev.map((a) => (a.id === id ? { ...a, status: "running" } : a)),
    );
  };

  const filtered = useMemo(() => {
    return automations.filter((a) => {
      const matchesSearch =
        a.name.toLowerCase().includes(search.toLowerCase()) ||
        a.description.toLowerCase().includes(search.toLowerCase()) ||
        a.tags.some((t) => t.toLowerCase().includes(search.toLowerCase()));
      if (tab === "all") return matchesSearch;
      return matchesSearch;
    });
  }, [automations, search, tab]);

  if (loading) {
    return (
      <div className="library-screen" role="main" aria-label="Automation">
        <header className="library-head">
          <div>
            <h2 className="library-title">Automation</h2>
            <p className="library-sub">
              Automate repetitive tasks across your projects.
            </p>
          </div>
        </header>
        <div className="library-empty">
          <span className="library-empty-icon">⟳</span>
          <p className="library-empty-title">Loading automations…</p>
        </div>
      </div>
    );
  }

  return (
    <div className="library-screen" role="main" aria-label="Automation">
      <header className="library-head">
        <div>
          <h2 className="library-title">Automation</h2>
          <p className="library-sub">
            Automate repetitive tasks across your projects.
          </p>
        </div>
        <div className="library-actions">
          <Button
            role="secondary"
            size="sm"
            onClick={() => void setLoading(true)}
            title="Refresh automations"
          >
            Refresh
          </Button>
        </div>
      </header>

      {/* Tabs */}
      <div className="library-tabs">
        {[
          { id: "all" as const, label: "All Automations" },
          { id: "templates" as const, label: "Templates" },
          { id: "runs" as const, label: "Runs" },
          { id: "schedules" as const, label: "Schedules" },
          { id: "integrations" as const, label: "Integrations" },
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

      {tab !== "all" && (
        <div className="library-coming-soon">
          <span className="library-coming-soon-icon">▱</span>
          <p className="library-coming-soon-title">
            {tab.charAt(0).toUpperCase() + tab.slice(1)} tab
          </p>
          <p className="library-coming-soon-desc">
            The {tab} tab is planned for a later milestone (design.md §18).
          </p>
        </div>
      )}

      {tab === "all" && (
        <>
          {/* Search */}
          <div className="library-filters">
            <div className="library-search">
              <input
                type="text"
                placeholder="Search name, description, tag…"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
                disabled={loading}
                className="library-search-input"
              />
            </div>
          </div>

          {/* Automations list */}
          <div className="library-list-wrap">
            {filtered.length === 0 && (
              <div className="library-empty">
                <span className="library-empty-icon">▱</span>
                <p className="library-empty-title">No automations</p>
                <p className="library-empty-desc">
                  No automations match the current filters.
                </p>
              </div>
            )}
            {filtered.length > 0 && (
              <div className="library-list">
                {filtered.map((automation) => (
                  <div
                    key={automation.id}
                    className={`library-item ${automation.status}`}
                  >
                    <div className="library-item-head">
                      <div className="library-item-info">
                        <h3 className="library-item-name">
                          {automation.name}
                        </h3>
                        <p className="library-item-desc">
                          {automation.description}
                        </p>
                        <div className="library-item-meta">
                          <span className="library-item-trigger">
                            {automation.trigger}
                          </span>
                          <span className="library-item-status">
                            {automation.status}
                          </span>
                          {automation.last_run && (
                            <span className="library-item-last-run">
                              Last run: {new Date(automation.last_run).toLocaleDateString()}
                            </span>
                          )}
                          {automation.next_run && (
                            <span className="library-item-next-run">
                              Next run: {new Date(automation.next_run).toLocaleDateString()}
                            </span>
                          )}
                        </div>
                      </div>
                      <div className="library-item-actions">
                        <Button
                          role="secondary"
                          size="sm"
                          onClick={() => handleToggle(automation.id, automation.status)}
                          title={
                            automation.status === "running"
                              ? "Stop automation"
                              : "Start automation"
                          }
                        >
                          {automation.status === "running" ? "Stop" : "Start"}
                        </Button>
                        <Button
                          role="secondary"
                          size="sm"
                          onClick={() => handleRun(automation.id)}
                          title="Run automation now"
                        >
                          Run
                        </Button>
                      </div>
                    </div>
                    {automation.tags.length > 0 && (
                      <div className="library-item-tags">
                        {automation.tags.map((tag) => (
                          <span key={tag} className="library-item-tag">
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

export default AutomationScreen;
