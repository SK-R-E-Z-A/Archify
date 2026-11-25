import {
  useEffect,
  useMemo,
  useState,
} from "react";

export type SidebarState = "expanded" | "compact" | "hidden";

export type SidebarScreenId =
  | "overview"
  | "skills"
  | "plugins"
  | "automation"
  | "projects"
  | "activity"
  | "notification"
  | "global-agent-console"
  | "recovery-center"
  | "workspace-manager"
  | "archive"
  | "trash"
  | "settings-menu"
  | "project-playground"
  | "project-tasks"
  | "project-conversations"
  | "project-agents"
  | "project-map"
  | "project-activity"
  | "project-reports"
  | "project-settings";

export type ProjectRecord = {
  id: string;
  name: string;
  root: string;
  provider_profile_id: string | null;
  created_at: string;
  last_opened_at: string;
  pinned: boolean;
  archived?: boolean;
};

type SidebarProps = {
  projects: ProjectRecord[];
  activeProjectId: string | null;
  screen: string;
  setScreen: (screenId: SidebarScreenId) => void;
  switchProject: (project: ProjectRecord) => void;
  sidebarState: SidebarState;
  setSidebarState: (state: SidebarState) => void;
  onOpenProjectFolder: () => void;
  unreadNotifications: number;
};

const SIDEBAR_STATE_KEY = "archify:sidebar-state";

const GLOBAL_NAV: { id: SidebarScreenId; label: string; icon: string }[] = [
  { id: "overview", label: "Overview", icon: "⌂" },
  { id: "skills", label: "Skills", icon: "🧩" },
  { id: "plugins", label: "Plugins", icon: "⚡" },
  { id: "automation", label: "Automation", icon: "↻" },
  { id: "projects", label: "Projects", icon: "▱" },
];

const PROJECT_NAV: { id: SidebarScreenId; label: string; icon: string }[] = [
  { id: "project-playground", label: "Playground", icon: "✳" },
  { id: "project-tasks", label: "Tasks", icon: "☑" },
  { id: "project-conversations", label: "Conversations", icon: "💬" },
  { id: "project-agents", label: "Agent Console", icon: "◐" },
  { id: "project-map", label: "Project Map", icon: "🗺" },
  { id: "project-activity", label: "Activity", icon: "📊" },
  { id: "project-reports", label: "Reports", icon: "📄" },
  { id: "project-settings", label: "Project Settings", icon: "⚙" },
];

const SECONDARY_GLOBAL_NAV: { id: SidebarScreenId; label: string; icon: string }[] = [
  { id: "activity", label: "Activity", icon: "📊" },
  { id: "notification", label: "Notifications", icon: "🔔" },
  { id: "global-agent-console", label: "Agent Console", icon: "◐" },
  { id: "recovery-center", label: "Recovery Center", icon: "🩹" },
  { id: "workspace-manager", label: "Workspace Manager", icon: "🏢" },
  { id: "archive", label: "Archive", icon: "📦" },
  { id: "trash", label: "Trash", icon: "🗑" },
];

const PROJECT_MENU_ACTIONS: {
  id: string;
  label: string;
  icon: string;
  destructive?: boolean;
}[] = [
  { id: "open", label: "Open", icon: "▶" },
  { id: "newTask", label: "New Task", icon: "✳" },
  { id: "rename", label: "Rename", icon: "✏" },
  { id: "pin", label: "Pin", icon: "★" },
  { id: "duplicate", label: "Duplicate", icon: "📋" },
  { id: "settings", label: "Project Settings", icon: "⚙" },
  { id: "archive", label: "Archive", icon: "📦" },
  { id: "remove", label: "Remove", icon: "🗑", destructive: true },
];

/**
 * Sidebar — primary navigation system for Archify.
 *
 * Supports three states: expanded (icons + labels), compact (icons only),
 * and hidden (sidebar collapsed, workspace uses full width).
 *
 * Global navigation is ordered per design.md §4.3. Settings is anchored at
 * the bottom (§4.3). Project navigation supports two distinct interactions
 * (§4.4): clicking the project opens it; clicking the chevron expands/collapses
 * project-specific navigation.
 */
export function Sidebar({
  projects,
  activeProjectId,
  screen,
  setScreen,
  switchProject,
  sidebarState,
  setSidebarState,
  onOpenProjectFolder,
  unreadNotifications,
}: SidebarProps) {
  const [expandedProjectId, setExpandedProjectId] = useState<string | null>(
    null,
  );
  const [menuOpen, setMenuOpen] = useState<{ projectId: string } | null>(
    null,
  );
  const [mounted, setMounted] = useState(false);

  // Persist sidebar state
  useEffect(() => {
    setMounted(true);
    try {
      const saved = localStorage.getItem(SIDEBAR_STATE_KEY) as
        | SidebarState
        | null;
      if (saved === "expanded" || saved === "compact" || saved === "hidden") {
        setSidebarState(saved);
      }
    } catch {
      /* localStorage unavailable — keep default */
    }
  }, [setSidebarState]);

  useEffect(() => {
    if (!mounted) return;
    try {
      localStorage.setItem(SIDEBAR_STATE_KEY, sidebarState);
    } catch {
      /* ignore */
    }
  }, [sidebarState, mounted]);

  const visibleProjects = projects.filter((p) => !p.archived);

  const isScreenActive = (id: string) => screen === id;

  const handleProjectClick = (project: ProjectRecord) => {
    setMenuOpen(null);
    switchProject(project);
    setScreen("projects");
  };

  const handleProjectScreenClick = (id: SidebarScreenId) => {
    setMenuOpen(null);
    setScreen(id);
  };

  const handleGlobalNavClick = (id: SidebarScreenId) => {
    setMenuOpen(null);
    setScreen(id);
  };

  const handleSecondaryNavClick = (id: SidebarScreenId) => {
    setMenuOpen(null);
    setScreen(id);
  };

  const handleToggleProjectExpand = (projectId: string) => {
    setMenuOpen(null);
    setExpandedProjectId(
      expandedProjectId === projectId ? null : projectId,
    );
  };

  const handleMenuAction = (projectId: string, actionId: string) => {
    setMenuOpen(null);
    switch (actionId) {
      case "open": {
        const p = projects.find((x) => x.id === projectId);
        if (p) {
          switchProject(p);
          setScreen("projects");
        }
        break;
      }
      case "newTask":
        setScreen("project-tasks");
        break;
      case "rename":
        setScreen("project-settings");
        break;
      case "pin": {
        // Pinning is a missing feature (§7.2). Honest placeholder.
        break;
      }
      case "duplicate":
        // Duplication is a missing feature (§7.2). Honest placeholder.
        break;
      case "settings":
        setScreen("project-settings");
        break;
      case "archive":
        setScreen("archive");
        break;
      case "remove": {
        const p = projects.find((x) => x.id === projectId);
        if (
          p &&
          window.confirm(
            `Remove "${p.name}" from the workspace? This cannot be undone.`,
          )
        ) {
          // Remove is a missing feature (§28.1). Honest placeholder.
        }
        break;
      }
      default:
        break;
    }
  };

  const isProjectExpanded = (id: string) => expandedProjectId === id;
  const isMenuOpenFor = (id: string) => menuOpen?.projectId === id;

  // Compute sidebar width class based on state
  const sidebarClasses = useMemo(() => {
    const base = "sidebar";
    switch (sidebarState) {
      case "expanded":
        return `${base} sidebar-expanded`;
      case "compact":
        return `${base} sidebar-compact`;
      case "hidden":
        return `${base} sidebar-hidden`;
      default:
        return base;
    }
  }, [sidebarState]);

  if (sidebarState === "hidden") {
    return (
      <aside className={sidebarClasses} aria-hidden="true">
        <button
          className="sidebar-toggle"
          title="Show sidebar"
          aria-label="Show sidebar"
          onClick={() => setSidebarState("expanded")}
        >
          ▷
        </button>
      </aside>
    );
  }

  return (
    <aside className={sidebarClasses}>
      {/* Sidebar toggle (visible in compact/hidden transitions) */}
      {sidebarState === "compact" && (
        <button
          className="sidebar-toggle"
          title="Expand sidebar"
          aria-label="Expand sidebar"
          onClick={() => setSidebarState("expanded")}
        >
          ◁
        </button>
      )}

      <div className="sidebar-scroll">
        {/* Brand */}
        <div className="sidebar-brand">
          <div className="sidebar-brand-mark">P</div>
          <span className={sidebarState === "expanded" ? "sidebar-brand-label" : "sidebar-brand-label-hidden"}>
            Archify
          </span>
        </div>

        {/* Global Navigation */}
        <nav className="sidebar-nav-section">
          <div className="sidebar-section-label">GLOBAL</div>
          {GLOBAL_NAV.map((item) => {
            const active = isScreenActive(item.id);
            return (
              <button
                key={item.id}
                type="button"
                className={`sidebar-nav-item ${active ? "sidebar-nav-item-active" : ""}`}
                onClick={() => handleGlobalNavClick(item.id)}
                title={item.label}
                aria-current={active ? "page" : undefined}
              >
                <span className="sidebar-nav-icon">{item.icon}</span>
                <span
                  className={sidebarState === "expanded" ? "sidebar-nav-label" : "sidebar-nav-label-hidden"}
                >
                  {item.label}
                </span>
                {active && <span className="sidebar-nav-indicator" />}
              </button>
            );
          })}
        </nav>

        {/* Secondary global destinations */}
        {sidebarState === "expanded" && (
          <nav className="sidebar-nav-section">
            <div className="sidebar-section-label">GLOBAL DESTINATIONS</div>
            {SECONDARY_GLOBAL_NAV.map((item) => {
              const active = isScreenActive(item.id);
              const unread =
                item.id === "notification" && unreadNotifications > 0;
              return (
                <button
                  key={item.id}
                  type="button"
                  className={`sidebar-nav-item ${active ? "sidebar-nav-item-active" : ""} ${unread ? "sidebar-nav-item-unread" : ""}`}
                  onClick={() => handleSecondaryNavClick(item.id)}
                  title={item.label}
                  aria-current={active ? "page" : undefined}
                >
                  <span className="sidebar-nav-icon">{item.icon}</span>
                  <span className="sidebar-nav-label">{item.label}</span>
                  {unread && (
                    <span className="sidebar-nav-unread-badge">
                      {unreadNotifications}
                    </span>
                  )}
                </button>
              );
            })}
          </nav>
        )}

        {/* Projects */}
        {visibleProjects.length > 0 && (
          <nav className="sidebar-nav-section">
            <div
              className="sidebar-section-label sidebar-section-label-clickable"
              onClick={() => {
                setMenuOpen(null);
                setScreen("projects");
              }}
              title="Projects"
            >
              Projects
              <span className="sidebar-project-count">
                {visibleProjects.length}
              </span>
            </div>
            <div className="sidebar-project-list">
              {visibleProjects.map((project) => {
                const expanded = isProjectExpanded(project.id);
                return (
                  <div
                    key={project.id}
                    className="sidebar-project-item-wrapper"
                  >
                    <div
                      className={`sidebar-project-item ${
                        project.id === activeProjectId
                          ? "sidebar-project-item-active"
                          : ""
                      } ${project.pinned ? "sidebar-project-item-pinned" : ""}`}
                    >
                      <button
                        type="button"
                        className="sidebar-project-open"
                        onClick={() => handleProjectClick(project)}
                        title={project.name}
                      >
                        <span className="sidebar-project-icon">
                          {project.pinned ? "★" : "▱"}
                        </span>
                        <span
                          className={
                            sidebarState === "expanded"
                              ? "sidebar-project-name"
                              : "sidebar-project-name-hidden"
                          }
                        >
                          {project.name}
                        </span>
                        <span className="sidebar-project-indicator">
                          {project.provider_profile_id ? "●" : "○"}
                        </span>
                      </button>
                      <button
                        type="button"
                        className="sidebar-project-chevron"
                        title={
                          expanded
                            ? "Collapse project navigation"
                            : "Expand project navigation"
                        }
                        onClick={(e) => {
                          e.stopPropagation();
                          handleToggleProjectExpand(project.id);
                        }}
                        aria-expanded={expanded}
                        aria-label="Toggle project navigation"
                      >
                        {expanded ? "▾" : "▸"}
                      </button>
                      <button
                        type="button"
                        className="sidebar-project-menu"
                        title="More options"
                        onClick={(e) => {
                          e.stopPropagation();
                          setMenuOpen(
                            isMenuOpenFor(project.id) ? null : { projectId: project.id },
                          );
                        }}
                        aria-haspopup="menu"
                        aria-label={`More options for ${project.name}`}
                      >
                        ⁝
                      </button>
                    </div>

                    {expanded && (
                      <div className="sidebar-project-nav">
                        {PROJECT_NAV.map((navItem) => {
                          const active = isScreenActive(navItem.id);
                          return (
                            <button
                              key={navItem.id}
                              type="button"
                              className={`sidebar-project-nav-item ${
                                active ? "sidebar-project-nav-item-active" : ""
                              }`}
                              onClick={() => handleProjectScreenClick(navItem.id)}
                              title={navItem.label}
                              aria-current={active ? "page" : undefined}
                            >
                              <span className="sidebar-nav-icon">
                                {navItem.icon}
                              </span>
                              <span
                                className={
                                  sidebarState === "expanded"
                                    ? "sidebar-nav-label"
                                    : "sidebar-nav-label-hidden"
                                }
                              >
                                {navItem.label}
                              </span>
                            </button>
                          );
                        })}
                      </div>
                    )}

                    {isMenuOpenFor(project.id) && (
                      <div className="sidebar-project-menu-panel">
                        {PROJECT_MENU_ACTIONS.map((action) => (
                          <button
                            key={action.id}
                            type="button"
                            className={`sidebar-menu-item ${
                              action.destructive
                                ? "sidebar-menu-item-destructive"
                                : ""
                            }`}
                            onClick={() => handleMenuAction(project.id, action.id)}
                            title={action.label}
                          >
                            <span className="sidebar-menu-icon">
                              {action.icon}
                            </span>
                            <span className="sidebar-menu-label">
                              {action.label}
                            </span>
                          </button>
                        ))}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </nav>
        )}

        {/* Settings - anchored at bottom */}
        <div className="sidebar-spacer" />
        <nav className="sidebar-nav-section sidebar-nav-section-bottom">
          <button
            type="button"
            className={`sidebar-nav-item ${
              isScreenActive("settings-menu") ? "sidebar-nav-item-active" : ""
            }`}
            onClick={() => {
              setMenuOpen(null);
              setScreen("settings-menu");
            }}
            title="Settings"
            aria-current={isScreenActive("settings-menu") ? "page" : undefined}
          >
            <span className="sidebar-nav-icon">⚙</span>
            <span
              className={
                sidebarState === "expanded"
                  ? "sidebar-nav-label"
                  : "sidebar-nav-label-hidden"
              }
            >
              Settings
            </span>
          </button>
        </nav>

        <div className="sidebar-footer">
          <button
            type="button"
            className="sidebar-add-project"
            onClick={onOpenProjectFolder}
            title="Add project"
          >
            ＋ Add project
          </button>
          <button
            type="button"
            className="sidebar-toggle-state"
            title={`Switch to ${
              sidebarState === "expanded"
                ? "compact"
                : sidebarState === "compact"
                  ? "hidden"
                  : "expanded"
            }`}
            onClick={() => {
              if (sidebarState === "expanded") {
                setSidebarState("compact");
              } else if (sidebarState === "compact") {
                setSidebarState("hidden");
              } else {
                setSidebarState("expanded");
              }
            }}
          >
            {sidebarState === "expanded"
              ? "≡"
              : sidebarState === "compact"
                ? "◷"
                : "⌂"}
          </button>
        </div>
      </div>
    </aside>
  );
}

export default Sidebar;
