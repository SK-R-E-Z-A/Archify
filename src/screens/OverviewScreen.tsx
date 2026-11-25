import { useCallback, useEffect, useMemo, useState } from "react";
import { Button } from "../components/Button";

type WidgetKind =
  | "projects"
  | "tasks"
  | "agents"
  | "automations"
  | "reports"
  | "notifications";

type WidgetSpan = "standard" | "wide";

type WidgetLayout = {
  id: WidgetKind;
  span: WidgetSpan;
};

interface Widget {
  id: WidgetKind;
  title: string;
  value: string;
  subtitle: string;
  action: string;
  icon: string;
}

interface OverviewScreenProps {
  projects: { id: string; name: string }[];
  onOpenProject: (id: string) => void;
  onNewTask: () => void;
  onOpenAgents: () => void;
  onOpenAutomation: () => void;
  onOpenReports: () => void;
  onOpenNotifications: () => void;
}

const LAYOUT_KEY = "archify:overview-layout";

const DEFAULT_LAYOUT: WidgetLayout[] = [
  { id: "projects", span: "standard" },
  { id: "tasks", span: "standard" },
  { id: "agents", span: "standard" },
  { id: "automations", span: "standard" },
  { id: "reports", span: "standard" },
  { id: "notifications", span: "standard" },
];

function loadLayout(): WidgetLayout[] {
  try {
    const raw = localStorage.getItem(LAYOUT_KEY);
    if (!raw) return DEFAULT_LAYOUT;
    const parsed = JSON.parse(raw) as WidgetLayout[];
    // Keep only known widget ids, then append any missing defaults.
    const known = new Set(DEFAULT_LAYOUT.map((w) => w.id));
    const kept = parsed.filter((w) => known.has(w.id) && (w.span === "standard" || w.span === "wide"));
    const present = new Set(kept.map((w) => w.id));
    return [...kept, ...DEFAULT_LAYOUT.filter((w) => !present.has(w.id))];
  } catch {
    return DEFAULT_LAYOUT;
  }
}

function saveLayout(layout: WidgetLayout[]) {
  try {
    localStorage.setItem(LAYOUT_KEY, JSON.stringify(layout));
  } catch {
    /* localStorage unavailable — layout stays session-only */
  }
}

/**
 * Overview dashboard — design.md §5.
 * Luxury minimalism command center. Widgets are draggable (native HTML5 DnD)
 * and resizable (standard/wide span); the arrangement persists per viewer.
 */
export function OverviewScreen({
  projects,
  onOpenProject,
  onNewTask,
  onOpenAgents,
  onOpenAutomation,
  onOpenReports,
  onOpenNotifications,
}: OverviewScreenProps) {
  const [layout, setLayout] = useState<WidgetLayout[]>(loadLayout);
  const [dragId, setDragId] = useState<WidgetKind | null>(null);
  const [dropTarget, setDropTarget] = useState<WidgetKind | null>(null);

  useEffect(() => {
    saveLayout(layout);
  }, [layout]);

  const widgets: Widget[] = useMemo(
    () => [
      {
        id: "projects",
        title: "Projects",
        value: String(projects.length).padStart(2, "0"),
        subtitle: projects.length === 0 ? "No active projects" : "Total repositories",
        action: projects.length ? "View projects" : "Add project",
        icon: "▱",
      },
      {
        id: "tasks",
        title: "Active tasks",
        value: "00",
        subtitle: "Orchestrator idle",
        action: "New task",
        icon: "✳",
      },
      {
        id: "agents",
        title: "Agents",
        value: "00",
        subtitle: "All systems ready",
        action: "Manage agents",
        icon: "◐",
      },
      {
        id: "automations",
        title: "Automations",
        value: "00",
        subtitle: "No scheduled runs",
        action: "Browse templates",
        icon: "↻",
      },
      {
        id: "reports",
        title: "Reports",
        value: "00",
        subtitle: "Analytics overview",
        action: "View reports",
        icon: "▤",
      },
      {
        id: "notifications",
        title: "Notifications",
        value: "00",
        subtitle: "System status",
        action: "Open center",
        icon: "◈",
      },
    ],
    [projects.length],
  );

  const widgetById = useMemo(
    () => new Map(widgets.map((w) => [w.id, w])),
    [widgets],
  );

  const handleWidgetAction = useCallback(
    (id: WidgetKind) => {
      switch (id) {
        case "tasks":
          onNewTask();
          break;
        case "agents":
          onOpenAgents();
          break;
        case "automations":
          onOpenAutomation();
          break;
        case "reports":
          onOpenReports();
          break;
        case "notifications":
          onOpenNotifications();
          break;
        default:
          break;
      }
    },
    [onNewTask, onOpenAgents, onOpenAutomation, onOpenReports, onOpenNotifications],
  );

  /** Move the dragged widget to the drop target's position. */
  function moveWidget(sourceId: WidgetKind, targetId: WidgetKind) {
    if (sourceId === targetId) return;
    setLayout((prev) => {
      const from = prev.findIndex((w) => w.id === sourceId);
      const to = prev.findIndex((w) => w.id === targetId);
      if (from < 0 || to < 0) return prev;
      const next = [...prev];
      const [moved] = next.splice(from, 1);
      next.splice(to, 0, moved);
      return next;
    });
  }

  function toggleSpan(id: WidgetKind) {
    setLayout((prev) =>
      prev.map((w) =>
        w.id === id
          ? { ...w, span: w.span === "wide" ? "standard" : "wide" }
          : w,
      ),
    );
  }

  function onOpenProjectPlaceholder() {
    // Placeholder: project creation dialog is a missing backend feature.
  }

  return (
    <div className="overview-screen" role="main" aria-label="Overview dashboard">
      <header className="overview-head">
        <div className="overview-head-left">
          <span className="eyebrow">COMMAND CENTER</span>
          <h2 className="overview-title">Overview</h2>
          <p className="overview-sub">Welcome back. Your workspace at a glance.</p>
        </div>
        <div className="overview-actions">
          <Button role="primary" onClick={onNewTask} title="Create a new task">
            <span className="icon">✳</span> New task
          </Button>
          <Button role="secondary" onClick={onOpenProjectPlaceholder}>
            + Add project
          </Button>
        </div>
      </header>

      <div className="overview-grid">
        {layout.map((slot) => {
          const widget = widgetById.get(slot.id);
          if (!widget) return null;
          const isDragOver = dropTarget === widget.id && dragId !== widget.id;
          return (
            <section
              key={widget.id}
              className={[
                "overview-widget",
                `overview-widget-${widget.id}`,
                slot.span === "wide" ? "overview-widget-wide" : "",
                dragId === widget.id ? "overview-widget-dragging" : "",
                isDragOver ? "overview-widget-drop-target" : "",
              ]
                .filter(Boolean)
                .join(" ")}
              draggable
              onDragStart={(e) => {
                setDragId(widget.id);
                e.dataTransfer.effectAllowed = "move";
                // Firefox requires data to be set for drag to start.
                e.dataTransfer.setData("text/plain", widget.id);
              }}
              onDragOver={(e) => {
                e.preventDefault();
                e.dataTransfer.dropEffect = "move";
                if (dropTarget !== widget.id) setDropTarget(widget.id);
              }}
              onDragLeave={() => {
                if (dropTarget === widget.id) setDropTarget(null);
              }}
              onDrop={(e) => {
                e.preventDefault();
                if (dragId) moveWidget(dragId, widget.id);
                setDragId(null);
                setDropTarget(null);
              }}
              onDragEnd={() => {
                setDragId(null);
                setDropTarget(null);
              }}
            >
              <div className="overview-widget-head">
                <div className="overview-widget-icon">{widget.icon}</div>
                <div className="overview-widget-controls">
                  <button
                    type="button"
                    className="overview-widget-control"
                    title={slot.span === "wide" ? "Shrink widget" : "Widen widget"}
                    aria-label={slot.span === "wide" ? "Shrink widget" : "Widen widget"}
                    onClick={() => toggleSpan(widget.id)}
                  >
                    {slot.span === "wide" ? "⤡" : "⤢"}
                  </button>
                  <span className="overview-widget-grip" aria-hidden="true" title="Drag to reorder">
                    ⋮⋮
                  </span>
                </div>
              </div>
              <span className="eyebrow">{widget.title}</span>
              <div className="overview-widget-value">{widget.value}</div>
              <div className="overview-widget-sub">{widget.subtitle}</div>
              <div className="overview-widget-body">
                {widget.id === "projects" && projects.length > 0 ? (
                  <div className="overview-project-list">
                    {projects.slice(0, 3).map((project) => (
                      <button
                        key={project.id}
                        type="button"
                        className="overview-project-item"
                        onClick={() => onOpenProject(project.id)}
                      >
                        <span className="indicator" />
                        {project.name}
                      </button>
                    ))}
                  </div>
                ) : null}
                <Button
                  role="ghost"
                  size="sm"
                  block
                  onClick={() =>
                    widget.id === "projects" && projects.length === 0
                      ? onOpenProjectPlaceholder()
                      : handleWidgetAction(widget.id)
                  }
                >
                  {widget.action}
                  <span className="arrow">→</span>
                </Button>
              </div>
            </section>
          );
        })}
      </div>

      <div className="overview-grid-secondary">
        <section className="activity-panel">
          <div className="panel-heading">
            <span>RECENT ACTIVITY</span>
            <button className="text-button" type="button">
              View all
            </button>
          </div>
          <div className="empty-state-small">
            <span className="icon">↻</span>
            <p>No recent activity in your workspace.</p>
          </div>
        </section>
        <section className="quick-access-panel">
          <div className="panel-heading">
            <span>PINNED PROJECTS</span>
            <button
              className="text-button"
              type="button"
              onClick={() => setLayout(DEFAULT_LAYOUT)}
              title="Restore the default widget arrangement"
            >
              Reset layout
            </button>
          </div>
          {projects.length === 0 ? (
            <div className="empty-state-small">
              <span className="icon">▱</span>
              <p>No pinned projects.</p>
            </div>
          ) : (
            <div className="pinned-list">
              {projects.map((p) => (
                <button
                  key={p.id}
                  className="pinned-item"
                  type="button"
                  onClick={() => onOpenProject(p.id)}
                >
                  <span>▱</span> {p.name}
                </button>
              ))}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

export default OverviewScreen;