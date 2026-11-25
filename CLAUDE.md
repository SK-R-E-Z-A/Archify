# CLAUDE.md

## Project Overview

Archify is a Tauri desktop application (Rust backend + React/TypeScript frontend) that provides an AI coding assistant with autonomous task orchestration.

- **Backend**: Rust (Tauri v2) at `src-tauri/`
- **Frontend**: React 19 / TypeScript / Vite at `src/`
- **Design**: Dark theme with blue accents; future black-red visual identity
- **Test Suite**: 346/346 tests passing in `src-tauri/`

---

### 2026-11-07 — Archify: Skills-to-Execution Integration

**Task**: Integrate the standalone Skills Manager (`src-tauri/src/skills.rs`) into task creation, planning context, and execution prompt pipelines across Tauri backend and React frontend.

**Architecture & Implementation Details**:
- **Selection & Persistence**: Added `selected_skill_ids: Vec<String>` to `TaskRecord`, `StartTaskOptions`, and `TaskCreationOptions`. Persists stable skill IDs across atomic task snapshots, restarts, and retries (`Workspace::retry_task`) without duplicating bulky markdown bodies.
- **Validation**: Enforced strict validation in `Workspace::start_task_with_context`: checks that each selected skill exists in the managed skill store, is enabled, and contains no error-level validation issues. Rejects invalid, missing, or disabled skills with actionable errors.
- **Agent Targeting & Compatibility**: Integrated `crate::skills::resolve_skills_for_agent` honoring `targets_match`:
  - Skills targeting `"planner"` (or `"all"`) are injected into the task's `planning_context`.
  - Skills targeting `"executor"` (or `"all"`) are injected into the CLI executor prompt for both `run_graph_orchestration_loop` and `run_flat_orchestration_loop`.
- **Instruction Precedence**: Managed instructions strictly precede user-selected skills in both planning context and executor prompt composition.
- **Budgeting & Safety**: Respects the `MAX_APPLIED_SKILL_CHARS` (16 KiB) prompt budget with clean truncation and log warnings. Memory-only credentials and secret redaction remain strictly preserved.
- **Deterministic Integration Tests**: Added 5 new integration tests in `src-tauri/src/tests.rs`:
  1. `test_skills_integration_task_without_skills`: Tasks without skill selection execute without skill blocks.
  2. `test_skills_integration_validation_rejects_missing_disabled_and_invalid_skills`: Rejects missing, disabled, and invalid skills, while accepting valid ones.
  3. `test_skills_integration_retry_preserves_selected_skills`: Task retries preserve `selected_skill_ids`.
  4. `test_skills_integration_prompt_injection_order_and_agent_targeting`: Verifies managed instruction precedence and agent-targeting filtering across planner and executor prompts.
  5. `test_skills_integration_budget_truncation`: Verifies prompt budget enforcement and truncation reporting.

**Verification**:
- ✅ `cargo test --lib` → **218/218 passed** (0 failures, 0 warnings).
- ✅ `npx tsc --noEmit` → clean (0 errors).
- ✅ `npm run build` → clean production bundle.

**Status**: ✅ **COMPLETE**

---

### 2026-11-05 — Roadmap Item #13 Windows Process-Tree Termination Fallback

**Task**: Investigate, test, and implement a robust Windows process-tree termination fallback when Windows Job Objects are unavailable or assignment fails, ensuring grandchild and descendant processes are not orphaned.

**Audit & Implementation Findings**:
- **Investigation**: When Windows Job Object creation or process assignment failed, `run_process` previously fell back to direct child termination (`child.kill()`), which left grandchild/descendant processes running.
- **Fallback Implementation**: Updated `kill_process_tree` in `src-tauri/src/providers/process_cli.rs` so that when `job` is `None` on Windows, it invokes `taskkill /T /F /PID <pid>` to cleanly and reliably terminate the entire process tree. Also propagated assignment errors so failed attachments correctly activate the fallback path.
- **Test Seam**: Added a lightweight test-only environment check (`FORCE_JOB_OBJECT_FAILURE=1`) to force Job Object unavailability in tests without modifying normal runtime behavior.

**Files changed**:
- `src-tauri/src/providers/process_cli.rs` — Added `taskkill /T /F` fallback tree termination in `kill_process_tree` when `job` is `None`, propagated assignment errors in `attach_to_job`, and added `FORCE_JOB_OBJECT_FAILURE` test seam.
- `src-tauri/src/tests.rs` — Added 2 new Windows integration tests:
  1. `test_job_object_fallback_cancellation_terminates_descendants` — Proves cancellation successfully reaps grandchild processes via the fallback path.
  2. `test_job_object_fallback_timeout_terminates_descendants` — Proves timeout successfully reaps grandchild processes via the fallback path.

**Verification**:
- ✅ `cargo test` → **137/137 passed** (2 new fallback integration tests + 135 prior tests).
- ✅ `npx tsc --noEmit` → clean.
- ✅ `npm run build` → clean.

**Status**: ✅ **COMPLETE**

---

### 2026-11-05 — Roadmap Item #14 Timeout and Cancellation: Investigate, Test & Fix

**Task**: Investigate, verify, and deterministically test timeout and cancellation behavior across Archify without architecture redesign, wrapping hacks, or weakening existing tests.

**Audit Findings**:
- **ProcessCliExecutor & Windows Job Object**: `run_process` creates a `WindowsJob` configured with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION`. On cancellation or timeout, it terminates the job object, reaps the direct child within 5 seconds, and drains stdout/stderr up to bounded limits. Dropping the execution future triggers OS-level termination via `kill_on_drop(true)` and handle closure.
- **State Representation**: Tasks use `TaskState::Cancelled` with descriptive error for cancellation, and `TaskState::Failed` with iteration-level `timed_out: true` when timeouts exhaust iterations.
- **Racing & Slot Accounting**: Cancellation channel safely aborts in-flight execution. `occupies_execution_slot()` returns `false` as soon as the task enters a terminal state, releasing the execution slot exactly once and allowing subsequent queued tasks to be dispatched.

**Files changed**:
- `src-tauri/src/tests.rs` — Added 5 new deterministic tests:
  1. `test_timed_out_process_terminates_its_descendants` — Real Windows OS process-tree termination test verifying grandchild PID destruction on process timeout.
  2. `test_task_timeout_enforcement_and_iteration_limit` — Verifies iteration-level `timed_out: true` recording and safe transition to `Failed` upon exhausting iteration limits.
  3. `test_timeout_and_cancellation_racing` — Verifies that cancellation cleanly wins races against in-flight timeouts without state corruption.
  4. `test_timeout_slot_release_and_queue_dispatch` — Verifies exactly-once slot release and subsequent queued task dispatch following a timeout failure.
  5. `test_concurrent_timeout_isolation` — Verifies that a timed-out task in a concurrent execution does not corrupt or terminate sibling running tasks.

**Verification**:
- ✅ `cargo test` → **135/135 passed** (5 new deterministic timeout/cancellation tests, 130 prior tests preserved).
- ✅ `npx tsc --noEmit` → clean (0 errors).
- ✅ `npm run build` → clean (production bundle built).

**Status**: ✅ **COMPLETE**

---

### 2026-11-05 — Roadmap Item #7 Actual Concurrency Proof

**Task**: Investigate, test, and deterministically prove actual concurrent task execution in Archify across 7 core scenarios (A: Two simultaneous tasks, B: Limits 1/2/3, C: Priority queueing and slot release, D: Task isolation, E: Failure isolation, F: Cancellation isolation, G: Cross-project isolation).

**Files changed**:
- `src-tauri/src/providers/test_adapter.rs` — Added `TestExecutorScenario::ControlledHold`, atomic execution instrumentation (`ACTIVE_EXECUTORS`, `PEAK_EXECUTORS`), file barrier synchronization (`.executor_holding` / `.executor_release`), and pass-through semantics for multi-step task graph components.
- `src-tauri/src/orchestrator.rs` — Mapped `Some("controlled_hold")` in `create_executor()`.
- `src-tauri/src/tests.rs` — Added 7 deterministic concurrency and isolation tests with `CONCURRENCY_TEST_MUTEX` synchronization guard; added barrier helpers (`wait_for_hold_file`, `release_hold`, `reset_executor_counters`).

**Verification**:
- ✅ `cargo test` → **130/130 passed** (7 new deterministic concurrency proof tests).
- ✅ `npx tsc --noEmit` → clean.
- ✅ `npm run build` → clean.

**Status**: ✅ **COMPLETE**

---

### 2026-11-02 — Phase 3 Verification Improvements

**Task**: Implement Phase 3 verification improvements (7 items: snapshots, file change detection, large-project scanning, ignored dirs, verification status enum, evidence preservation, real change validation).

**Files changed**:
- `src-tauri/src/verification.rs` — Rewrote with snapshot-based change detection: `run_verification_command` now returns `VerificationStatus` (Passed/Failed/Skipped/Unavailable) instead of `bool`; added `snapshot_project` and `diff_snapshots` for before/after comparison; `collect_changed_files` accepts `ignored_dirs` parameter and returns `ChangedFiles` (created/modified/deleted); git porcelain detects created/modified/deleted with status codes; non-git fallback uses snapshot + mtime filtering (5-minute window); added `default_ignored_dirs()` and `is_excluded_dir()` helpers.
- `src-tauri/src/models.rs` — Removed duplicate `FileEntry` definition; changed `IterationRecord.changed_files` from `Vec<String>` to `ChangedFiles` to preserve created/modified/deleted evidence in task history.
- `src-tauri/src/orchestrator.rs` — Updated `.success` → `.status != VerificationStatus::Passed` in both graph and flat loops; passes `config.ignored_dirs` to `collect_changed_files`; passes `changed_files.all_paths()` to `review()`; stores `ChangedFiles` directly in `IterationRecord`.
- `src-tauri/src/tests.rs` — Updated `.success` → `.status` references; updated `collect_changed_files` calls to pass `&default_ignored_dirs()`; updated assertions for `ChangedFiles` type (`all_paths()`, `is_empty()`); added `ignored_dirs: vec![]` to `ProviderConfig` constructions.
- `src-tauri/src/providers/cloud_http.rs` — Changed `v.success` → `v.status` in review input format string.

**Verification**:
- ✅ `cargo test --lib --manifest-path src-tauri/Cargo.toml` → **73/73 passed**.
- ✅ `npx tsc --noEmit` → clean.

**Status**: ✅ **COMPLETE**

---

### 2026-11-02 — Phase 4 Persistence and Recovery

**Task**: Complete Phase 4: atomic task persistence, restart restoration, interrupted-task detection/recovery, duplicate-execution prevention after restart, history integrity validation, credential-safe history, and deterministic tests. Also fix the path-validation/duplicate-check ordering reported by the previous agent.

**Files changed**:
- `src-tauri/src/orchestrator.rs`:
  - `save_task_to_disk` is now atomic (temp file + `sync_all` + rename, Windows lock retry/remove fallback) and reports failures instead of silently dropping them.
  - Added `redact_secrets` so configured/environment API keys are scrubbed from persisted task history.
  - Added `Orchestrator::new_with_paths` so restore/recovery can be exercised against explicit storage.
  - `recover_interrupted_tasks` now recovers **every** mid-flight record (`Planning`, `Executing`, `Verifying`, `Reviewing`, `Retrying`), not just `current_task`, persists the cancellation, and records the original state in the error. A task in `AwaitingApproval` is restored intact because no CLI work has run and approval remains an explicit user action.
  - `start_task` validates the project root **before** the duplicate-task check, so invalid paths are never masked by stale active history.
  - Headless `Orchestrator::new(None)` uses a unique scratch directory, eliminating shared-state test pollution.
- `src-tauri/src/models.rs` — added `TaskState::is_interrupted_by_restart()`.
- `src-tauri/src/orchestrator.rs` (config persistence follow-up) — `ignored_dirs` is now applied by `configure_providers` and restored by `load_provider_config`.
- `src-tauri/src/tests.rs` — isolated orchestrator helper updated; restart/recovery tests rewritten deterministically; added coverage for path-validation ordering, recovery across multiple restart, atomic overwrite/no temp files, and API-key redaction in history.

**Behavior / safety decisions**:
- Mid-flight tasks are **never** re-executed on startup. They become `Cancelled` with an explicit "not resumed automatically; recovery required" error.
- `AwaitingApproval` tasks are restored with their plan/graph and still require explicit approval before execution.
- Because all mid-flight records are terminalized, stale history can no longer block a new task after restart.

**Verification**:
- ✅ `cargo test --manifest-path src-tauri/Cargo.toml` → **83/83 passed** (repeated twice).
- ✅ `npx tsc --noEmit` → clean.
- ✅ `npm run build` → clean.

**Status**: ✅ **COMPLETE**

---

### 2026-11-02 — Phase 5 Frontend and User Experience

**Task**: Connect the React UI to the full backend task contract and improve submission, approval, live progress, verification/reviewer evidence, correction history, cancellation, failure/completion reporting, task history, and state synchronization.

**Files changed**:
- `src/App.tsx`:
  - Types now match the backend: `VerificationResult.status`, structured `ChangedFiles`, and `ProviderStatus` execution settings.
  - Added `applyTask` ordering guard plus 1.5s polling while active and focus/visibility resync so late events or invoke responses cannot regress the displayed task.
  - Submission guards for empty prompts, missing project folders, active tasks, and pending actions; failures restore the prompt and surface inline errors.
  - Plan approval now supports an optional rejection reason, pending labels, and inline action errors.
  - Cancellation uses an inline confirmation step and a cancelling state.
  - Added graph step rendering with state badges, dependencies, acceptance criteria, errors, and progress.
  - Added a per-attempt timeline with executor stdout/stderr/exit/duration, grouped changed files, verification status/output, reviewer findings/unresolved issues/suggested fixes, and correction notes.
  - Added final completion/failure report cards with outcome stats, effective reviewer verdict (verification overrides), aggregate changed files, and a suggested next step.
  - Terminal tabs now show live orchestrator logs, latest executor output, and a dynamic problems list.
  - Added a read-only task-history modal using `list_task_history` with loading/error/empty states and plan/step/attempt details.
  - Provider modal now parses `cli_args` as an array, prefills verification command/ignored dirs, and clears the entered API key after saving.
- `src/App.css` — added styles for all task, plan, graph, iteration, result, terminal, and history components; widened the AI panel and terminal panel.
- `src-tauri/src/models.rs` — `ProviderStatus` now exposes non-secret `cli_args`, `verification_command`, and `ignored_dirs` so the modal can prefill them.
- `src-tauri/src/orchestrator.rs` — provider status populates the new fields; `update_and_persist_task` refreshes `updated_at` on every snapshot; both loops log executor exit status, changed-file counts, and verification outcomes.
- `src-tauri/src/tests.rs` — added coverage that `ProviderStatus` exposes non-secret execution settings.

**Verification**:
- ✅ `cargo test --all-targets` → **84/84 passed**.
- ✅ `npx tsc --noEmit` → clean.
- ✅ `npm run build` → clean.

**Status**: ✅ **COMPLETE**

---

### 2026-11-02 — Phase 6 Provider Configuration

**Task**: Validate planner/provider configuration end to end: provider types, endpoint/model settings, authentication, CLI executable and arguments, environment propagation, non-secret persistence, memory-only credentials, local model endpoints, and provider switching.

**Files changed**:
- `src-tauri/src/orchestrator.rs`:
  - `configure_providers` now validates the entire input before mutating anything: planner type must be `test`/`cloud_http`, executor type must be `test`/`process_cli`, `max_iterations` must be 1–10, and optional text is trimmed with control characters rejected. A rejected batch leaves memory and disk untouched.
  - Empty strings now clear optional settings (`cloud_endpoint`, `cloud_model`, `cli_executable`, `verification_command`, `cli_args`, `ignored_dirs`); blank `api_key` keeps the existing memory-only key.
  - API keys are rejected if they contain whitespace or control characters (prevents HTTP header injection) and are never persisted.
  - `process_cli` now requires an executable that resolves on `PATH` or as a file path; re-enabling `process_cli` reuses the previously saved executable instead of failing.
  - `normalize_cloud_endpoint` rejects embedded credentials, query/fragment components, malformed ports, and whitespace while still accepting bracketed IPv6.
  - `load_provider_config` applies the same sanitization non-fatally: unknown provider types fall back to `test`, endpoints are re-validated, and malformed args/models/commands are dropped with a log entry.
  - `get_provider_status` ignores empty credential environment variables and reports local loopback endpoints as configured without a key.
  - Added `validate_provider_ready`, called by `start_task` after the project-root and duplicate-task checks, so a task is never created when the selected cloud planner has no endpoint/key or the `process_cli` executor has no resolvable executable.
- `src-tauri/src/providers/mod.rs` — added `endpoint_host`, `is_local_endpoint_url` (localhost, 127.0.0.1, `[::1]`), and `resolve_executable` (PATH + `PATHEXT` on Windows).
- `src-tauri/src/providers/cloud_http.rs` — local-endpoint detection now uses the shared helper, covering IPv6 loopback.
- `src-tauri/src/providers/process_cli.rs` — removed the redundant `env_clear().envs(std::env::vars())` round-trip; the child inherits the parent environment and the documented Anthropic/Claude variables are forwarded explicitly.
- `src/App.tsx` — provider modal sends empty strings to clear optional settings, validates cloud endpoint/CLI executable before saving, warns when a remote endpoint has no API key, and documents that keys are memory-only and that local servers need no key.
- `src/App.css` — added `.config-hint` and `.config-warning` styles.
- `src-tauri/src/tests.rs` — added 18 tests for provider-type validation, max-iteration bounds, model trimming, clearing optional settings, CLI executable/argument validation, executable reuse, endpoint security, local endpoint detection/status, API key shape, provider switching (planner, executor, and persistence), load-time sanitization, CLI argument/environment propagation, and start-task preflight rejection.

**Verification**:
- ✅ `cargo test --all-targets` → **102/102 passed**.
- ✅ `npx tsc --noEmit` → clean.
- ✅ `npm run build` → clean.

**Status**: ✅ **COMPLETE**

---

### 2026-11-02 — Phase 7 Final Reliability and Release Preparation

**Task**: Harden process cleanup, error handling, credential handling, and task-history integrity; verify the whole product end to end; prepare the first usable Windows release.

**Files changed**:
- `src-tauri/src/providers/process_cli.rs`:
  - New shared `run_process(...)` supervisor used by both the executor and verification.
  - Windows children are assigned to a Job Object immediately after spawn; cancellation, timeout, wait errors, and drop paths terminate the full process tree.
  - stdout/stderr are read through `read_bounded`, which keeps at most `limit + 1` bytes in memory while continuing to drain the pipe. Source processes can no longer deadlock on a full pipe or exhaust memory.
  - Pipe drain and kill waits are bounded (5 s each) so cleanup itself cannot hang.
  - Default executor timeout raised from 120 s to **30 minutes** (`DEFAULT_TIMEOUT_SECS`) for realistic coding sessions.
  - Cancellation error strings preserved for existing tests/UI.
- `src-tauri/src/providers/windows_job.rs` — `WindowsJob` is documented as uniquely owning a kernel handle and implements `Send`/`Sync`, allowing the async supervisor to hold it across awaits.
- `src-tauri/src/providers/cloud_http.rs` — the curl transport now runs through `run_process`, so cloud responses are bounded (8 MiB), the request has a hard timeout, and cancellation/drop kills the transport process.
- `src-tauri/src/verification.rs` — verification commands now run through `run_process` with a **5-minute** timeout (`VERIFICATION_TIMEOUT_SECS`), bounded output, and process-tree termination. Runtime and timeout failures are reported explicitly.
- `src-tauri/src/orchestrator.rs`:
  - Planning runs in an inner spawned task; a planner panic becomes a clean planning failure and cancellation aborts the planner.
  - A plan/graph is attached only if the `AwaitingApproval` transition succeeds, so a task cancelled during planning cannot retain a plan.
  - `approve_plan` runs the execution loop behind a supervisor; a panic fails the task with an internal-error message instead of leaving it active forever.
  - Every transition in `approve_plan`, `reject_plan`, `cancel_task`, `update_and_persist_task`, and the flat/graph retry paths now updates in-memory `task_history` as well as the event stream and disk.
  - `save_task_to_disk` moves an existing record to `.bak` before a Windows replace fallback and restores it if replacement fails; temp/backup files are cleaned up.
  - Unreadable history records are logged instead of silently skipped.
  - Secret redaction now also covers `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY`, and `CLAUDE_API_KEY`.
- `src-tauri/src/tests.rs` — added:
  - `test_process_output_is_bounded_but_fully_drained`
  - `test_cancelled_process_terminates_its_descendants` (Windows)
  - `test_long_session_multiple_tasks_keep_history_consistent`
- `src-tauri/tauri.conf.json` — release metadata: product name, `1440x900` window with minimum size, centered, developer-tool category, bundle descriptions, per-user NSIS install, and a restrictive production CSP (with a dev CSP for Vite/HMR).
- `src-tauri/Cargo.toml` — real package description and author.
- `index.html` — real page title and description (no longer the Tauri scaffold).
- `README.md` — replaced the scaffold with release documentation: features, requirements, run/build commands, provider configuration, data locations, project layout, testing, safety model, and status.

**Verification**:
- ✅ `cargo test --all-targets` → **105/105 passed** (including the three new reliability tests), zero compiler warnings.
- ✅ `npx tsc --noEmit` → clean.
- ✅ `npm run build` → clean.
- ✅ `npm run tauri build` → release executable plus MSI (`2.46 MiB`) and NSIS (`1.64 MiB`) installers.
- ✅ Desktop smoke test: final executable launched, window title `Archify`, WebView UI rendered (115 accessibility elements: Explorer, Welcome, AI panel, terminal), process responsive, main-window close exited cleanly.

**Release**: Version **0.1.0**, Windows-first. Remaining known debt is non-blocking: pre-existing clippy style lints and `cargo fmt` drift outside the Phase 7 changes.

**Status**: ✅ **COMPLETE**

---

### 2026-11-05 — Task #9 Baseline Change Attribution Regression Tests

**Task**: Write deterministic regression tests verifying `task_level_changes` at completion, blocked, and failure paths.

**Files changed**:
- `src-tauri/src/tests.rs` — Added 9 deterministic baseline attribution tests:
  1. `test_baseline_excludes_pre_existing_modifications` — Pre-existing modifications before baseline are not attributed.
  2. `test_baseline_attributes_agent_modifications` — Executor-created files are attributed as created.
  3. `test_baseline_attributes_created_files` — Created files in empty project are attributed.
  4. `test_baseline_attributes_deleted_files` — Files absent at baseline are not attributed as deleted.
  5. `test_baseline_multi_iteration_attribution` — Each iteration reports only its own changes.
  6. `test_baseline_not_overwritten_by_later_iteration` — Baseline files never overwritten by later iteration attribution.
  7. `test_graph_baseline_attribution` — Baseline attribution works for graph/planner-based tasks.
  8. `test_cancellation_no_misleading_attribution` — Cancelled tasks have no misleading attribution.
  9. `test_recovery_without_baseline_no_fabricated_attribution` — No baseline = no fabricated attribution after restart.
- Restored `write_history_record` helper function used by workspace recovery tests.

**Verification**:
- ✅ `cargo test --lib` → **167/167 passed** (0 warnings).
- ✅ `npx tsc --noEmit` → clean.
- ✅ `npm run build` → clean.

**Status**: ✅ **COMPLETE**

---

### 2026-11-05 — Task #10 Full Verification

**Task**: Run full verification suite (`cargo test --lib`, `npx tsc --noEmit`, `npm run build`) after all Task #9 changes.

**Verification**:
- ✅ `cargo test --lib` → **167/167 passed**, zero warnings.
- ✅ `npx tsc --noEmit` → clean (0 errors).
- ✅ `npm run build` → clean production bundle.

**Status**: ✅ **COMPLETE**

---

### 2026-11-13 — Verification Edge-Case and Regression Audit

**Task**: Resolve Archify's task-completion gating ambiguity where unconfigured verification commands could be conflated with missing/unexecuted ones, preventing false-positive completions and ensuring strict requirement adherence.

**Audit & Implementation Details**:
- **Distinction**: Implemented an explicit `verification_configured` flag, passed through `ExecutionReport`, to distinguish "not configured" (no-op) from "configured but not executed" (blocking failure).
- **Enforcement**: Updated `assess_completion` in `src-tauri/src/verification.rs` and `recovery_policy::evaluate` to:
  - Permit completion if no command is configured.
  - Require successful verification if one is configured.
  - Fail with `MissingEvidence` if a command is configured but failed to produce results.
- **Regression Tests**: Added deterministic regression tests in `src-tauri/src/recovery_policy.rs` covering all four edge cases: unconfigured, missing results, passed, and failed.

**Verification**:
- ✅ `cargo test --lib` → **165/165 passed** (existing tests + new regression coverage).
- ✅ Full gate integration suite → **Passed**.
- ✅ `npx tsc --noEmit` → clean.
- ✅ `npm run build` → clean.

**Status**: ✅ **COMPLETE**

## Persistent Project Change Log

### 2026-11-07 — Archify Stage 3 Verification and Recovery Reliability

**Task**: Integrate verification, persisted task baselines, crash recovery, workspace persistence, and truthful graph progress across the combined Agent 1/2 implementation.

**Files changed**:
- `src-tauri/src/verification.rs` — Parse NUL-delimited Git status, attribute rename source/destination paths, apply ignored-directory filtering, include staged changes, and cover verification failure, timeout, and missing project roots.
- `src-tauri/src/workspace.rs` — Preserve valid JSON on replacement failure, recover from `.bak`, report malformed task history, bound restart retries to eligible saved plans, restore persisted baselines after queue reattachment, and start explicit retries with a fresh baseline.
- `src/App.tsx` — Count only completed graph steps toward completion progress.
- `src/App.test.tsx` — Verify skipped and failed graph steps do not inflate progress.

**Verification**:
- PASS `cargo test --manifest-path src-tauri/Cargo.toml --lib verification::tests` — 4/4 passed.
- PASS `cargo test --manifest-path src-tauri/Cargo.toml --lib workspace::persistence_tests` — 7/7 passed.
- PASS `cargo test --manifest-path src-tauri/Cargo.toml --lib` — 257/257 passed, including `terminal::tests::pty_roundtrip_carries_output_and_exits`.
- PASS Isolated PTY test — 5/5 repeated runs passed.
- PASS `npx tsc --noEmit`.
- PASS `npm run test` — 32/32 passed.
- PASS `npm run build`.
- PASS `git diff --check` (Git emitted only existing line-ending normalization warnings).

**Recovery limitation**: Restored task records are not treated as completed work. Missing baselines do not fabricate file attribution; crash retries are bounded and require a saved plan. Intelligent autonomous recovery remains unimplemented.

**Status**: COMPLETE

---
### 2026-11-01 — Real CLI Workflow Readiness Fixes

**Task**: Make the actual application ready to execute a real coding task through Claude Code CLI and OmniRoute.

**Files changed**:
- `src-tauri/src/orchestrator.rs` — Changed default `cli_executable` from `"git"` to `"claude"` in `create_executor()`; added validation in `configure_providers()` so `executor_type == "process_cli"` requires `cli_executable` to be set and non-empty (otherwise returns error).

**Verification**:
- ✅ `cargo test --manifest-path src-tauri/Cargo.toml --lib` → **68/68 passed**.
- ✅ `npx tsc --noEmit` → clean.
- ✅ `npm run build` → clean.

**Status**: ✅ **COMPLETE**

---

### 2026-11-01 — End-to-End Workflow Smoke Tests

**Task**: Add Section 8 smoke tests covering correction-cycle success and verification-failure recording.

**Files changed**:
- `src-tauri/src/tests.rs` — Added `test_full_successful_workflow_with_correction_cycle` (`needs_changes_once` planner scenario; asserts ≥2 iterations, changed files on disk, history persistence) and `test_verification_failure_and_rejection_workflow` (configures failing `cargo test` verification; asserts failed verification results are recorded).

**Verification**:
- ✅ `cargo test --manifest-path src-tauri/Cargo.toml --lib` → **68/68 passed** (66 prior + 2 smoke).

**Status**: ✅ **COMPLETE**

---

### 2026-11-01 — Recursive Changed-File Detection for Non-Git Projects

**Task**: Implement recursive changed-file detection for projects that are not Git repositories.

**Files changed**:
- `src-tauri/src/verification.rs` — Replaced top-level-only non-Git fallback with recursive scan; skip `node_modules`/`.git`/`target`/`dist`/`build`/common build dirs; keep 5-minute mtime window; normalize relative paths.
- `src-tauri/src/tests.rs` — Added focused tests for nested files, exclusions, empty/error paths, old vs new files, and Git behavior preservation.

**Verification**:
- ✅ `cargo test --manifest-path src-tauri/Cargo.toml --lib` → **66/66 passed** (61 prior + 5 new).
- ✅ Git path unchanged; non-Git path no longer returns only top-level files.

**Status**: ✅ **COMPLETE**

---

### 2026-10-25 — Task 16 Windows Job Object Placeholder Cleanup

**Task**: Task 16 — Remove unused Windows Job Object placeholder from `process_cli.rs`.

**Files changed**:
- `src-tauri/src/providers/process_cli.rs` — Removed unused WindowsJob import and placeholder code.

**Verification**:
- ✅ Test suite: 59/59 tests passing (57 core + 2 Windows Job Object tests in `windows_job.rs`).
- ✅ Compilation: Clean, no warnings in `process_cli.rs`.
- ✅ Diff inspected: Only placeholder code and unused imports removed.

**Status**: ✅ **COMPLETE**

---

### 2026-10-25 — Task 15 Windows Launcher Feasibility Spike (Investigation Only)

**Task**: Task 15 — Investigate feasibility of integrating Windows Job Object into `ProcessCliExecutor` (Task 13 follow-up).

**Files changed**:
- `src-tauri/src/providers/process_cli.rs` — Temporarily explored Job Object creation.

**Audit findings**:
- **Blocked**: `tokio::process::Command` does not expose the raw process `HANDLE` required for `AssignProcessToJobObject` without rewriting child process spawning.
- **Architectural Risk**: Integrating Windows Job Objects directly into `ProcessCliExecutor` would require replacing `tokio::process::Command` with raw Windows `CreateProcessW` spawning and custom asynchronous pipe management, violating the "no redesign" constraint.
- ✅ Existing test suite (59/59) passing.

**Status**: ⚠️ **BLOCKED / PAUSED (Integration architecture incompatibility; see Pending Decisions)**

---

### 2026-10-24 — Task 12 Windows Job Object Runtime Validation

**Task**: Task 12 — Prove Windows Job Object termination at runtime.

**Files changed**:
- `src-tauri/src/providers/windows_job.rs` — Added integration tests (`test_job_object_termination`, `test_managed_process_termination_on_drop`).

**Audit findings**:
- ✅ Integration Test Ran: `test_managed_process_termination_on_drop` proved that dropping `ManagedProcess` (closing the Job Object handle) successfully terminates the child process (`cmd.exe /c timeout /t 60`) via `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
- ✅ Test suite expanded: 59/59 tests passing (57 core + 2 Job Object tests).

**Status**: ✅ **COMPLETE (RUNTIME VALIDATED)**

---

### 2026-10-24 — Task 11 Windows Job Object Fix

**Task**: Task 11 — Fix compilation of Windows Job Object abstraction.

**Files changed**:
- `src-tauri/src/providers/windows_job.rs` — Fixed Windows API signatures for `windows` crate v0.60 (`PWSTR`/`PCWSTR` handling, `std::fmt::Display` for `JobError`, exported `ManagedProcess`).

**Audit findings**:
- ✅ Compilation fixed: `windows_job.rs` compiles cleanly without errors.
- ✅ All existing tests (57/57) passing.

**Status**: ✅ **COMPLETE (ABSTRACTION COMPILES)**

---

### 2026-10-24 — Task 10 Audit & Windows Job Object Status

**Task**: Task 10 — Audit and repair Windows Job Object abstraction independently.

**Historical context**:
- Task 8 output draining and cancellation was confirmed working (57/57 tests passing).
- Initial `windows_job.rs` had crate API mismatches with `windows` v0.60 (`PWSTR`/`PCWSTR` and missing `Display` implementations).
- *(Note: All compilation and interface issues identified here were subsequently resolved in Task 11 and validated in Task 12).*

**Status**: 📜 **SUPERSEDED BY TASKS 11 & 12**

---

### 2026-10-24 — Task 9 Windows Process-Tree Termination (WIP)

**Task**: Task 9 — Implement Windows Process-Tree Termination (Job Objects).

**Files changed**:
- `src-tauri/Cargo.toml` — Added `windows` crate v0.60 with features `Win32_Foundation`, `Win32_System_JobObjects`, `Win32_System_Threading`, `Win32_System_SystemServices`, `Win32_Security`.
- `src-tauri/src/providers/windows_job.rs` — Created initial Job Object abstraction (`WindowsJob`, `ManagedProcess`).
- `src-tauri/src/providers/mod.rs` — Added `pub mod windows_job`.

**Status**: 📜 **SUPERSEDED BY TASKS 10–12**

---

### 2026-10-24 — Task 8 Output Draining Implementation

**Task**: Task 8 — Make Real Process Cancellation Reliable (output draining).

**Files changed**:
- `src-tauri/src/providers/process_cli.rs` — Implemented `drain_pipe` helper for asynchronous stdout/stderr reading.

**Audit findings**:
- Ensured stdout and stderr are fully drained on normal completion, timeout, and cancellation.
- Output truncation at 1MB limit working correctly.
- All 57 tests passing (including `test_cancellation_during_active_graph_execution`).

**Status**: ✅ **COMPLETE**

---

### 2026-10-24 — Task 7 Design-Only Cancellation Contract Revision

**Task**: Task 7 — Finalize Cancellation Guarantees and Agent Ownership (design-only revision).

**Files changed**:
- `CLAUDE.md` — Created root documentation and persistent change log.

**Status**: ✅ **COMPLETE (DESIGN ONLY)**

---

# Project Instructions

## Development Workflow

- Design tasks require design-only audits; no implementation without user approval.
- Implementation tasks require TDD (test-first) unless user grants exception.
- Before starting a task, read the relevant source files fully.
- Maintain the Persistent Project Change Log in `CLAUDE.md` on every completed task.
- Project is not currently tracked by git; change verification relies on local inspection and automated test suites.

## Architecture Notes

- **Orchestrator** (`src-tauri/src/orchestrator.rs`):
  - Manages task execution lifecycle: `start_task`, `approve_plan`, `reject_plan`, `cancel_task`.
  - Execution loops: `run_orchestration_loop`, `run_graph_orchestration_loop`, `run_flat_orchestration_loop`.
- **Models** (`src-tauri/src/models.rs`):
  - State machine: `TaskState`, `ComponentState`, `TaskRecord`, `TaskGraph`, `Plan`.
  - Execution outcomes: `ExecutorResult`, `VerificationResult`, `IterationRecord`, `ReviewerResponse`.
- **Providers** (`src-tauri/src/providers/`):
  - `process_cli.rs`: `ProcessCliExecutor` plus the shared `run_process` supervisor — Job Object tree termination on Windows, bounded output drain, 30-minute executor timeout, and `tokio::select!` cancellation.
  - `windows_job.rs`: `WindowsJob`, `ManagedProcess` — Windows Job Object abstraction with process-tree termination (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`). `WindowsJob` is now integrated into `run_process`.
  - `test_adapter.rs`: `TestExecutor`, `TestPlannerReviewer`.
  - `cloud_http.rs`: `CloudHttpPlannerReviewer` (uses curl / HTTP client for model endpoints).
- **Verification** (`src-tauri/src/verification.rs`):
  - `run_verification_command` (5-minute supervised timeout), `collect_changed_files` (Git porcelain first; non-Git recursive mtime scan with excluded build dirs).

## Cancellation Design (Current State)

- **Mechanism**: Cancellation uses `tokio::sync::watch` channel (`cancel_tx` / `cancel_rx`).
- **Trigger**: `cancel_task()` sends `true` through the channel and transitions task state to `Cancelled`.
- **Execution Hook**: `run_process` checks `cancel_rx` before spawning and selects between `child.wait()` and `cancel_rx.changed()`.
- **Process Tree**: On Windows the child is assigned to a Job Object immediately after spawn; cancellation and timeouts call `TerminateJobObject` and then reap the direct child. A regression test spawns a grandchild and verifies it does not survive cancellation.
- **Remaining Limitation**: Verification commands are deliberately not user-cancellable mid-run; they are bounded by the 5-minute timeout. Unix process-group support is not implemented (Windows is the primary target).

## Testing & Verification

- **Backend (Rust)**:
  - Run all tests: `cargo test --manifest-path src-tauri/Cargo.toml` (or `cargo test` from `src-tauri/`).
  - Run library tests: `cargo test --lib --manifest-path src-tauri/Cargo.toml`.
  - Status: ✅ **218/218 tests passing** (includes skills-to-execution integration, prompt injection, agent targeting, validation rejection, retry preservation, and prompt budget truncation tests).
- **Frontend (TypeScript / React)**:
  - Type checking: `npx tsc --noEmit`. Status: ✅ **Clean (0 errors)**.
  - Note: `package.json` does not currently define a `test` script (`npm test` is unconfigured).

## Current State Summary

- ✅ Tasks 8–12 and 16 complete; Task 15 investigated and documented.
- ✅ Output draining and direct process cancellation fully functional in `ProcessCliExecutor`.
- ✅ Windows Job Object abstraction (`windows_job.rs`) implemented, compiled, and verified via runtime integration tests.
- ✅ Unused Job Object placeholder code removed from `process_cli.rs` (Task 16).
- ✅ Non-Git `collect_changed_files` now recursively detects recent creates/modifies with excluded dirs.
- ✅ Phase 3 verification improvements complete: snapshots, created/modified/deleted detection, ignored dirs, VerificationStatus enum, evidence preservation in history.
- ✅ Phase 4 persistence/recovery complete: atomic task writes, restart restoration, mid-flight interruption cancellation without re-execution, duplicate-execution prevention, history integrity checks, and API-key redaction in history.
- ✅ Phase 5 frontend/UX complete: backend-contract types, ordered live sync, approval/rejection flow, graph and iteration progress, verification/reviewer evidence, cancellation confirmation, completion/failure reports, terminal output/problems tabs, and task-history modal.
- ✅ Phase 6 provider configuration complete: atomic validation of provider types, endpoint/model/max-iteration bounds, header-safe memory-only API keys, resolvable CLI executables, sanitized args/ignored dirs, local loopback endpoints without keys, persistence of non-secret settings (including clearing), and provider switching.
- ✅ Phase 7 reliability/release complete: Windows process-tree termination via Job Objects, bounded output, 30-minute executor and 5-minute verification timeouts, panic-safe planning/execution supervision, per-transition history persistence, atomic save with backup/restore, expanded secret redaction, restrictive CSP, release metadata, README, and a successful desktop smoke test.
- ✅ Task #9 baseline attribution regression tests complete (9 deterministic tests).
- ✅ Task #10 full verification complete: 167/167 tests, tsc clean, build clean.
- ✅ 167/167 backend tests passing; frontend TypeScript type check and production build passing cleanly.
- ✅ E2E smoke coverage: correction-cycle success + verification-failure recording.

## Pending Decisions & Next Steps

1. ~~**Process-Tree Termination in `ProcessCliExecutor`**~~ — **Resolved in Phase 7.** `run_process` now assigns each child to a `WindowsJob` and terminates the job on cancellation/timeout; a regression test verifies descendants do not survive.
2. **Unix Process-Group Management**:
   - Determine crate selection (`nix` vs `libc`) for POSIX process-group termination (`setpgid` / `killpg`) when expanding cross-platform cancellation. Currently out of scope for the Windows-first release.
3. **Frontend Test Suite**:
   - Add a test runner configuration (e.g. Vitest or Jest) to `package.json` scripts if automated frontend testing is required.
4. **Lint/Format Baseline**:
   - `cargo fmt --check` and `cargo clippy -- -D warnings` still fail on pre-existing debt in `models.rs`, `tests.rs`, `verification.rs`, and `windows_job.rs`. Clean this up in a dedicated change rather than reformatting the crate during a reliability phase.
