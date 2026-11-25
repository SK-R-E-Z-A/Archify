import type { ReactNode } from "react";

export type AgentKind = "planner" | "executor" | "reviewer" | "verifier";

export type AgentStatus = "idle" | "running" | "completed" | "failed" | "stalled";

export type AgentCardProps = {
  kind: AgentKind;
  status: AgentStatus;
  label?: string;
  currentTask?: string | null;
  durationMs?: number | null;
  lastEvent?: string | null;
  children?: ReactNode;
};

const AGENT_LABELS: Record<AgentKind, string> = {
  planner: "Planner",
  executor: "Executor",
  reviewer: "Reviewer",
  verifier: "Verifier",
};

const AGENT_STATUS_LABELS: Record<AgentStatus, string> = {
  idle: "Idle",
  running: "Running",
  completed: "Completed",
  failed: "Failed",
  stalled: "Stalled",
};

function statusClass(status: AgentStatus): string {
  switch (status) {
    case "completed":
      return "agent-status completed";
    case "failed":
      return "agent-status failed";
    case "stalled":
      return "agent-status stalled";
    case "running":
      return "agent-status running";
    default:
      return "agent-status idle";
  }
}

function formatDuration(durationMs: number | null | undefined): string {
  if (durationMs == null) return "";
  if (durationMs < 1000) return `${durationMs} ms`;
  return `${(durationMs / 1000).toFixed(1)} s`;
}

export function AgentCard({
  kind,
  status,
  label,
  currentTask,
  durationMs,
  lastEvent,
  children,
}: AgentCardProps) {
  const name = label ?? AGENT_LABELS[kind];
  return (
    <div className="agent-card">
      <div className="agent-card-head">
        <span className="agent-name">{name}</span>
        <span className={statusClass(status)}>
          {AGENT_STATUS_LABELS[status]}
        </span>
      </div>
      {currentTask && (
        <div className="agent-current-task">
          <span className="agent-current-task-label">Task</span>
          <span className="agent-current-task-value" title={currentTask}>
            {currentTask}
          </span>
        </div>
      )}
      {durationMs != null && (
        <div className="agent-duration">
          <span className="agent-duration-label">Duration</span>
          <span className="agent-duration-value">{formatDuration(durationMs)}</span>
        </div>
      )}
      {lastEvent && (
        <div className="agent-last-event">
          <span className="agent-last-event-label">Last event</span>
          <span className="agent-last-event-value" title={lastEvent}>
            {lastEvent}
          </span>
        </div>
      )}
      {children && <div className="agent-card-body">{children}</div>}
    </div>
  );
}
