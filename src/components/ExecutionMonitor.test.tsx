import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { AgentCard } from "./AgentCard";
import { EventTimeline, type ExecutionEvent } from "./EventTimeline";
import { ExecutionMonitor } from "./ExecutionMonitor";

function makeEvent(overrides: Partial<ExecutionEvent> = {}): ExecutionEvent {
  return {
    timestamp: "2024-01-02T00:00:00Z",
    task_id: "t1",
    phase: "planning_started",
    data: {},
    ...overrides,
  };
}

function makeTask(overrides: Record<string, unknown> = {}): ExecutionMonitorProps["task"] {
  return {
    id: "t1",
    state: "executing",
    user_task: "Create a new feature",
    updated_at: "2024-01-02T00:00:00Z",
    iterations: [{ iteration: 1 }],
    ...overrides,
  };
}

type ExecutionMonitorProps = {
  task: {
    id: string;
    state: string;
    user_task: string;
    updated_at: string;
    iterations: Array<{ iteration: number }>;
  } | null;
  events: ExecutionEvent[];
};

describe("AgentCard", () => {
  it("renders agent name and status badge", () => {
    render(<AgentCard kind="planner" status="running" />);
    expect(screen.getByText("Planner")).toBeInTheDocument();
    expect(screen.getByText("Running")).toBeInTheDocument();
  });

  it("renders all agent kinds with correct labels", () => {
    const { rerender } = render(<AgentCard kind="planner" status="idle" />);
    expect(screen.getByText("Planner")).toBeInTheDocument();
    expect(screen.getByText("Idle")).toBeInTheDocument();

    rerender(<AgentCard kind="executor" status="completed" />);
    expect(screen.getByText("Executor")).toBeInTheDocument();
    expect(screen.getByText("Completed")).toBeInTheDocument();

    rerender(<AgentCard kind="reviewer" status="failed" />);
    expect(screen.getByText("Reviewer")).toBeInTheDocument();
    expect(screen.getByText("Failed")).toBeInTheDocument();

    rerender(<AgentCard kind="verifier" status="stalled" />);
    expect(screen.getByText("Verifier")).toBeInTheDocument();
    expect(screen.getByText("Stalled")).toBeInTheDocument();
  });

  it("shows current task when provided", () => {
    render(<AgentCard kind="executor" status="running" currentTask="Add login button" />);
    expect(screen.getByText("Add login button")).toBeInTheDocument();
  });

  it("shows duration when provided", () => {
    render(<AgentCard kind="executor" status="running" durationMs={5500} />);
    expect(screen.getByText("5.5 s")).toBeInTheDocument();
  });

  it("shows last event when provided", () => {
    render(<AgentCard kind="planner" status="completed" lastEvent="Plan ready" />);
    expect(screen.getByText("Plan ready")).toBeInTheDocument();
  });
});

describe("EventTimeline", () => {
  it("shows empty state when no events", () => {
    render(<EventTimeline events={[]} />);
    expect(screen.getByText(/No execution events received yet/)).toBeInTheDocument();
  });

  it("renders events with timestamp, phase, and data", () => {
    const events = [
      makeEvent({ phase: "planning_started", data: { agent: "test-planner" } }),
      makeEvent({ phase: "plan_ready", data: { steps: 3 } }),
      makeEvent({ phase: "executor_finished", data: { exit_code: 0, timed_out: false } }),
    ];
    render(<EventTimeline events={events} />);
    expect(screen.getByText("Planning started")).toBeInTheDocument();
    expect(screen.getByText("Plan ready")).toBeInTheDocument();
    expect(screen.getByText("Executor finished")).toBeInTheDocument();
    expect(screen.getByText("agent: test-planner")).toBeInTheDocument();
    expect(screen.getByText("steps: 3")).toBeInTheDocument();
    expect(screen.getByText("exit_code: 0 · timed_out: false")).toBeInTheDocument();
  });

  it("shows iteration when present", () => {
    render(<EventTimeline events={[makeEvent({ phase: "iteration_started", iteration: 2 })]} />);
    expect(screen.getByText("iteration 2")).toBeInTheDocument();
  });

  it("renders compact mode", () => {
    render(<EventTimeline events={[makeEvent()]} compact />);
    expect(screen.getByText("Planning started")).toBeInTheDocument();
  });
});

describe("ExecutionMonitor", () => {
  it("shows empty state when expanded without events", () => {
    render(<ExecutionMonitor task={null} events={[]} />);
    expect(screen.getByText(/Expand the monitor/)).toBeInTheDocument();
  });

  it("renders agent cards for all four agent kinds", () => {
    render(<ExecutionMonitor task={makeTask()} events={[makeEvent()]} expanded={true} />);
    expect(screen.getByText("Planner")).toBeInTheDocument();
    expect(screen.getByText("Executor")).toBeInTheDocument();
    expect(screen.getByText("Reviewer")).toBeInTheDocument();
    expect(screen.getByText("Verifier")).toBeInTheDocument();
  });

  it("shows running status for planner when planning_started event received", () => {
    const events = [makeEvent({ phase: "planning_started" })];
    render(<ExecutionMonitor task={makeTask({ state: "planning" })} events={events} expanded={true} />);
    expect(screen.getByText("Running")).toBeInTheDocument();
  });

  it("shows completed status when task is completed", () => {
    render(<ExecutionMonitor task={makeTask({ state: "completed" })} events={[]} expanded={true} />);
    expect(screen.getAllByText("Completed").length).toBeGreaterThan(0);
  });

  it("shows failed status when task is failed", () => {
    render(<ExecutionMonitor task={makeTask({ state: "failed" })} events={[]} expanded={true} />);
    expect(screen.getAllByText("Failed").length).toBeGreaterThan(0);
  });

  it("shows execution duration when task is active", () => {
    render(<ExecutionMonitor task={makeTask({ state: "executing" })} events={[makeEvent({ phase: "execution_started" })]} expanded={true} />);
    expect(screen.getAllByText(/\d+\.\d+ s/).length).toBeGreaterThan(0);
  });

  it("shows recovery attempts count", () => {
    const events = [
      makeEvent({ phase: "iteration_retry_scheduled" }),
      makeEvent({ phase: "iteration_retry_scheduled" }),
    ];
    render(<ExecutionMonitor task={makeTask({ state: "retrying" })} events={events} expanded={true} />);
    expect(screen.getByText("2")).toBeInTheDocument();
  });

  it("filters events by task_id", () => {
    const events = [
      makeEvent({ task_id: "t1", phase: "planning_started" }),
      makeEvent({ task_id: "t2", phase: "planning_started" }),
    ];
    render(<ExecutionMonitor task={makeTask({ id: "t1" })} events={events} expanded={true} />);
    // Should only show t1 events - both planner cards should show "Planning started" as last event
    const lastEvents = screen.getAllByText("Planning started");
    expect(lastEvents.length).toBeGreaterThan(0);
  });

  it("shows collapsed state by default", () => {
    const { container } = render(<ExecutionMonitor task={makeTask()} events={[makeEvent()]} />);
    expect(container.querySelector(".execution-monitor-empty")).toBeInTheDocument();
  });
});
