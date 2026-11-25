import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import TerminalWorkspace from "./TerminalWorkspace";

const {
  terminalInstances,
  listenHandlers,
  listenReject,
  lastSpawnedId,
  historyFactory,
} = vi.hoisted(() => ({
  terminalInstances: [] as Array<{
    written: string[];
    onDataCb: ((d: string) => void) | null;
    host: HTMLElement | null;
  }>,
  listenHandlers: new Map<string, Set<(e: { payload: unknown }) => void>>(),
  listenReject: { value: false },
  lastSpawnedId: { value: "" },
  historyFactory: { value: null as ((id: string) => unknown) | null },
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, cb: (e: { payload: unknown }) => void) => {
    if (listenReject.value) throw new Error("listen boom");
    if (!listenHandlers.has(event)) listenHandlers.set(event, new Set());
    listenHandlers.get(event)!.add(cb);
    return () => {
      listenHandlers.get(event)?.delete(cb);
    };
  }),
}));
vi.mock("@xterm/xterm", () => {
  class Terminal {
    options: { disableStdin: boolean } = { disableStdin: true };
    written: string[] = [];
    onDataCb: ((d: string) => void) | null = null;
    onResizeCb: ((s: { cols: number; rows: number }) => void) | null = null;
    host: HTMLElement | null = null;
    open(host: HTMLElement) {
      this.host = host;
    }
    write(data: string | Uint8Array) {
      this.written.push(
        typeof data === "string" ? data : new TextDecoder().decode(data),
      );
    }
    onData(cb: (d: string) => void) {
      this.onDataCb = cb;
      return { dispose: () => (this.onDataCb = null) };
    }
    onResize(cb: (s: { cols: number; rows: number }) => void) {
      this.onResizeCb = cb;
      return { dispose: () => (this.onResizeCb = null) };
    }
    loadAddon() {}
    focus() {}
    dispose() {}
    constructor() {
      terminalInstances.push(this as unknown as (typeof terminalInstances)[number]);
    }
  }
  return { Terminal };
});
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: class {
    fit() {}
  },
}));

import { invoke } from "@tauri-apps/api/core";

const mockInvoke = vi.mocked(invoke);

function b64(s: string) {
  return btoa(unescape(encodeURIComponent(s)));
}

function emit(event: string, payload: unknown) {
  listenHandlers.get(event)?.forEach((cb) => cb({ payload }));
}

const defaultHistory = (_id: string) => ({ data: b64("PS> "), seq: 1 });

beforeEach(() => {
  terminalInstances.length = 0;
  listenHandlers.clear();
  listenReject.value = false;
  lastSpawnedId.value = "";
  historyFactory.value = null;
  mockInvoke.mockReset();
  mockInvoke.mockImplementation(async (cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "terminal_spawn":
        lastSpawnedId.value = String(a.id);
        return { id: a.id, shell: "powershell.exe", cwd: "C:\\work", pid: 42 };
      case "terminal_history": {
        const fn = historyFactory.value ?? defaultHistory;
        const r = fn(String(a.id));
        if (r instanceof Error) throw r;
        if (r instanceof Promise) return r;
        return r;
      }
      case "terminal_write":
      case "terminal_resize":
      case "terminal_close":
        return undefined;
      default:
        throw new Error(`Unhandled ${String(cmd)}`);
    }
  });
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("TerminalWorkspace", () => {
  it("bootstraps an initial running terminal", async () => {
    render(<TerminalWorkspace cwd={null} active={true} />);
    expect(await screen.findByText(/Terminal 1 · powershell\.exe/)).toBeInTheDocument();
    await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith("terminal_spawn", expect.objectContaining({ cols: 80, rows: 24 })));
    await waitFor(() => expect(terminalInstances[0].written.join("")).toContain("PS> "));
    expect(document.querySelector(".terminal-status-dot")).toHaveClass("running");
  });

  it("creates and switches between tabs", async () => {
    render(<TerminalWorkspace cwd="/work" active={true} />);
    await screen.findByText(/Terminal 1/);
    fireEvent.click(screen.getByRole("button", { name: /New terminal/i }));
    await screen.findByText(/Terminal 2/);
    const tab2 = screen.getByText(/Terminal 2/).closest("button")!;
    expect(tab2).toHaveAttribute("aria-selected", "true");
    fireEvent.click(screen.getByText(/Terminal 1/));
    const tab1 = screen.getByText(/Terminal 1/).closest("button")!;
    expect(tab1).toHaveAttribute("aria-selected", "true");
    expect(tab2).toHaveAttribute("aria-selected", "false");
  });

  it("closes a tab and terminates the session", async () => {
    render(<TerminalWorkspace cwd={null} active={true} />);
    await screen.findByText(/Terminal 1/);
    fireEvent.click(screen.getByRole("button", { name: /Close Terminal 1/i }));
    await waitFor(() => expect(screen.queryByText(/Terminal 1/)).not.toBeInTheDocument());
    expect(screen.getByText("No terminals open.")).toBeInTheDocument();
    expect(mockInvoke).toHaveBeenCalledWith("terminal_close", expect.objectContaining({ id: expect.any(String) }));
  });

  it("renders terminal output events in order", async () => {
    render(<TerminalWorkspace cwd={null} active={true} />);
    await waitFor(() => expect(terminalInstances.length).toBe(1));
    const id = lastSpawnedId.value;
    await waitFor(() => expect(terminalInstances[0].written.join("")).toContain("PS> "));
    const inst = terminalInstances[0];

    emit("terminal_output", { id, seq: 2, data: b64("hello") });
    await waitFor(() => expect(inst.written.join("")).toContain("hello"));

    const before = inst.written.length;
    emit("terminal_output", { id, seq: 2, data: b64("dup") });
    await new Promise((r) => setTimeout(r, 50));
    expect(inst.written.length).toBe(before);

    emit("terminal_output", { id, seq: 1, data: b64("old") });
    await new Promise((r) => setTimeout(r, 50));
    expect(inst.written.join("")).not.toContain("old");
  });

  it("flushes output that arrived before history", async () => {
    let resolveHistory: (v: unknown) => void = () => {};
    historyFactory.value = () =>
      new Promise((resolve) => {
        resolveHistory = resolve;
      });
    render(<TerminalWorkspace cwd={null} active={true} />);
    await waitFor(() => expect(terminalInstances.length).toBe(1));
    const id = lastSpawnedId.value;
    const inst = terminalInstances[0];

    emit("terminal_output", { id, seq: 2, data: b64("early") });
    await new Promise((r) => setTimeout(r, 50));
    expect(inst.written.join("")).not.toContain("early");

    resolveHistory({ data: b64("HIST"), seq: 1 });
    await waitFor(() => expect(inst.written.join("")).toContain("early"));
    expect(inst.written.join("")).toContain("HIST");
  });

  it("handles exit events", async () => {
    render(<TerminalWorkspace cwd={null} active={true} />);
    await waitFor(() => expect(terminalInstances.length).toBe(1));
    const id = lastSpawnedId.value;
    emit("terminal_exit", { id, exit_code: 0, signal: null, error: null });
    expect(await screen.findByText(/Process exited with code 0\./)).toBeInTheDocument();
    expect(terminalInstances[0].written.join("")).toContain("[process exited with code 0]");
    expect(document.querySelector(".terminal-status-dot")).toHaveClass("exited");
    expect(screen.getByRole("button", { name: /Restart/i })).toBeInTheDocument();
  });

  it("does not overwrite a real exit with an attach failure", async () => {
    let rejectHistory: (e: Error) => void = () => {};
    historyFactory.value = () =>
      new Promise((_, reject) => {
        rejectHistory = reject;
      });
    render(<TerminalWorkspace cwd={null} active={true} />);
    await waitFor(() => expect(terminalInstances.length).toBe(1));
    const id = lastSpawnedId.value;

    emit("terminal_exit", { id, exit_code: 0, signal: null, error: null });
    expect(await screen.findByText(/Process exited with code 0\./)).toBeInTheDocument();

    rejectHistory(new Error("late"));
    await new Promise((r) => setTimeout(r, 50));
    expect(screen.queryByText(/ended before it could be attached/)).not.toBeInTheDocument();
    expect(screen.getByText(/Process exited with code 0\./)).toBeInTheDocument();
  });

  it("reports sessions that end before attach", async () => {
    historyFactory.value = () => {
      throw new Error("gone");
    };
    render(<TerminalWorkspace cwd={null} active={true} />);
    expect(await screen.findByText(/ended before it could be attached/)).toBeInTheDocument();
  });

  it("surfaces listener attachment failure", async () => {
    listenReject.value = true;
    render(<TerminalWorkspace cwd={null} active={true} />);
    expect(await screen.findByText(/Could not attach to the terminal/i)).toBeInTheDocument();
  });

  it("shows an error notice when spawn fails", async () => {
    mockInvoke.mockImplementation(async (cmd) => {
      if (cmd === "terminal_spawn") throw new Error("spawn failed");
      if (cmd === "terminal_history") return defaultHistory("x");
      if (cmd === "terminal_write" || cmd === "terminal_resize" || cmd === "terminal_close") return undefined;
      throw new Error(`Unhandled ${String(cmd)}`);
    });
    render(<TerminalWorkspace cwd={null} active={true} />);
    expect(await screen.findByText(/spawn failed/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Retry/i })).toBeInTheDocument();
  });

  it("cleans up listeners on unmount", async () => {
    const { unmount } = render(<TerminalWorkspace cwd={null} active={true} />);
    await waitFor(() => expect(terminalInstances.length).toBe(1));
    unmount();
    await waitFor(() => expect(listenHandlers.get("terminal_output")?.size ?? 0).toBe(0));
    expect(listenHandlers.get("terminal_exit")?.size ?? 0).toBe(0);
  });

  it("does not register duplicate listeners on rerender", async () => {
    const { rerender } = render(<TerminalWorkspace cwd={null} active={true} />);
    await waitFor(() => expect(terminalInstances.length).toBe(1));
    const before =
      (listenHandlers.get("terminal_output")?.size ?? 0) +
      (listenHandlers.get("terminal_exit")?.size ?? 0);
    rerender(<TerminalWorkspace cwd="/other" active={true} />);
    await new Promise((r) => setTimeout(r, 50));
    const after =
      (listenHandlers.get("terminal_output")?.size ?? 0) +
      (listenHandlers.get("terminal_exit")?.size ?? 0);
    expect(after).toBe(before);
  });

  it("preserves tabs when toggled inactive and active", async () => {
    const { rerender } = render(<TerminalWorkspace cwd={null} active={true} />);
    await screen.findByText(/Terminal 1/);
    rerender(<TerminalWorkspace cwd={null} active={false} />);
    expect(screen.getByText(/Terminal 1/)).toBeInTheDocument();
    rerender(<TerminalWorkspace cwd={null} active={true} />);
    expect(screen.getByText(/Terminal 1/)).toBeInTheDocument();
    const spawns = mockInvoke.mock.calls.filter((c) => c[0] === "terminal_spawn");
    expect(spawns).toHaveLength(1);
  });
});
