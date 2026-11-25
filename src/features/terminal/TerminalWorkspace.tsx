import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import "./TerminalWorkspace.css";

type SpawnInfo = {
  id: string;
  shell: string;
  cwd: string;
  pid: number | null;
};

type OutputPayload = {
  id: string;
  seq: number;
  data: string;
};

type ExitPayload = {
  id: string;
  exit_code: number | null;
  signal: string | null;
  error: string | null;
};

type HistoryPayload = {
  data: string;
  seq: number;
};

type TabStatus = "starting" | "running" | "exited" | "error";

type TerminalTab = {
  id: string;
  title: string;
  status: TabStatus;
  cwd: string | null;
  exitCode: number | null;
  error: string | null;
};

type TerminalWorkspaceProps = {
  cwd: string | null;
  /** True when the whole shell panel is visible and focused. */
  active: boolean;
};

type TerminalPaneProps = {
  sessionId: string;
  active: boolean;
  running: boolean;
  onExit: (payload: ExitPayload) => void;
};

const OUTPUT_EVENT = "terminal_output";
const EXIT_EVENT = "terminal_exit";

function describeError(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Unexpected terminal error.";
}

function newTerminalId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  const random = Math.random().toString(16).slice(2);
  return `terminal-${Date.now().toString(16)}-${random}`;
}

function basename(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

function decodeBase64(value: string): Uint8Array {
  const binary = atob(value);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

function TerminalPane({ sessionId, active, running, onExit }: TerminalPaneProps) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const activeRef = useRef(active);
  const runningRef = useRef(running);
  const onExitRef = useRef(onExit);
  const lastSeqRef = useRef(0);
  const historyReadyRef = useRef(false);
  const pendingRef = useRef<OutputPayload[]>([]);
  const exitReceivedRef = useRef(false);

  activeRef.current = active;
  runningRef.current = running;
  onExitRef.current = onExit;

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const terminal = new Terminal({
      cursorBlink: true,
      fontFamily: 'Consolas, "Cascadia Mono", "Courier New", monospace',
      fontSize: 12,
      lineHeight: 1.15,
      scrollback: 5000,
      convertEol: false,
      theme: {
        background: "#141519",
        foreground: "#d3d6df",
        cursor: "#8caeff",
        cursorAccent: "#141519",
        selectionBackground: "#35415a",
        black: "#141519",
        red: "#ff8f8f",
        green: "#73c69c",
        yellow: "#e6c07b",
        blue: "#8caeff",
        magenta: "#c39bff",
        cyan: "#7fd1d1",
        white: "#d3d6df",
        brightBlack: "#6b6f7c",
        brightRed: "#ffb0b0",
        brightGreen: "#9be0ba",
        brightYellow: "#f0d39b",
        brightBlue: "#aec7ff",
        brightMagenta: "#d7baff",
        brightCyan: "#a8e6e6",
        brightWhite: "#f0f1f5",
      },
    });
    const fit = new FitAddon();
    terminal.loadAddon(fit);
    terminal.open(host);

    termRef.current = terminal;
    fitRef.current = fit;
    lastSeqRef.current = 0;
    historyReadyRef.current = false;
    pendingRef.current = [];
    exitReceivedRef.current = false;
    terminal.options.disableStdin = !runningRef.current;

    let disposed = false;
    const unlisteners: UnlistenFn[] = [];

    const writeChunk = (payload: OutputPayload) => {
      if (payload.id !== sessionId) return;
      if (!historyReadyRef.current) {
        pendingRef.current.push(payload);
        return;
      }
      if (payload.seq <= lastSeqRef.current) return;
      lastSeqRef.current = payload.seq;
      terminal.write(decodeBase64(payload.data));
    };

    const inputDisposable = terminal.onData((data) => {
      if (!runningRef.current) return;
      void invoke("terminal_write", { id: sessionId, data }).catch(() => {
        // A session can exit between a keystroke and the write. The exit event
        // updates the tab state; dropping the keystroke is correct.
      });
    });

    const resizeDisposable = terminal.onResize(({ cols, rows }) => {
      if (!runningRef.current) return;
      void invoke("terminal_resize", { id: sessionId, cols, rows }).catch(
        () => {},
      );
    });

    const fitIfVisible = () => {
      if (disposed || !activeRef.current) return;
      const { width, height } = host.getBoundingClientRect();
      if (width < 12 || height < 12) return;
      try {
        fit.fit();
      } catch {
        // The host can be between layouts while a tab is being revealed.
      }
    };

    const observer = new ResizeObserver(() => fitIfVisible());
    observer.observe(host);

    const start = async () => {
      const unlistenOutput = await listen<OutputPayload>(
        OUTPUT_EVENT,
        (event) => writeChunk(event.payload),
      );
      if (disposed) {
        unlistenOutput();
        return;
      }
      unlisteners.push(unlistenOutput);

      const unlistenExit = await listen<ExitPayload>(EXIT_EVENT, (event) => {
        if (event.payload.id !== sessionId) return;
        const payload = event.payload;
        // Remember that the authoritative exit arrived; a failed history read
        // must not replace it with a generic "ended before attach" error.
        exitReceivedRef.current = true;
        const detail = payload.error
          ? payload.error
          : payload.exit_code !== null
            ? `process exited with code ${payload.exit_code}`
            : "process exited";
        terminal.write(`\r\n\x1b[90m[${detail}]\x1b[0m\r\n`);
        onExitRef.current(payload);
      });
      if (disposed) {
        unlistenExit();
        return;
      }
      unlisteners.push(unlistenExit);

      let historyFailed = false;
      try {
        const history = await invoke<HistoryPayload>("terminal_history", {
          id: sessionId,
        });
        if (disposed) return;
        if (history.data) terminal.write(decodeBase64(history.data));
        lastSeqRef.current = history.seq;
      } catch {
        // The session disappears from the backend when the shell exits. That
        // can happen before this pane attaches (for example a shell that
        // cannot start), so surface it instead of leaving the tab "running".
        historyFailed = true;
      }
      if (disposed) return;

      historyReadyRef.current = true;
      const queued = pendingRef.current;
      pendingRef.current = [];
      queued.sort((left, right) => left.seq - right.seq);
      for (const payload of queued) {
        if (payload.seq <= lastSeqRef.current) continue;
        lastSeqRef.current = payload.seq;
        terminal.write(decodeBase64(payload.data));
      }

      if (historyFailed && !exitReceivedRef.current) {
        onExitRef.current({
          id: sessionId,
          exit_code: null,
          signal: null,
          error:
            "The terminal session ended before it could be attached. Restart the tab to try again.",
        });
      }
    };

    void start().catch((error) => {
      if (disposed) return;
      onExitRef.current({
        id: sessionId,
        exit_code: null,
        signal: null,
        error: `Could not attach to the terminal: ${describeError(error)}`,
      });
    });

    const frame = window.requestAnimationFrame(() => fitIfVisible());
    if (activeRef.current) terminal.focus();

    return () => {
      disposed = true;
      window.cancelAnimationFrame(frame);
      observer.disconnect();
      inputDisposable.dispose();
      resizeDisposable.dispose();
      unlisteners.forEach((unlisten) => unlisten());
      terminal.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  }, [sessionId]);

  useEffect(() => {
    const terminal = termRef.current;
    if (!terminal) return;
    terminal.options.disableStdin = !running;
  }, [running]);

  useEffect(() => {
    if (!active) return;
    const frame = window.requestAnimationFrame(() => {
      const fit = fitRef.current;
      const host = hostRef.current;
      if (!fit || !host) return;
      const { width, height } = host.getBoundingClientRect();
      if (width < 12 || height < 12) return;
      try {
        fit.fit();
      } catch {
        return;
      }
      termRef.current?.focus();
    });
    return () => window.cancelAnimationFrame(frame);
  }, [active]);

  return (
    <div className="terminal-pane">
      <div className="terminal-xterm-host" ref={hostRef} />
    </div>
  );
}

export default function TerminalWorkspace({ cwd, active }: TerminalWorkspaceProps) {
  const [tabs, setTabs] = useState<TerminalTab[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const counterRef = useRef(0);
  const bootstrapRef = useRef(false);
  const closingRef = useRef<Set<string>>(new Set());
  const startingRef = useRef<Set<string>>(new Set());
  const cwdRef = useRef(cwd);
  cwdRef.current = cwd;

  const createTab = useCallback(async () => {
    const id = newTerminalId();
    counterRef.current += 1;
    const title = `Terminal ${counterRef.current}`;

    setTabs((previous) => [
      ...previous,
      {
        id,
        title,
        status: "starting",
        cwd: null,
        exitCode: null,
        error: null,
      },
    ]);
    setActiveId(id);
    startingRef.current.add(id);

    try {
      const info = await invoke<SpawnInfo>("terminal_spawn", {
        id,
        cwd: cwdRef.current,
        cols: 80,
        rows: 24,
      });

      if (closingRef.current.delete(id)) {
        // The tab was closed while the shell was starting; do not leak it.
        void invoke("terminal_close", { id }).catch(() => {});
        return;
      }

      setTabs((previous) =>
        previous.map((tab) =>
          tab.id === id
            ? {
                ...tab,
                status: "running" as const,
                cwd: info.cwd,
                title: `${title} · ${basename(info.shell)}`,
              }
            : tab,
        ),
      );
    } catch (error) {
      closingRef.current.delete(id);
      const message = describeError(error);
      setTabs((previous) =>
        previous.map((tab) =>
          tab.id === id
            ? { ...tab, status: "error" as const, error: message }
            : tab,
        ),
      );
    } finally {
      startingRef.current.delete(id);
    }
  }, []);

  const closeTab = useCallback(async (id: string) => {
    const wasStarting = startingRef.current.has(id);
    closingRef.current.add(id);
    setTabs((previous) => previous.filter((tab) => tab.id !== id));
    setActiveId((current) => (current === id ? null : current));
    try {
      await invoke("terminal_close", { id });
    } catch {
      // Closing is idempotent: the shell may already have exited.
    } finally {
      // A tab that is still starting must keep the marker until the pending
      // spawn settles; createTab closes the late session for us.
      if (!wasStarting) closingRef.current.delete(id);
    }
  }, []);

  const restartTab = useCallback(
    async (id: string) => {
      await closeTab(id);
      await createTab();
    },
    [closeTab, createTab],
  );

  const handleExit = useCallback((tabId: string, payload: ExitPayload) => {
    setTabs((previous) =>
      previous.map((tab) =>
        tab.id === tabId
          ? {
              ...tab,
              status: payload.error ? ("error" as const) : ("exited" as const),
              exitCode: payload.exit_code,
              error: payload.error,
            }
          : tab,
      ),
    );
  }, []);

  useEffect(() => {
    if (bootstrapRef.current) return;
    bootstrapRef.current = true;
    void createTab();
  }, [createTab]);

  useEffect(() => {
    if (activeId !== null || tabs.length === 0) return;
    setActiveId(tabs[tabs.length - 1].id);
  }, [activeId, tabs]);

  const activeTab = tabs.find((tab) => tab.id === activeId) ?? null;
  const activeCwd = activeTab?.cwd ?? cwd;

  return (
    <div className="terminal-workspace">
      <div className="terminal-workspace-bar">
        <div className="terminal-workspace-tabs" role="tablist">
          {tabs.map((tab) => (
            <div
              key={tab.id}
              className={
                tab.id === activeId
                  ? "terminal-workspace-tab-wrap selected"
                  : "terminal-workspace-tab-wrap"
              }
            >
              <button
                type="button"
                role="tab"
                aria-selected={tab.id === activeId}
                className="terminal-workspace-tab"
                title={tab.error ?? tab.title}
                onClick={() => setActiveId(tab.id)}
              >
                <span className={`terminal-status-dot ${tab.status}`} />
                <span className="terminal-workspace-tab-label">{tab.title}</span>
              </button>
              <button
                type="button"
                className="terminal-workspace-tab-close"
                aria-label={`Close ${tab.title}`}
                title="Close terminal"
                onClick={() => void closeTab(tab.id)}
              >
                ×
              </button>
            </div>
          ))}
        </div>
        <div className="terminal-workspace-actions">
          <span className="terminal-workspace-cwd" title={activeCwd ?? ""}>
            {activeCwd ? basename(activeCwd) : "home"}
          </span>
          <button
            type="button"
            className="terminal-workspace-new"
            title="New terminal"
            aria-label="New terminal"
            onClick={() => void createTab()}
          >
            ＋
          </button>
        </div>
      </div>

      <div className="terminal-workspace-body">
        {tabs.length === 0 && (
          <div className="terminal-workspace-empty">
            <p>No terminals open.</p>
            <button type="button" onClick={() => void createTab()}>
              New terminal
            </button>
          </div>
        )}

        {tabs.map((tab) => (
          <div
            key={tab.id}
            className={
              tab.id === activeId
                ? "terminal-workspace-pane"
                : "terminal-workspace-pane hidden"
            }
          >
            {tab.status === "starting" && (
              <div className="terminal-workspace-notice">
                Starting {tab.title}…
              </div>
            )}

            {tab.status === "error" && (
              <div className="terminal-workspace-notice error">
                <p>{tab.error ?? "The terminal could not start."}</p>
                <button
                  type="button"
                  onClick={() => void restartTab(tab.id)}
                >
                  Retry
                </button>
              </div>
            )}

            {tab.status === "exited" && (
              <div className="terminal-workspace-exit">
                <span>
                  {tab.exitCode !== null
                    ? `Process exited with code ${tab.exitCode}.`
                    : "Process exited."}
                </span>
                <button
                  type="button"
                  onClick={() => void restartTab(tab.id)}
                >
                  Restart
                </button>
              </div>
            )}

            {(tab.status === "running" || tab.status === "exited") && (
              <TerminalPane
                sessionId={tab.id}
                active={active && tab.id === activeId}
                running={tab.status === "running"}
                onExit={(payload) => handleExit(tab.id, payload)}
              />
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
