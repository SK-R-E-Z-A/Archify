# Archify

A local-first desktop AI coding assistant built with **Tauri v2**, **Rust**, and **React 19 / TypeScript**.

Describe a coding task, review the generated plan, approve it, and the app drives the full loop:
**plan → execute → detect changed files → verify → review → iterate or complete**. Every transition is
persisted, streamed to the UI, and recoverable after a restart.

## Highlights

- **Approval-gated execution** — no CLI command runs until you approve the plan.
- **Supervised process execution** — on Windows, coding agents run inside a Job Object so cancellation,
  timeouts, and app shutdown terminate the whole process tree, not just the direct child.
- **Bounded output** — stdout/stderr are capped while still being drained, so a noisy CLI cannot hang or
  exhaust memory.
- **Snapshots and verification** — changed files are detected (Git or filesystem snapshot), verification
  commands run with a five-minute hang guard, and evidence is retained in task history.
- **Crash-safe history** — atomic writes with Windows backup/restore fallback, per-transition timestamps,
  and interrupted tasks are cancelled on restart instead of silently re-executed.
- **Credential hygiene** — provider-profile API keys are stored in Windows Credential Manager,
  excluded from ordinary configuration and task-history serialization, and never returned to the UI;
  well-known key environment variables are redacted from persisted task history.
- **Provider flexibility** — test adapters, OpenAI-compatible cloud planners/reviewers (including local
  loopback endpoints), and any CLI coding agent with configurable arguments.
- **Bounded cloud transport** — HTTP responses are capped, requests have a hard timeout, and the curl
  transport is terminated if a task is cancelled.

## Requirements

- Windows 10/11 (primary target) with **WebView2**
- [Node.js](https://nodejs.org/) 20+ and npm
- [Rust](https://www.rust-lang.org/tools/install) stable with the MSVC toolchain
- For cloud planning: an OpenAI-compatible endpoint and API key (or a local endpoint such as Ollama/LM Studio)
- For CLI execution: a coding CLI on `PATH` (for example `claude`, `codex`, `aider`, or a custom wrapper)

## Run locally

```bash
npm install
npm run tauri dev
```

The frontend alone can be previewed with `npm run dev` (Tauri commands will be unavailable in a plain
browser).

## Build a release

```bash
npm run tauri build
```

Artifacts are written under `src-tauri/target/release/`:

- `archify.exe` — standalone executable
- `bundle/msi/Archify_0.1.0_x64_en-US.msi` — MSI installer
- `bundle/nsis/Archify_0.1.0_x64-setup.exe` — NSIS installer

To compile without creating installers:

```bash
npm run tauri build -- --no-bundle
```

> The 0.1.0 installers are not code-signed. Windows SmartScreen may warn on first launch; use
> "More info → Run anyway" after verifying the artifact you built.

## Configuration

Open **Provider Configuration** in the app to choose:

| Setting | Values |
| --- | --- |
| Planner / reviewer | `test` (offline adapter) or `cloud_http` (OpenAI-compatible) |
| Executor | `test` (offline adapter) or `process_cli` |
| Cloud endpoint | Chat-completions URL, e.g. `https://api.openai.com/v1/chat/completions` |
| Model | Provider model name |
| API key | Saved per provider profile in Windows Credential Manager; never returned to the UI |
| CLI executable | Resolvable command, e.g. `claude` or an absolute path |
| CLI args | Passed to the CLI; the prompt is written to stdin |
| Verification command | e.g. `cargo test`, `npm test`, `npx tsc --noEmit` |
| Max iterations | 1–10 |
| Ignored directories | Excluded from change detection |

The following environment variables are supported as fallbacks and are redacted from history:

`PLANNER_API_KEY`, `OPENAI_API_KEY`, `GEMINI_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY`,
`CLAUDE_API_KEY`.

Non-secret provider settings persist to the app data directory
(`%APPDATA%\com.skrez.archify\provider_config.json` on Windows). Task history is stored as
JSON under `%APPDATA%\com.skrez.archify\task_history\`. Headless/test builds use an isolated
scratch directory instead.

> Starting a task requires a configured planner and executor. In `test` mode the built-in adapters run
> offline and are useful for trying the full workflow without credentials.

## Project layout

```
src/                     React + TypeScript UI
src-tauri/src/
  lib.rs                 Tauri commands, window setup, directory listing
  models.rs              Task, plan, graph, provider, and verification models
  orchestrator.rs        Task lifecycle, persistence, recovery, redaction
  verification.rs        Snapshots, changed-file detection, verification runner
  providers/
    cloud_http.rs        OpenAI-compatible planner/reviewer
    process_cli.rs       Supervised CLI executor (bounded output, timeouts)
    windows_job.rs       Windows Job Object process-tree control
    test_adapter.rs      Deterministic offline adapters
  tests.rs               Integration and regression tests
```

## Testing

```bash
cd src-tauri
cargo test --all-targets        # backend suite
```

```bash
npx tsc --noEmit                # TypeScript type check
npm run build                   # production frontend build
```

Node-based process tests skip automatically when `node` is not resolvable.

## Safety model

- A task cannot start while another task is active (checked in memory and in recovered history).
- Cancellation stops the executor process tree and persists a terminal `Cancelled` state.
- Timeouts: 30 minutes for executor processes, 5 minutes for verification commands.
- Interrupted tasks are marked `Cancelled` on restart and are never re-executed automatically.
- History writes are atomic; a failed replacement restores the previous complete record.
- A panic inside the planning or execution loop fails the task instead of leaving it active forever.

## Status

Version 0.1.0 — first usable release. Single-window desktop app; Windows is the primary supported target.
