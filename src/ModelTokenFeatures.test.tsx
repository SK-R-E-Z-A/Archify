import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import App from "./App";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("./features/terminal/TerminalWorkspace", () => ({
  default: () => <div data-testid="terminal-workspace">Mock Terminal</div>,
}));

import { invoke } from "@tauri-apps/api/core";
const mockInvoke = vi.mocked(invoke);

function makeWorkspace() {
  return {
    projects: [{ id: "p1", name: "Alpha", root: "/alpha", provider_profile_id: null, created_at: "2024-01-01T00:00:00Z", last_opened_at: "2024-01-01T00:00:00Z", pinned: false }],
    provider_profiles: [],
    settings: { max_concurrent_tasks: 3, max_concurrent_sessions: 4, scheduler_paused: false, auto_retry_interrupted: false, max_recovery_attempts: 1 },
    active_task_ids: [],
    queued_task_ids: [],
  };
}

const providerStatus = {
  planner_type: "cloud_http", executor_type: "process_cli", is_test_mode: false, cloud_configured: true, cli_configured: true,
  model_name: "m", cli_executable: "claude", max_iterations: 3, has_api_key: true, cloud_endpoint: "http://x", cli_args: [],
  cli_preset: null, prompt_delivery: "prefixed_stdin" as const, verification_command: "", ignored_dirs: [],
};

const models = [
  { id: "gpt-4o", name: "GPT-4o", provider_id: "openai", provider_name: "OpenAI", available: true, deprecated: false, capabilities: ["chat", "planning"] },
  { id: "old-model", name: "Old Model", provider_id: "openai", provider_name: "OpenAI", available: true, deprecated: true, capabilities: [] },
  { id: "gone-model", name: "Gone Model", provider_id: "x", provider_name: "X", available: false, deprecated: false, capabilities: [] },
];

const prefs = {
  default_model_id: "gpt-4o",
  responsibility_preferences: { planning: { model_id: "old-model", auto_select: false } },
  manual_overrides: {},
  auto_selection_enabled: true,
};

const usageStats = {
  total_input_tokens: 120, total_output_tokens: 30, total_tokens: 150, request_count: 2, unknown_request_count: 1,
  by_provider: { openai: { input_tokens: 100, output_tokens: 20, total_tokens: 120, request_count: 1 } },
  by_model: { "gpt-4o": { input_tokens: 100, output_tokens: 20, total_tokens: 120, request_count: 1 } },
  by_project: { p1: { input_tokens: 100, output_tokens: 20, total_tokens: 120, request_count: 1 } },
  by_task: { "task-12345678": { input_tokens: 100, output_tokens: 20, total_tokens: 120, request_count: 1 } },
  by_responsibility: { chat: { input_tokens: 100, output_tokens: 20, total_tokens: 120, request_count: 1 } },
  by_date: {},
};

const usageRecords = [
  {
    id: "r1", operation_type: "chat", is_retry: false, count_towards_totals: true, created_at: "2024-01-01T00:00:00Z",
    context: { provider_id: "openai", provider_name: "OpenAI", model_id: "gpt-4o", model_name: "GPT-4o", project_id: "p1", project_name: "Alpha", task_id: "task-12345678", session_id: null, responsibility: "chat" },
    usage: { input_tokens: 100, output_tokens: 20, total_tokens: 120, tokens_unknown: false, timestamp: "2024-01-01T00:00:00Z", duration_ms: null },
  },
  {
    id: "r2", operation_type: "plan", is_retry: true, count_towards_totals: false, created_at: "2024-01-01T00:01:00Z",
    context: { provider_id: null, provider_name: null, model_id: null, model_name: null, project_id: null, project_name: null, task_id: null, session_id: null, responsibility: null },
    usage: { input_tokens: null, output_tokens: null, total_tokens: null, tokens_unknown: true, timestamp: "2024-01-01T00:01:00Z", duration_ms: null },
  },
];

function setup(overrides: Record<string, (args: any) => unknown> = {}) {
  mockInvoke.mockImplementation(async (cmd, args: any) => {
    if (overrides[cmd]) return overrides[cmd](args);
    switch (cmd) {
      case "get_provider_status": return providerStatus;
      case "get_workspace": return makeWorkspace();
      case "list_cli_agent_presets": return [];
      case "get_task_status": return null;
      case "list_task_history": return [];
      case "list_sessions": return [];
      case "list_skills": return [];
      case "list_instructions": return [];
      case "list_available_models": return models;
      case "get_model_preferences": return prefs;
      case "get_model_selection": return { model_id: "gpt-4o", model_name: "GPT-4o", provider_id: "openai", provider_name: "OpenAI", responsibility: "default", strategy: "default_model", auto_selected: false };
      case "update_model_preferences": return args?.preferences ?? prefs;
      case "get_token_usage_stats": return usageStats;
      case "get_token_usage_records": return usageRecords;
      case "clear_token_usage": return null;
      default: return null;
    }
  });
}

describe("Model preferences and token usage UI", () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    setup();
    vi.spyOn(window, "confirm").mockReturnValue(true);
  });

  it("loads token usage from backend and distinguishes unknown/missing values", async () => {
    render(<App />);
    await screen.findByText("Alpha");
    fireEvent.click(screen.getByTitle("Token Usage"));
    expect(await screen.findByText("Token Usage & Analytics")).toBeInTheDocument();
    expect((await screen.findAllByText("150")).length).toBeGreaterThan(0);
    expect(await screen.findByText(/Unknown Token Requests/i)).toBeInTheDocument();
    expect(await screen.findByText("Token counts unknown")).toBeInTheDocument();
    expect(screen.getAllByText(/provider unknown|model unknown|project missing|task missing/i).length).toBeGreaterThan(0);
    expect(mockInvoke).toHaveBeenCalledWith("get_token_usage_stats");
    expect(mockInvoke).toHaveBeenCalledWith("get_token_usage_records", expect.any(Object));
  });

  it("shows token usage load errors and supports backend-confirmed clear", async () => {
    setup({ get_token_usage_stats: () => { throw new Error("usage boom"); } });
    render(<App />);
    await screen.findByText("Alpha");
    fireEvent.click(screen.getByTitle("Token Usage"));
    expect(await screen.findByRole("alert")).toHaveTextContent("usage boom");

    setup(); // recover
    const modal = screen.getByText("Token Usage & Analytics").closest(".config-modal") as HTMLElement;
    fireEvent.click(within(modal).getByRole("button", { name: /^Refresh$/i }));
    await screen.findAllByText("150");
    fireEvent.click(screen.getByRole("button", { name: /Clear Usage/i }));
    await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith("clear_token_usage"));
    expect(await screen.findByRole("status")).toHaveTextContent(/backend-confirmed/i);
  });

  it("loads model preferences from backend and saves backend-returned preferences", async () => {
    let saved: unknown = null;
    setup({
      update_model_preferences: (args) => {
        saved = (args as Record<string, unknown>)["preferences"];
        const p = (args?.preferences ?? {}) as object; return { ...p, default_model_id: "gpt-4o" };
      },
    });
    render(<App />);
    await screen.findByText("Alpha");
    fireEvent.click(screen.getByRole("button", { name: /^Models$/i }));
    expect(await screen.findByText("Model Preferences")).toBeInTheDocument();
    expect(await screen.findByText(/backend selection preview/i)).toBeInTheDocument();
    expect(screen.getAllByText(/GPT-4o/).length).toBeGreaterThan(0);
    // Deprecated/unavailable warnings are visible (old-model selected for planning)
    expect((await screen.findAllByText(/deprecated/i)).length).toBeGreaterThan(0);
    expect((await screen.findAllByText(/unavailable/i)).length).toBeGreaterThan(0);
    expect(screen.getByText(/project-level overrides.*not exposed/i)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Save Preferences/i }));
    await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith("update_model_preferences", expect.any(Object)));
    expect(saved).toBeTruthy();
    expect(await screen.findByRole("status")).toHaveTextContent(/backend-confirmed/i);
  });

  it("shows model preference load/save errors without implying execution", async () => {
    setup({ get_model_preferences: () => { throw new Error("prefs boom"); } });
    render(<App />);
    await screen.findByText("Alpha");
    fireEvent.click(screen.getByRole("button", { name: /^Models$/i }));
    expect(await screen.findByRole("alert")).toHaveTextContent("prefs boom");
    expect(screen.queryByText(/now using/i)).not.toBeInTheDocument();
  });
});
