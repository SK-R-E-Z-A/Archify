import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import InstructionManager from "./InstructionManager";
import { invoke } from "@tauri-apps/api/core";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const mockInvoke = vi.mocked(invoke);

type InstructionRecord = {
  id: string;
  title: string;
  content: string;
  scope: "global" | "project";
  project_root: string | null;
  project_id: string | null;
  task_id: string | null;
  format: "markdown" | "plain_text" | "discovered_file";
  enabled: boolean;
  agent_targets: string[];
  priority: number;
  tags: string[];
  created_at: string;
  updated_at: string;
  source_path: string | null;
  managed: boolean;
  read_error: string | null;
};

function makeInstruction(overrides: Partial<InstructionRecord> = {}): InstructionRecord {
  return {
    id: "i1",
    title: "First",
    content: "Hello",
    scope: "global",
    project_root: null,
    project_id: null,
    task_id: null,
    format: "markdown",
    enabled: true,
    agent_targets: [],
    priority: 0,
    tags: [],
    created_at: "2024-01-01T00:00:00Z",
    updated_at: "2024-01-01T00:00:00Z",
    source_path: null,
    managed: true,
    read_error: null,
    ...overrides,
  };
}

describe("InstructionManager", () => {
  let listData: InstructionRecord[] = [];
  let listError: string | null = null;

  beforeEach(() => {
    listData = [];
    listError = null;
    mockInvoke.mockImplementation(async (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case "list_instructions":
          if (listError) throw new Error(listError);
          return [...listData];
        case "upsert_instruction": {
          const input = a.input as Record<string, unknown>;
          const id = (input.id as string | null) ?? `new-${listData.length + 1}`;
          const saved: InstructionRecord = makeInstruction({
            id,
            title: String(input.title ?? ""),
            content: String(input.content ?? ""),
            scope: (input.scope as "global" | "project") ?? "global",
            project_root: (input.project_root as string | null) ?? null,
            project_id: (input.project_id as string | null) ?? null,
            task_id: (input.task_id as string | null) ?? null,
            format: (input.format as InstructionRecord["format"]) ?? "markdown",
            enabled: (input.enabled as boolean) ?? true,
            agent_targets: (input.agent_targets as string[]) ?? [],
            priority: (input.priority as number) ?? 0,
            tags: (input.tags as string[]) ?? [],
            managed: true,
          });
          const idx = listData.findIndex((r) => r.id === saved.id);
          if (idx >= 0) listData[idx] = saved;
          else listData.push(saved);
          return saved;
        }
        case "set_instruction_enabled": {
          const id = a.id as string;
          const enabled = a.enabled as boolean;
          const rec = listData.find((r) => r.id === id);
          if (rec) rec.enabled = enabled;
          return rec ?? makeInstruction({ id, enabled });
        }
        case "delete_instruction": {
          const id = a.id as string;
          listData = listData.filter((r) => r.id !== id);
          return null;
        }
        case "preview_instruction_application": {
          const agent = a.agentKind as string;
          const applied = listData.filter(
            (r) =>
              r.enabled &&
              r.managed &&
              (r.agent_targets.length === 0 || r.agent_targets.includes(agent)),
          );
          return {
            refs: applied.map((r) => ({ id: r.id, title: r.title, scope: r.scope, source: "managed" })),
            text: applied.length ? `applied ${applied.length}` : "",
            warnings: [],
            truncated: false,
          };
        }
        default:
          throw new Error(`Unhandled command ${String(cmd)}`);
      }
    });
  });

  it("loads and displays instructions", async () => {
    listData = [
      makeInstruction({ id: "a", title: "Global one", priority: 2, agent_targets: ["planner"] }),
      makeInstruction({ id: "b", title: "Disabled one", enabled: false, scope: "project", project_root: "/tmp/proj" }),
    ];
    render(<InstructionManager projectRoot={null} projectId={null} />);

    expect(await screen.findByText("Global one")).toBeInTheDocument();
    expect(screen.getByText("Disabled one")).toBeInTheDocument();
    expect(screen.getByText(/Instructions \(2\)/)).toBeInTheDocument();
    expect(screen.getByText(/global · enabled · p2 · planner/)).toBeInTheDocument();
    expect(screen.getByText(/project · disabled · p0/)).toBeInTheDocument();
  });

  it("shows loading state while the initial list is pending", async () => {
    let resolveList: (value: unknown) => void = () => {};
    mockInvoke.mockImplementation((cmd) => {
      if (cmd === "list_instructions") {
        return new Promise((resolve) => {
          resolveList = resolve;
        });
      }
      return Promise.resolve(null);
    });

    render(<InstructionManager projectRoot={null} projectId={null} />);
    const btn = screen.getByRole("button", { name: /Loading…/i });
    expect(btn).toBeDisabled();

    resolveList([]);
    await waitFor(() => expect(screen.getByRole("button", { name: /Refresh/i })).not.toBeDisabled());
  });

  it("shows an error when loading fails", async () => {
    listError = "boom";
    render(<InstructionManager projectRoot={null} projectId={null} />);
    expect(await screen.findByText(/Error: boom/)).toBeInTheDocument();
  });

  it("validates required title (backend error surfaced)", async () => {
    mockInvoke.mockImplementation(async (cmd) => {
      if (cmd === "list_instructions") return [];
      if (cmd === "upsert_instruction") throw new Error("title is required");
      return null;
    });

    render(<InstructionManager projectRoot={null} projectId={null} />);
    await screen.findByText(/Instructions \(0\)/);

    fireEvent.click(screen.getByRole("button", { name: /^Create$/i }));
    expect(await screen.findByText(/Error: title is required/)).toBeInTheDocument();
    // No successful upsert should have happened.
    expect(mockInvoke).toHaveBeenCalledWith("upsert_instruction", expect.anything());
  });

  it("creates a managed instruction", async () => {
    render(<InstructionManager projectRoot={null} projectId={null} />);
    await screen.findByText(/Instructions \(0\)/);

    fireEvent.change(screen.getByLabelText(/^Title$/i), { target: { value: "Smoke" } });
    fireEvent.change(screen.getByLabelText(/^Content$/i), { target: { value: "body" } });
    fireEvent.change(screen.getByLabelText(/Priority/i), { target: { value: "5" } });
    fireEvent.change(screen.getByLabelText(/Agent targets/i), { target: { value: "planner, executor" } });
    fireEvent.change(screen.getByLabelText(/^Tags$/i), { target: { value: "a, b" } });

    fireEvent.click(screen.getByRole("button", { name: /^Create$/i }));

    await waitFor(() => expect(screen.getByText("Instruction created.")).toBeInTheDocument());
    expect(screen.getByText("Smoke")).toBeInTheDocument();
    expect(screen.getByText(/global · enabled · p5 · planner,executor/)).toBeInTheDocument();

    const upsertCall = mockInvoke.mock.calls.find((c) => c[0] === "upsert_instruction");
    expect(upsertCall).toBeTruthy();
    const input = (upsertCall![1] as { input: Record<string, unknown> }).input;
    expect(input.title).toBe("Smoke");
    expect(input.content).toBe("body");
    expect(input.scope).toBe("global");
    expect(input.project_root).toBeNull();
    expect(input.project_id).toBeNull();
    expect(input.agent_targets).toEqual(["planner", "executor"]);
    expect(input.tags).toEqual(["a", "b"]);
    expect(input.priority).toBe(5);
  });

  it("edits an existing instruction", async () => {
    listData = [makeInstruction({ id: "e1", title: "Editable", content: "old" })];
    render(<InstructionManager projectRoot={null} projectId={null} />);
    await screen.findByText("Editable");

    fireEvent.click(screen.getByRole("button", { name: /^Edit$/i }));
    const textarea = screen.getByDisplayValue("old");
    fireEvent.change(textarea, { target: { value: "new content" } });
    fireEvent.click(screen.getByRole("button", { name: /Save Changes/i }));

    await waitFor(() => expect(screen.getByText("Instruction updated.")).toBeInTheDocument());
    expect(screen.getByDisplayValue("new content")).toBeInTheDocument();

    const upsertCall = mockInvoke.mock.calls.find((c) => c[0] === "upsert_instruction");
    const input = (upsertCall![1] as { input: Record<string, unknown> }).input;
    expect(input.id).toBe("e1");
    expect(input.content).toBe("new content");
  });

  it("disables and re-enables an instruction", async () => {
    listData = [makeInstruction({ id: "t1", title: "Toggle me" })];
    render(<InstructionManager projectRoot={null} projectId={null} />);
    await screen.findByText("Toggle me");

    fireEvent.click(screen.getByRole("button", { name: /^Disable$/i }));
    await waitFor(() => expect(screen.getByText(/global · disabled · p0/)).toBeInTheDocument());
    expect(mockInvoke).toHaveBeenCalledWith("set_instruction_enabled", { id: "t1", enabled: false });

    fireEvent.click(screen.getByRole("button", { name: /^Enable$/i }));
    await waitFor(() => expect(screen.getByText(/global · enabled · p0/)).toBeInTheDocument());
    expect(mockInvoke).toHaveBeenCalledWith("set_instruction_enabled", { id: "t1", enabled: true });
  });

  it("deletes an instruction after confirmation", async () => {
    listData = [makeInstruction({ id: "d1", title: "Doomed" })];
    render(<InstructionManager projectRoot={null} projectId={null} />);
    await screen.findByText("Doomed");

    fireEvent.click(screen.getByRole("button", { name: /^Delete$/i }));
    expect(screen.getByText(/Delete this managed instruction\?/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Confirm Delete/i }));
    await waitFor(() => expect(screen.getByText("Instruction deleted.")).toBeInTheDocument());
    expect(screen.queryByText("Doomed")).not.toBeInTheDocument();
    expect(mockInvoke).toHaveBeenCalledWith("delete_instruction", { id: "d1" });
  });

  it("previews planner vs executor targeting", async () => {
    listData = [makeInstruction({ id: "p1", title: "Planner only", agent_targets: ["planner"], content: "p-body" })];
    render(<InstructionManager projectRoot={null} projectId={null} />);
    await screen.findByText("Planner only");

    // planner (default) -> applied
    fireEvent.click(screen.getByRole("button", { name: /Preview Apply/i }));
    expect(await screen.findByText(/Applied refs: Planner only/)).toBeInTheDocument();
    expect(screen.getByText(/applied 1/)).toBeInTheDocument();

    // switch to executor -> not applied
    const agentSelect = screen.getByTitle(/Agent stage to preview/i);
    fireEvent.change(agentSelect, { target: { value: "executor" } });
    fireEvent.click(screen.getByRole("button", { name: /Preview Apply/i }));
    expect(await screen.findByText(/Applied refs: \(none\)/)).toBeInTheDocument();
    expect(screen.getByText(/\(no managed instructions would be applied\)/)).toBeInTheDocument();

    expect(mockInvoke).toHaveBeenCalledWith(
      "preview_instruction_application",
      expect.objectContaining({ agentKind: "planner" }),
    );
    expect(mockInvoke).toHaveBeenCalledWith(
      "preview_instruction_application",
      expect.objectContaining({ agentKind: "executor" }),
    );
  });

  it("treats discovered files as read-only", async () => {
    listData = [makeInstruction({ id: "disc", title: "Discovered", managed: false })];
    render(<InstructionManager projectRoot={null} projectId={null} />);
    await screen.findByText("Discovered");

    expect(screen.getByText(/\(discovered\)/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Edit$/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Delete$/i })).not.toBeInTheDocument();

    fireEvent.click(screen.getByText("Discovered"));
    expect(screen.getByText(/Discovered files are read-only references/)).toBeInTheDocument();
  });

  it("clears preview when starting a new entry", async () => {
    listData = [makeInstruction({ id: "x", title: "X", content: "x-body" })];
    render(<InstructionManager projectRoot={null} projectId={null} />);
    await screen.findByText("X");

    fireEvent.click(screen.getByRole("button", { name: /Preview Apply/i }));
    expect(await screen.findByText(/Applied refs: X/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^New$/i }));
    expect(screen.queryByText(/Applied refs:/)).not.toBeInTheDocument();
  });
});
