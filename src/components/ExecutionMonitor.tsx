import { useMemo, useState } from "react";import { AgentCard, type AgentKind, type AgentStatus } from "./AgentCard";
import { EventTimeline, type ExecutionEvent } from "./EventTimeline";

export interface ExecutionMonitorProps {
  task: { id: string; state: string; user_task: string; updated_at: string; iterations: Array<{ iteration: number }> } | null;
  events: ExecutionEvent[];
  expanded?: boolean;
}

const AGENT_KINDS: AgentKind[] = ["planner", "executor", "reviewer", "verifier"];

const PHASE_TO_AGENT: Record<string, AgentKind> = {
  planning_started: "planner",
  plan_ready: "planner",
  planning_failed: "planner",
  execution_started: "executor",
  executor_started: "executor",
  executor_finished: "executor",
  verification_started: "verifier",
  verification_finished: "verifier",
  review_started: "reviewer",
  review_finished: "reviewer",
};

const PHASE_TO_STATUS: Record<string, AgentStatus> = {
  planning_started: "running",
  plan_ready: "completed",
  planning_failed: "failed",
  execution_started: "running",
  executor_started: "running",
  executor_finished: "completed",
  verification_started: "running",
  verification_finished: "completed",
  review_started: "running",
  review_finished: "completed",
};

function taskStateToAgentStatus(
  kind: AgentKind,
  taskState: string,
  lastPhaseForAgent: string | null,
): AgentStatus {
  if (taskState === "completed") return "completed";
  if (taskState === "failed") return "failed";
  if (taskState === "cancelled") return "stalled";

  switch (kind) {
    case "planner":
      if (taskState === "awaiting_approval") return "idle";
      if (taskState === "planning") return "running";
      break;
    case "executor":
      if (taskState === "executing" || taskState === "retrying") return "running";
      break;
    case "verifier":
      if (taskState === "verifying") return "running";
      break;
    case "reviewer":
      if (taskState === "reviewing") return "running";
      break;
  }

  if (lastPhaseForAgent) {
    const status = PHASE_TO_STATUS[lastPhaseForAgent];
    if (status) return status;
    if (lastPhaseForAgent.includes("failed")) return "failed";
  }

  return "idle";
}

function deriveAgentStatuses(
  task: ExecutionMonitorProps["task"],
  events: ExecutionEvent[],
): Map<AgentKind, { status: AgentStatus; lastEvent: string | null }> {
  const result = new Map<AgentKind, { status: AgentStatus; lastEvent: string | null }>();
  for (const kind of AGENT_KINDS) {
    result.set(kind, { status: "idle", lastEvent: null });
  }

  const ordered = [...events].sort((a, b) => a.timestamp.localeCompare(b.timestamp));
  const taskState = task?.state ?? "idle";

  for (const event of ordered) {
    const kind = PHASE_TO_AGENT[event.phase];
    if (!kind) continue;
    const status = PHASE_TO_STATUS[event.phase] ?? "idle";
    result.set(kind, { status, lastEvent: phaseLabel(event.phase) });
  }

  for (const kind of AGENT_KINDS) {
    const current = result.get(kind)!;
    const lastEventForKind = ordered
      .filter((e) => PHASE_TO_AGENT[e.phase] === kind)
      .pop();
    result.set(kind, {
      status: taskStateToAgentStatus(kind, taskState, lastEventForKind?.phase ?? null),
      lastEvent: current.lastEvent,
    });
  }

  return result;
}

function phaseLabel(phase: string): string {
  const labels: Record<string, string> = {
    planning_started: "Planning started",
    plan_ready: "Plan ready",
    planning_failed: "Planning failed",
    execution_started: "Execution started",
    executor_started: "Executor started",
    executor_finished: "Executor finished",
    verification_started: "Verification started",
    verification_finished: "Verification finished",
    review_started: "Review started",
    review_finished: "Review finished",
  };
  return labels[phase] ?? phase;
}

function formatDuration(durationMs: number): string {
  if (durationMs < 1000) return `${durationMs} ms`;
  return `${(durationMs / 1000).toFixed(1)} s`;
}

function retryCount(events: ExecutionEvent[]): number {
  return events.filter((e) => e.phase === "iteration_retry_scheduled").length;
}

export function ExecutionMonitor({ task, events, expanded: initialExpanded = false }: ExecutionMonitorProps) {
  const [expanded, setExpanded] = useState(initialExpanded);
  const agentStatuses = useMemo(() => deriveAgentStatuses(task, events), [task, events]);
  const retries = useMemo(() => retryCount(events), [events]);

  const startedAt = task ? new Date(task.updated_at).getTime() : 0;
  const now = Date.now();
  const elapsedMs = startedAt > 0 ? now - startedAt : 0;

  const terminalStates = ["completed", "failed", "cancelled"];
  const isTerminal = task ? terminalStates.includes(task.state) : false;

  return (
    <section className="execution-monitor">
      <div className="execution-monitor-head">
        <span className="execution-monitor-title">Execution Monitor</span>
        <button
          type="button"
          className="execution-monitor-toggle"
          onClick={() => setExpanded((prev) => !prev)}
          aria-expanded={expanded}
        >
          {expanded ? "▾" : "▸"}
        </button>
      </div>

      {!expanded && (
        <p className="execution-monitor-empty">
          Expand the monitor to view live agent activity and execution events.
        </p>
      )}

      {expanded && (
        <>
          <div className="execution-monitor-body">
            <div className="agent-grid">
              {AGENT_KINDS.map((kind) => {
                const current = agentStatuses.get(kind)!;
                const taskLabel =
                  task?.user_task && current.status !== "idle"
                    ? task.user_task.length > 40
                      ? `${task.user_task.slice(0, 40)}…`
                      : task.user_task
                    : null;
                return (
                  <AgentCard
                    key={kind}
                    kind={kind}
                    status={current.status}
                    currentTask={taskLabel}
                    durationMs={current.status === "running" ? elapsedMs : null}
                    lastEvent={current.lastEvent}
                  />
                );
              })}
            </div>

            <div className="execution-meta">
              {task && (
                <div className="execution-meta-item">
                  <span className="execution-meta-label">Task</span>
                  <span className="execution-meta-value">{task.user_task}</span>
                </div>
              )}
              <div className="execution-meta-item">
                <span className="execution-meta-label">Stage</span>
                <span className="execution-meta-value">
                  {task ? task.state.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase()) : "—"}
                </span>
              </div>
              {!isTerminal && task && elapsedMs > 0 && (
                <div className="execution-meta-item execution-meta-timer">
                  <span className="execution-meta-label">Elapsed</span>
                  <span className="execution-meta-value">{formatDuration(elapsedMs)}</span>
                </div>
              )}
              {retries > 0 && (
                <div className="execution-meta-item execution-meta-retries">
                  <span className="execution-meta-label">Recovery attempts</span>
                  <span className="execution-meta-value">{retries}</span>
                </div>
              )}
            </div>

            <div className="event-timeline-section">
              <h3 className="event-timeline-section-title">Execution events</h3>
              <EventTimeline events={events} compact />
            </div>
          </div>
        </>
      )}
    </section>
  );
}
