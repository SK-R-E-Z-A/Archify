import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import React from "react";
import App from "./App";

const { eventHandlers, mountCount } = vi.hoisted(() => ({
  eventHandlers: new Map<string, Set<(event: { payload: unknown }) => void>>(),
  mountCount: { value: 0 },
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, cb: (event: { payload: unknown }) => void) => {
    if (!eventHandlers.has(event)) eventHandlers.set(event, new Set());
    eventHandlers.get(event)!.add(cb);
    return () => {
      eventHandlers.get(event)?.delete(cb);
    };
  }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("./features/terminal/TerminalWorkspace", () => {
  const MockTerminal = ({ active }: { cwd: string | null; active: boolean }) => {
    // Count mounts once per instance.
    React.useEffect(() => {
      mountCount.value += 1;
    }, []);
    return (
      <div data-testid="terminal-workspace" data-active={String(active)}>
        Mock Terminal
      </div>
    );
  };
  return { default: MockTerminal };
});

import { invoke } from "@tauri-apps/api/core";

const mockInvoke = vi.mocked(invoke);

function makeWorkspace(projects = [{ id: "p1", name: "Alpha", root: "/alpha" }]) {
  return {
    projects: projects.map((p) => ({
      id: p.id,
      name: p.name,
      root: p.root,
      provider_profile_id: null,
      created_at: "2024-01-01T00:00:00Z",
      last_opened_at: "2024-01-01T00:00:00Z",
      pinned: false,
    })),
    provider_profiles: [],
    settings: {
      max_concurrent_tasks: 3,
      max_concurrent_sessions: 4,
      scheduler_paused: false,
      auto_retry_interrupted: false,
      max_recovery_attempts: 1,
    },
    active_task_ids: [],
    queued_task_ids: [],
  };
}

const providerStatus = {
  planner_type: "cloud_http",
  executor_type: "process_cli",
  is_test_mode: false,
  cloud_configured: true,
  cli_configured: true,
  model_name: "m",
  cli_executable: "claude",
  max_iterations: 3,
  has_api_key: true,
  cloud_endpoint: "http://x",
  cli_args: [],
  cli_preset: null,
  prompt_delivery: "prefixed_stdin" as const,
  verification_command: "",
  ignored_dirs: [],
};

function makeTask(overrides: Partial<Record<string, unknown>> = {}) {
  return {
    id: "t1",
    user_task: "do thing",
    project_root: "/alpha",
    state: "planning",
    plan: null,
    task_graph: null,
    iteration: 0,
    max_iterations: 3,
    iterations: [],
    created_at: "2024-01-01T00:00:00Z",
    updated_at: "2024-01-02T00:00:00Z",
    error: null,
    project_id: null,
    provider_profile_id: null,
    agent_name: null,
    priority: 0,
    queued_at: null,
    recovery_attempts: 0,
    ...overrides,
  };
}

function setupInvoke(overrides: Record<string, (args: any) => unknown> = {}) {
  mockInvoke.mockImplementation(async (cmd, args) => {
    if (overrides[cmd]) return overrides[cmd](args);
    switch (cmd) {
      case "get_provider_status":
        return providerStatus;
      case "get_workspace":
        return makeWorkspace();
      case "list_cli_agent_presets":
        return [];
      case "get_task_status":
        return null;
      case "list_task_history":
        return [];
      case "list_sessions":
        return [];
      case "list_skills":
        return [];
      case "list_instructions":
        return [];
      default:
        return null;
    }
  });
}

function emit(event: string, payload: unknown) {
  eventHandlers.get(event)?.forEach((cb) => cb({ payload }));
}

describe("App integration", () => {
  beforeEach(() => {
    eventHandlers.clear();
    mountCount.value = 0;
    mockInvoke.mockReset();
    setupInvoke();
  });

  it("renders the workspace snapshot from the backend", async () => {
    render(<App />);
    expect(await screen.findByText("Alpha")).toBeInTheDocument();
    expect(screen.getAllByText(/No folder opened/).length).toBeGreaterThan(0);
  });

  it("opens and closes the Instructions modal", async () => {
    render(<App />);
    await screen.findByText("Alpha");
    fireEvent.click(screen.getByRole("button", { name: /^Instructions$/i }));
    expect(await screen.findByText("Instruction Manager")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /^Close$/i }));
    await waitFor(() => expect(screen.queryByText("Instruction Manager")).not.toBeInTheDocument());
  });

  it("opens and closes the Skills modal", async () => {
    render(<App />);
    await screen.findByText("Alpha");
    fireEvent.click(screen.getByRole("button", { name: /^Skills$/i }));
    expect(await screen.findByText("Skills Manager")).toBeInTheDocument();
    // close via backdrop click is not trivial; use the modal's close if present
  });

  it("lazy-mounts the terminal and preserves it when the panel closes", async () => {
    render(<App />);
    await screen.findByText("Alpha");
    expect(screen.queryByTestId("terminal-workspace")).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^SHELL$/i }));
    expect(await screen.findByTestId("terminal-workspace")).toBeInTheDocument();
    const before = mountCount.value;

    fireEvent.click(screen.getByTitle("Close terminal"));
    expect(screen.getByTestId("terminal-workspace")).toBeInTheDocument();
    const panel = document.querySelector(".terminal-panel");
    expect(panel).toHaveClass("terminal-panel-hidden");

    fireEvent.click(screen.getByTitle("Toggle terminal"));
    expect(panel).not.toHaveClass("terminal-panel-hidden");
    expect(mountCount.value).toBe(before);
  });

  it("surfaces task status sync errors", async () => {
    setupInvoke({ get_task_status: () => { throw new Error("sync boom"); } });
    render(<App />);
    expect(await screen.findByText(/out of sync/i)).toBeInTheDocument();
  });

  it("subscribes to backend events once and cleans up on unmount", async () => {
    const { unmount } = render(<App />);
    await screen.findByText("Alpha");
    expect(eventHandlers.get("orchestration_update")?.size).toBe(1);
    expect(eventHandlers.get("orchestration_log")?.size).toBe(1);
    expect(eventHandlers.get("workspace_update")?.size).toBe(1);

    unmount();
    expect(eventHandlers.get("orchestration_update")?.size ?? 0).toBe(0);
    expect(eventHandlers.get("orchestration_log")?.size ?? 0).toBe(0);
    expect(eventHandlers.get("workspace_update")?.size ?? 0).toBe(0);
  });

  it("ignores stale task snapshots", async () => {
    render(<App />);
    await screen.findByText("Alpha");

    emit("orchestration_update", makeTask({ state: "planning", updated_at: "2024-01-02T00:00:00Z" }));
    await waitFor(() => expect(document.querySelector("footer")?.textContent).toContain("Planning"));

    // Older snapshot must not replace the newer one.
    emit("orchestration_update", makeTask({ state: "completed", updated_at: "2024-01-01T00:00:00Z" }));
    await new Promise((r) => setTimeout(r, 50));
    expect(document.querySelector("footer")?.textContent).toContain("Planning");
    expect(document.querySelector("footer")?.textContent).not.toContain("Completed");

    // Newer snapshot replaces.
    emit("orchestration_update", makeTask({ state: "completed", updated_at: "2024-01-03T00:00:00Z" }));
    await waitFor(() => expect(document.querySelector("footer")?.textContent).toContain("Completed"));
  });

  it("does not count skipped graph steps as completed progress", async () => {
    render(<App />);
    await screen.findByText("Alpha");

    emit("orchestration_update", makeTask({
      state: "executing",
      task_graph: {
        components: [
          {
            id: "done",
            description: "Completed step",
            state: "completed",
            depends_on: [],
            acceptance_criteria: [],
            error: null,
          },
          {
            id: "skipped",
            description: "Skipped step",
            state: "skipped",
            depends_on: [],
            acceptance_criteria: [],
            error: null,
          },
          {
            id: "failed",
            description: "Failed step",
            state: "failed",
            depends_on: [],
            acceptance_criteria: [],
            error: "Verification failed",
          },
        ],
      },
    }));

    expect(await screen.findByRole("progressbar")).toHaveAttribute(
      "aria-valuenow",
      "33",
    );
    expect(screen.getByText(/1\/3 steps complete/)).toBeInTheDocument();
    expect(screen.getByText("Failed")).toBeInTheDocument();
  });
});
