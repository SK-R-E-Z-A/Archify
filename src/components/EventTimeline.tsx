import type { ReactNode } from "react";

export type ExecutionPhase =
  | "task_created"
  | "task_queued"
  | "planning_started"
  | "plan_ready"
  | "planning_failed"
  | "plan_approved"
  | "plan_rejected"
  | "execution_started"
  | "task_cancelled"
  | "task_failed"
  | "iteration_started"
  | "executor_started"
  | "executor_finished"
  | "verification_started"
  | "verification_finished"
  | "review_started"
  | "review_finished"
  | "iteration_retry_scheduled"
  | "task_completed"
  | "component_started"
  | "component_finished"
  | "component_failed"
  | "component_verification_started"
  | "component_review_started"
  | "component_completed"
  | "component_retry_scheduled"
  | "task_verification_started"
  | string;

export interface ExecutionEvent {
  timestamp: string;
  task_id: string;
  phase: ExecutionPhase;
  data: Record<string, unknown>;
  iteration?: number;
}

export interface EventTimelineProps {
  events: ExecutionEvent[];
  compact?: boolean;
  className?: string;
}

const PHASE_LABELS: Record<string, string> = {
  task_created: "Task created",
  task_queued: "Task queued",
  planning_started: "Planning started",
  plan_ready: "Plan ready",
  planning_failed: "Planning failed",
  plan_approved: "Plan approved",
  plan_rejected: "Plan rejected",
  execution_started: "Execution started",
  task_cancelled: "Task cancelled",
  task_failed: "Task failed",
  iteration_started: "Iteration started",
  executor_started: "Executor started",
  executor_finished: "Executor finished",
  verification_started: "Verification started",
  verification_finished: "Verification finished",
  review_started: "Review started",
  review_finished: "Review finished",
  iteration_retry_scheduled: "Retry scheduled",
  task_completed: "Task completed",
  component_started: "Component started",
  component_finished: "Component finished",
  component_failed: "Component failed",
  component_verification_started: "Component verification started",
  component_review_started: "Component review started",
  component_completed: "Component completed",
  component_retry_scheduled: "Component retry scheduled",
  task_verification_started: "Task verification started",
};

function phaseLabel(phase: ExecutionPhase): string {
  return PHASE_LABELS[phase] ?? phase;
}

function phaseClass(phase: ExecutionPhase): string {
  const p = String(phase);
  if (p.includes("failed") || p.includes("cancelled")) return "event-phase failed";
  if (p.includes("completed")) return "event-phase completed";
  if (p.includes("started") || p.includes("running")) return "event-phase active";
  return "event-phase";
}

function formatEventData(data: Record<string, unknown>): ReactNode {
  const entries = Object.entries(data);
  if (entries.length === 0) return <span className="event-data-empty">no details</span>;

  const shown: string[] = [];
  for (const [key, value] of entries) {
    if (key === "task_id" || key === "iteration" || key === "timestamp") continue;
    let text: string;
    if (value === null) text = "null";
    else if (value === true) text = "true";
    else if (value === false) text = "false";
    else if (typeof value === "number") text = String(value);
    else if (typeof value === "string") {
      text = value.length > 60 ? `${value.slice(0, 60)}…` : value;
    } else {
      text = JSON.stringify(value);
    }
    shown.push(`${key}: ${text}`);
  }

  if (shown.length === 0) return <span className="event-data-empty">no details</span>;
  return <span className="event-data">{shown.join(" · ")}</span>;
}

function formatTimestamp(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return value;
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

export function EventTimeline({
  events,
  compact = false,
  className,
}: EventTimelineProps) {
  const ordered = [...events].sort(
    (a, b) => a.timestamp.localeCompare(b.timestamp),
  );

  if (ordered.length === 0) {
    return (
      <div className={`event-timeline ${compact ? "compact" : ""} ${className ?? ""}`.trim()}>
        <p className="event-timeline-empty">No execution events received yet.</p>
      </div>
    );
  }

  return (
    <div className={`event-timeline ${compact ? "compact" : ""} ${className ?? ""}`.trim()}>
      {ordered.map((event, index) => (
        <div key={`${event.task_id}-${event.timestamp}-${index}`} className="event-item">
          <div className="event-item-head">
            <span className="event-timestamp">{formatTimestamp(event.timestamp)}</span>
            <span className={phaseClass(event.phase)}>{phaseLabel(event.phase)}</span>
            {event.iteration !== undefined && (
              <span className="event-iteration">iteration {event.iteration}</span>
            )}
          </div>
          <div className="event-item-body">
            {formatEventData(event.data)}
          </div>
        </div>
      ))}
    </div>
  );
}
