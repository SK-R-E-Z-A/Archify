import { useState } from "react";
import { Button } from "../components/Button";

type AutomationTemplate = {
  id: string;
  name: string;
  description: string;
  category: string;
  trigger_type: string;
  actions: string[];
};

interface AutomationBuilderScreenProps {
}

/**
 * Automation Builder screen — design.md §18.5.
 *
 * Visual workflow builder for creating automations. Templates and a
 * drag-and-drop canvas. Includes a live graph (Stage 4) that renders the
 * trigger → action → completion flow for the selected template.
 * Every button uses the token-driven Button component.
 */
export function AutomationBuilderScreen({}: AutomationBuilderScreenProps) {
  const [selectedTemplate, setSelectedTemplate] = useState<string | null>(null);

  const templates: AutomationTemplate[] = [
    {
      id: "daily-deps",
      name: "Daily Dependency Check",
      description: "Checks for outdated dependencies every morning.",
      category: "Maintenance",
      trigger_type: "cron",
      actions: ["Check package updates", "Compare versions", "Report outdated"],
    },
    {
      id: "pre-commit-review",
      name: "Pre-Commit Review",
      description: "Runs the review agent on every commit.",
      category: "Review",
      trigger_type: "event",
      actions: ["Analyze diff", "Run review agent", "Post comments"],
    },
    {
      id: "nightly-tests",
      name: "Nightly Test Suite",
      description: "Runs the full test suite nightly.",
      category: "Testing",
      trigger_type: "cron",
      actions: ["Install dependencies", "Run test suite", "Publish results"],
    },
    {
      id: "deploy-on-merge",
      name: "Deploy on Merge",
      description: "Deploys to staging when a PR is merged.",
      category: "Deploy",
      trigger_type: "event",
      actions: ["Build artifacts", "Run smoke tests", "Deploy to staging"],
    },
  ];

  const template = selectedTemplate
    ? templates.find((t) => t.id === selectedTemplate)
    : undefined;

  return (
    <div className="automation-builder-screen" role="main" aria-label="Automation Builder">
      <header className="automation-builder-head">
        <div>
          <h2 className="automation-builder-title">Automation Builder</h2>
          <p className="automation-builder-sub">
            Build automation workflows with triggers and actions.
          </p>
        </div>
        <div className="automation-builder-actions">
          <Button
            role="primary"
            size="sm"
            onClick={() => {
              // Create automation is wired to a later milestone
            }}
            title="Create automation"
          >
            Create Automation
          </Button>
        </div>
      </header>

      <div className="automation-builder-panel">
        <div className="automation-builder-templates">
          <h3 className="automation-builder-section-title">Templates</h3>
          <div className="automation-builder-template-grid">
            {templates.map((template) => (
              <div
                key={template.id}
                className={`automation-builder-template ${selectedTemplate === template.id ? "selected" : ""}`}
                onClick={() => setSelectedTemplate(template.id)}
              >
                <h4 className="automation-builder-template-name">{template.name}</h4>
                <span className="automation-builder-template-category">{template.category}</span>
                <p className="automation-builder-template-desc">{template.description}</p>
                <span className="automation-builder-template-trigger">
                  Trigger: {template.trigger_type}
                </span>
              </div>
            ))}
          </div>
        </div>

        <div className="automation-builder-canvas">
          {selectedTemplate && template ? (
            <>
              <h3 className="automation-builder-section-title">
                Configure: {template.name}
              </h3>
              <p className="automation-builder-canvas-hint">
                Live graph preview of the workflow. Click nodes to configure.
              </p>
              <div className="automation-builder-live-graph">
                <div className="automation-builder-graph-label">Live Graph (Stage 4)</div>
                <svg
                  className="automation-builder-graph-svg"
                  width="100%"
                  height="200"
                  role="img"
                  aria-label={`Workflow graph for ${template.name}`}
                >
                  <defs>
                    <marker
                      id="arrowhead"
                      markerWidth="10"
                      markerHeight="7"
                      refX="9"
                      refY="3.5"
                      orient="auto"
                    >
                      <polygon points="0 0, 10 3.5, 0 7" fill="#c9c0be" />
                    </marker>
                  </defs>
                  {/* Trigger node */}
                  <g className="automation-builder-graph-node">
                    <circle
                      cx="60"
                      cy="100"
                      r="24"
                      className="automation-builder-graph-node-circle trigger"
                    />
                    <text
                      x="60"
                      y="100"
                      className="automation-builder-graph-node-label"
                      textAnchor="middle"
                      dy="0.35em"
                    >
                      Trigger
                    </text>
                    <text
                      x="60"
                      y="130"
                      className="automation-builder-graph-node-sub"
                      textAnchor="middle"
                    >
                      {template.trigger_type}
                    </text>
                  </g>
                  {/* Edges */}
                  {template.actions.map((_action, i) => {
                    const x1 = 84;
                    const y1 = 100;
                    const x2 = 220 - 24;
                    const y2 = 100;
                    return (
                      <line
                        key={`edge-${i}`}
                        x1={x1}
                        y1={y1}
                        x2={x2}
                        y2={y2}
                        className="automation-builder-graph-edge"
                        markerEnd="url(#arrowhead)"
                      />
                    );
                  })}
                  {/* Action nodes */}
                  {template.actions.map((action, i) => {
                    const cx = 220;
                    const cy = 60 + i * 40;
                    return (
                      <g key={`action-${i}`} className="automation-builder-graph-node">
                        <circle
                          cx={cx}
                          cy={cy}
                          r="24"
                          className="automation-builder-graph-node-circle action"
                        />
                        <text
                          x={cx}
                          y={cy}
                          className="automation-builder-graph-node-label"
                          textAnchor="middle"
                          dy="0.35em"
                        >
                          {action.length > 12 ? action.slice(0, 12) + "…" : action}
                        </text>
                      </g>
                    );
                  })}
                  {/* Completion node */}
                  <g className="automation-builder-graph-node">
                    <circle
                      cx="380"
                      cy="100"
                      r="24"
                      className="automation-builder-graph-node-circle complete"
                    />
                    <text
                      x="380"
                      y="100"
                      className="automation-builder-graph-node-label"
                      textAnchor="middle"
                      dy="0.35em"
                    >
                      Complete
                    </text>
                  </g>
                  {/* Edges from last action to complete */}
                  {template.actions.length > 0 && (
                    <line
                      x1={244}
                      y1={60 + (template.actions.length - 1) * 40}
                      x2={356}
                      y2={100}
                      className="automation-builder-graph-edge"
                      markerEnd="url(#arrowhead)"
                    />
                  )}
                  {/* Edges from trigger to first action */}
                  {template.actions.length > 0 && (
                    <line
                      x1={84}
                      y1={100}
                      x2={196}
                      y2={60}
                      className="automation-builder-graph-edge"
                      markerEnd="url(#arrowhead)"
                    />
                  )}
                </svg>
                <div className="automation-builder-graph-legend">
                  <span className="automation-builder-legend-item trigger">
                    ● Trigger
                  </span>
                  <span className="automation-builder-legend-item action">
                    ● Action
                  </span>
                  <span className="automation-builder-legend-item complete">
                    ● Complete
                  </span>
                </div>
              </div>
              <div className="automation-builder-canvas-actions">
                <Button
                  role="secondary"
                  size="sm"
                  onClick={() => setSelectedTemplate(null)}
                  title="Clear selection"
                >
                  Clear
                </Button>
                <Button
                  role="primary"
                  size="sm"
                  onClick={() => {
                    // Save automation is wired to a later milestone
                  }}
                  title="Save automation"
                >
                  Save Automation
                </Button>
              </div>
            </>
          ) : (
            <div className="automation-builder-canvas-empty">
              <span className="automation-builder-canvas-icon">▱</span>
              <p>Select a template to view its live graph</p>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

export default AutomationBuilderScreen;
