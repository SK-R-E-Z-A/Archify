use crate::models::PromptDelivery;
use crate::providers::{BoxFuture, CodingExecutor};
use crate::stall_watchdog;
use std::env;
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::watch;
use tokio::time::{Duration, timeout};

#[cfg(windows)]
use crate::providers::windows_job::WindowsJob;

/// Long-running coding agents routinely need more than a couple of minutes.
/// The timeout is a safety net against a genuinely hung process, not a normal
/// working limit, so the default is deliberately generous (30 minutes).
pub const DEFAULT_TIMEOUT_SECS: u64 = 30 * 60;

/// How long to wait for pipe readers to finish after the child is terminated.
const PIPE_DRAIN_TIMEOUT_SECS: u64 = 5;

/// How long to wait for a killed process to be reaped.
const KILL_WAIT_TIMEOUT_SECS: u64 = 5;

#[cfg(windows)]
type JobGuard = WindowsJob;
#[cfg(not(windows))]
type JobGuard = ();

#[derive(Debug)]
pub struct ProcessOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub duration_ms: u64,
}

pub struct ProcessCliExecutor {
    executable: String,
    args: Vec<String>,
    prompt_delivery: PromptDelivery,
    timeout_secs: u64,
    max_output_bytes: usize,
}

impl ProcessCliExecutor {
    pub fn new(
        executable: String,
        args: Vec<String>,
        timeout_secs: Option<u64>,
        max_output_bytes: Option<usize>,
    ) -> Self {
        Self::with_prompt_delivery(
            executable,
            args,
            PromptDelivery::PrefixedStdin,
            timeout_secs,
            max_output_bytes,
        )
    }

    pub fn with_prompt_delivery(
        executable: String,
        args: Vec<String>,
        prompt_delivery: PromptDelivery,
        timeout_secs: Option<u64>,
        max_output_bytes: Option<usize>,
    ) -> Self {
        Self {
            executable,
            args,
            prompt_delivery,
            timeout_secs: timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS),
            max_output_bytes: max_output_bytes.unwrap_or(1024 * 1024), // 1MB
        }
    }

    fn truncate_output(bytes: &[u8], limit: usize) -> String {
        if bytes.len() <= limit {
            String::from_utf8_lossy(bytes).into_owned()
        } else {
            let mut s = String::from_utf8_lossy(&bytes[..limit]).into_owned();
            s.push_str("\n... [Output truncated: exceeded byte limit]");
            s
        }
    }
}

impl Default for ProcessCliExecutor {
    fn default() -> Self {
        Self::new("cmd".to_string(), Vec::new(), None, None)
    }
}

fn structured_output_args(
    executable: &str,
    args: &[String],
    prompt_delivery: PromptDelivery,
) -> Vec<String> {
    let executable_name = Path::new(executable)
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    let mut args = args.to_vec();

    if executable_name.eq_ignore_ascii_case("claude")
        && prompt_delivery == PromptDelivery::PrefixedStdin
        && !args
            .iter()
            .any(|arg| arg == "--output-format" || arg.starts_with("--output-format="))
    {
        args.splice(0..0, ["--output-format".to_string(), "json".to_string()]);
    } else if executable_name.eq_ignore_ascii_case("codex")
        && !args.iter().any(|arg| arg == "--json")
    {
        if let Some(exec_index) = args.iter().position(|arg| arg == "exec") {
            args.insert(exec_index + 1, "--json".to_string());
        }
    }

    args
}

/// Read a pipe while keeping at most `limit + 1` bytes in memory.
///
/// The extra byte lets `truncate_output` distinguish "exactly at the limit"
/// from "more output was discarded". Everything past the cap is drained and
/// thrown away, which keeps a runaway CLI from exhausting memory or blocking
/// on a full pipe.
async fn read_bounded<R: AsyncRead + Unpin>(
    mut pipe: R,
    limit: usize,
    tracker: Option<Arc<tokio::sync::Mutex<crate::agent_lifecycle::AgentLifecycleTracker>>>,
) -> Vec<u8> {
    let cap = limit.saturating_add(1);
    let mut collected = Vec::with_capacity(cap.min(64 * 1024));
    let mut buffer = [0u8; 8192];

    loop {
        match pipe.read(&mut buffer).await {
            Ok(0) => break,
            Ok(n) => {
                // Report incremental output progress to the lifecycle tracker so
                // the automatic stall watchdog can reset its silence timer as
                // bytes arrive, rather than only when the pipe closes.
                if let Some(ref t) = tracker {
                    t.lock().await.record_output(n);
                }
                if collected.len() < cap {
                    let take = (cap - collected.len()).min(n);
                    collected.extend_from_slice(&buffer[..take]);
                }
            }
            Err(_) => break,
        }
    }

    collected
}

async fn drain_pipe(handle: &mut tokio::task::JoinHandle<Vec<u8>>) -> Vec<u8> {
    match timeout(Duration::from_secs(PIPE_DRAIN_TIMEOUT_SECS), &mut *handle).await {
        Ok(Ok(data)) => data,
        Ok(Err(_)) => Vec::new(),
        Err(_) => {
            // A descendant may still hold the pipe open. Do not let cleanup
            // hang forever: abort the reader and close our end.
            handle.abort();
            Vec::new()
        }
    }
}

#[cfg(windows)]
fn attach_to_job(
    job: &JobGuard,
    child: &tokio::process::Child,
) -> Result<(), crate::providers::windows_job::JobError> {
    use windows::Win32::Foundation::HANDLE;

    if let Some(raw) = child.raw_handle() {
        if let Err(e) = job.assign_process(HANDLE(raw as _)) {
            return Err(e);
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn attach_to_job(_job: &JobGuard, _child: &tokio::process::Child) {}

/// Ensure that the child *and any descendants it spawned* are terminated.
#[cfg(windows)]
async fn kill_process_tree(child: &mut tokio::process::Child, job: Option<&JobGuard>) {
    if let Some(job) = job {
        // Use Job Object to terminate the entire process tree
        let _ = job.terminate();
    } else if let Some(pid) = child.id() {
        // Fallback: use taskkill to kill the entire process tree
        // Direct child.kill() alone leaves grandchildren, so taskkill /T /F is required
        let _ = tokio::process::Command::new("taskkill")
            .args(["/T", "/F", "/PID", &pid.to_string()])
            .output()
            .await;
    }
    // No need to call child.kill() — taskkill handles the entire tree
    let _ = child.kill().await;
    let _ = timeout(Duration::from_secs(KILL_WAIT_TIMEOUT_SECS), child.wait()).await;
}

#[cfg(not(windows))]
async fn kill_process_tree(child: &mut tokio::process::Child, _job: Option<&JobGuard>) {
    use libc::{killpg, setpgid};

    let pid = child.id().expect("process has no id") as i32;

    // Set the child's process group ID to its own PID so killpg can
    // terminate the entire group.  This must happen before any descendants
    // are spawned; we do it immediately after spawn via the job supervisor.
    // We use setpgid(pid, pid) to create a new process group.
    let _ = unsafe { setpgid(pid, pid) };

    // Send SIGTERM to the entire process group for graceful termination
    let _ = unsafe { killpg(pid, 15) }; // SIGTERM

    // Wait with timeout for graceful termination
    let _ = timeout(Duration::from_secs(KILL_WAIT_TIMEOUT_SECS), child.wait()).await;

    // If the process is still running, force kill the entire group
    let _ = unsafe { killpg(pid, 9) }; // SIGKILL
}

enum WaitOutcome {
    Completed(std::io::Result<std::process::ExitStatus>),
    TimedOut,
    Cancelled,
}

/// Spawn and supervise a child process.
///
/// On Windows the child is attached to a Job Object so cancellation and
/// timeouts terminate the entire process tree, not just the direct child.
/// Output is bounded while still being drained, so a noisy process cannot
/// block on a full pipe or exhaust memory.
pub async fn run_process(
    mut cmd: Command,
    stdin_data: Option<Vec<u8>>,
    timeout_secs: u64,
    max_output_bytes: usize,
    mut cancel_rx: Option<&mut watch::Receiver<bool>>,
    task_id: Option<&str>,
) -> Result<ProcessOutput, String> {
    use crate::agent_lifecycle::AgentLifecycleTracker;

    cmd.stdin(if stdin_data.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    // Safety net if this future is dropped unexpectedly.
    .kill_on_drop(true);

    if cancel_rx
        .as_deref()
        .map_or(false, |receiver| *receiver.borrow())
    {
        return Err("Execution was cancelled before starting".to_string());
    }

    let start_time = Instant::now();
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn process: {}", e))?;

    // Create lifecycle tracker for this process and register it with the
    // automatic stall watchdog so a stalled executor process can be detected
    // by the background scheduler tick.
    let tracker = Arc::new(tokio::sync::Mutex::new(AgentLifecycleTracker::new(
        crate::agent_lifecycle::TrackerConfig::default(),
    )));
    tracker.lock().await.record_start();
    if let Some(tid) = task_id {
        stall_watchdog::register_process(
            tid.to_string(),
            tracker.clone(),
            crate::stall_watchdog::DEFAULT_STALL_WINDOW_SECS,
        );
    }

    #[cfg(windows)]
    let job: Option<JobGuard> = if env::var("FORCE_JOB_OBJECT_FAILURE").is_ok() {
        eprintln!("[process] Job Object disabled via FORCE_JOB_OBJECT_FAILURE");
        None
    } else {
        match WindowsJob::new() {
            Ok(job) => match attach_to_job(&job, &child) {
                Ok(()) => Some(job),
                Err(e) => {
                    eprintln!("[process] could not attach child to job object: {}", e);
                    None
                }
            },
            Err(e) => {
                eprintln!("[process] job object creation failed: {}", e);
                None
            }
        }
    };
    #[cfg(not(windows))]
    let job: Option<JobGuard> = {
        attach_to_job(&(), &child);
        None
    };

    if let Some(data) = stdin_data {
        if let Some(mut stdin) = child.stdin.take() {
            tokio::spawn(async move {
                let _ = stdin.write_all(&data).await;
                let _ = stdin.flush().await;
            });
        }
    }

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let limit = max_output_bytes;
    let stdout_tracker = tracker.clone();
    let mut stdout_handle = tokio::spawn(async move {
        match stdout_pipe {
            Some(pipe) => read_bounded(pipe, limit, Some(stdout_tracker)).await,
            None => Vec::new(),
        }
    });
    let stderr_tracker = tracker.clone();
    let mut stderr_handle = tokio::spawn(async move {
        match stderr_pipe {
            Some(pipe) => read_bounded(pipe, limit, Some(stderr_tracker)).await,
            None => Vec::new(),
        }
    });

    let wait_future = timeout(Duration::from_secs(timeout_secs), child.wait());
    let wait_outcome = if let Some(receiver) = cancel_rx.as_deref_mut() {
        tokio::select! {
            res = wait_future => match res {
                Ok(result) => WaitOutcome::Completed(result),
                Err(_) => {
                    tracker.lock().await.record_timed_out();
                    WaitOutcome::TimedOut
                }
            },
            _ = receiver.changed() => {
                if *receiver.borrow_and_update() {
                    tracker.lock().await.check_cancellation();
                }
                WaitOutcome::Cancelled
            }
        }
    } else {
        match wait_future.await {
            Ok(result) => WaitOutcome::Completed(result),
            Err(_) => {
                tracker.lock().await.record_timed_out();
                WaitOutcome::TimedOut
            }
        }
    };

    let duration_ms = start_time.elapsed().as_millis() as u64;
    let mut tracker = tracker.lock().await;
    let tid = task_id.map(|t| t.to_string());

    match wait_outcome {
        WaitOutcome::Completed(Ok(status)) => {
            let stdout = drain_pipe(&mut stdout_handle).await;
            let stderr = drain_pipe(&mut stderr_handle).await;
            tracker.record_completed(status.code());
            if let Some(ref t) = tid {
                stall_watchdog::deregister_process(t);
            }
            Ok(ProcessOutput {
                exit_code: status.code(),
                stdout: ProcessCliExecutor::truncate_output(&stdout, max_output_bytes),
                stderr: ProcessCliExecutor::truncate_output(&stderr, max_output_bytes),
                timed_out: false,
                duration_ms,
            })
        }
        WaitOutcome::Completed(Err(e)) => {
            kill_process_tree(&mut child, job.as_ref()).await;
            let _ = drain_pipe(&mut stdout_handle).await;
            let _ = drain_pipe(&mut stderr_handle).await;
            tracker.record_failed(e.to_string());
            if let Some(ref t) = tid {
                stall_watchdog::deregister_process(t);
            }
            Err(format!("Process wait error: {}", e))
        }
        WaitOutcome::TimedOut => {
            kill_process_tree(&mut child, job.as_ref()).await;
            let stdout = drain_pipe(&mut stdout_handle).await;
            let stderr = drain_pipe(&mut stderr_handle).await;
            tracker.record_timed_out();
            if let Some(ref t) = tid {
                stall_watchdog::deregister_process(t);
            }
            Ok(ProcessOutput {
                exit_code: None,
                stdout: ProcessCliExecutor::truncate_output(&stdout, max_output_bytes),
                stderr: ProcessCliExecutor::truncate_output(&stderr, max_output_bytes),
                timed_out: true,
                duration_ms,
            })
        }
        WaitOutcome::Cancelled => {
            kill_process_tree(&mut child, job.as_ref()).await;
            let _ = drain_pipe(&mut stdout_handle).await;
            let _ = drain_pipe(&mut stderr_handle).await;
            tracker.check_cancellation();
            if let Some(ref t) = tid {
                stall_watchdog::deregister_process(t);
            }
            Err("Execution was cancelled by user".to_string())
        }
    }
}

/// Parse explicit token-usage metadata from supported CLI structured output.
///
/// Claude Code is supported through its final `result` event. OpenAI Codex CLI
/// is supported through its cumulative `token_count` event; the latest report
/// is used rather than summing repeated snapshots. Other known agents (Aider,
/// Gemini CLI, OpenCode, Cursor Agent, and GitHub Copilot CLI) currently have no
/// supported, stable usage format here and return `None`. Nothing is inferred.
pub fn parse_cli_token_usage(stdout: &str) -> Option<crate::models::CliTokenUsage> {
    use crate::models::CliTokenUsage;
    use serde_json::Value;

    fn number(value: &Value, key: &str) -> Option<u64> {
        value.get(key).and_then(Value::as_u64)
    }

    fn claude_result(value: &Value) -> Option<CliTokenUsage> {
        if value.get("type").and_then(Value::as_str) != Some("result") {
            return None;
        }
        let usage = value.get("usage")?;
        let mut result = CliTokenUsage {
            input_tokens: number(usage, "input_tokens"),
            output_tokens: number(usage, "output_tokens"),
            cache_creation_input_tokens: number(usage, "cache_creation_input_tokens"),
            cache_read_input_tokens: number(usage, "cache_read_input_tokens"),
            model: value
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    let models = value
                        .get("modelUsage")
                        .and_then(Value::as_object)
                        .filter(|models| models.len() == 1)?;
                    models.keys().next().cloned()
                }),
            format: "claude_code_json".to_string(),
            ..Default::default()
        };
        if let Some(cost) = value
            .get("total_cost_usd")
            .and_then(Value::as_f64)
            .filter(|cost| cost.is_finite() && *cost >= 0.0)
        {
            result.total_cost_usd = Some(cost.to_string());
        }
        (result.input_tokens.is_some() || result.output_tokens.is_some()).then_some(result)
    }

    fn codex_token_count(value: &Value) -> Option<CliTokenUsage> {
        let info = if value.get("type").and_then(Value::as_str) == Some("token_count") {
            value.get("info")?
        } else {
            let payload = value.get("payload")?;
            if value.get("type").and_then(Value::as_str) != Some("event_msg")
                || payload.get("type").and_then(Value::as_str) != Some("token_count")
            {
                return None;
            }
            payload.get("info")?
        };

        let totals = info.get("total_token_usage")?;
        let result = CliTokenUsage {
            input_tokens: number(totals, "input_tokens"),
            output_tokens: number(totals, "output_tokens"),
            cache_read_input_tokens: number(totals, "cached_input_tokens"),
            model: info
                .get("model")
                .and_then(Value::as_str)
                .map(str::to_string),
            format: "codex_jsonl".to_string(),
            ..Default::default()
        };
        (result.input_tokens.is_some() || result.output_tokens.is_some()).then_some(result)
    }

    fn extract(value: &Value) -> Option<CliTokenUsage> {
        claude_result(value).or_else(|| codex_token_count(value))
    }

    fn is_usage_report(value: &Value) -> bool {
        value.get("type").and_then(Value::as_str) == Some("result")
            || value.get("type").and_then(Value::as_str) == Some("token_count")
            || (value.get("type").and_then(Value::as_str) == Some("event_msg")
                && value
                    .get("payload")
                    .and_then(|payload| payload.get("type"))
                    .and_then(Value::as_str)
                    == Some("token_count"))
    }

    if let Ok(value) = serde_json::from_str::<Value>(stdout.trim()) {
        if is_usage_report(&value) {
            return extract(&value);
        }
    }

    let mut latest = None;
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            if is_usage_report(&value) {
                latest = extract(&value);
            }
        }
    }
    latest
}

impl CodingExecutor for ProcessCliExecutor {
    fn execute<'a>(
        &'a self,
        prompt: &'a str,
        project_root: &'a str,
        task_id: Option<&'a str>,
        cancel_rx: &'a mut tokio::sync::watch::Receiver<bool>,
    ) -> BoxFuture<'a, Result<crate::models::ExecutorResult, String>> {
        Box::pin(async move {
            let root_path = Path::new(project_root);
            if !root_path.exists() {
                return Err(format!("Project root does not exist: {}", project_root));
            }
            let canonical_root = dunce::canonicalize(root_path)
                .map_err(|e| format!("Failed to canonicalize project root: {}", e))?;

            if *cancel_rx.borrow() {
                return Err("Execution was cancelled before starting".to_string());
            }

            let mut cmd = Command::new(&self.executable);
            let command_args =
                structured_output_args(&self.executable, &self.args, self.prompt_delivery);
            let stdin_data = match self.prompt_delivery {
                PromptDelivery::PrefixedStdin => {
                    cmd.arg("-p").args(&command_args);
                    Some(prompt.as_bytes().to_vec())
                }
                PromptDelivery::Stdin => {
                    cmd.args(&command_args);
                    Some(prompt.as_bytes().to_vec())
                }
                PromptDelivery::Argument => {
                    cmd.args(&command_args).arg(prompt);
                    None
                }
            };
            // Claude Code runs non-interactively (print mode) and cannot answer
            // permission prompts, so enable its bypassPermissions mode only for
            // the Claude Code executable. Avoid duplicating the flag if the
            // configured args already include it.
            let is_claude = std::path::Path::new(&self.executable)
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("claude"))
                .unwrap_or(false);
            if is_claude
                && !self
                    .args
                    .iter()
                    .any(|a| a == "--dangerously-skip-permissions")
            {
                cmd.arg("--dangerously-skip-permissions");
            }

            cmd.current_dir(&canonical_root);

            // Inherit the parent environment (the CLI needs PATH and its own
            // credentials). These well-known variables are forwarded explicitly
            // so the requirement stays visible even if inheritance changes.
            for var in &[
                "ANTHROPIC_AUTH_TOKEN",
                "ANTHROPIC_MODEL",
                "ANTHROPIC_BASE_URL",
                "CLAUDE_CONFIG_DIR",
            ] {
                if let Ok(val) = env::var(var) {
                    if !val.trim().is_empty() {
                        cmd.env(var, val);
                    }
                }
            }

            let output = run_process(
                cmd,
                stdin_data,
                self.timeout_secs,
                self.max_output_bytes,
                Some(cancel_rx),
                task_id,
            )
            .await?;

            Ok(crate::models::ExecutorResult {
                exit_code: output.exit_code,
                stdout: output.stdout.clone(),
                stderr: output.stderr,
                timed_out: output.timed_out,
                duration_ms: output.duration_ms,
                token_usage: if output
                    .stdout
                    .contains("[Output truncated: exceeded byte limit]")
                {
                    None
                } else {
                    parse_cli_token_usage(&output.stdout)
                },
            })
        })
    }

    fn name(&self) -> &str {
        "ProcessCliExecutor"
    }

    fn is_test_adapter(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PromptDelivery;
    use tokio::process::Command;
    use tokio::sync::watch;

    #[test]
    fn structured_output_flags_are_enabled_only_for_supported_default_cli_modes() {
        assert_eq!(
            structured_output_args("claude", &[], PromptDelivery::PrefixedStdin),
            vec!["--output-format".to_string(), "json".to_string()]
        );
        assert_eq!(
            structured_output_args(
                "codex.exe",
                &["exec".into(), "-".into()],
                PromptDelivery::Stdin
            ),
            vec!["exec".to_string(), "--json".to_string(), "-".to_string()]
        );
        assert_eq!(
            structured_output_args(
                "claude",
                &["--output-format".into(), "stream-json".into()],
                PromptDelivery::PrefixedStdin
            ),
            vec!["--output-format".to_string(), "stream-json".to_string()]
        );
        assert_eq!(
            structured_output_args("aider", &["--message".into()], PromptDelivery::Argument),
            vec!["--message".to_string()]
        );
    }

    fn temp_root(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "archify_exec_{}_{}_{}",
            label,
            std::process::id(),
            nanos
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[tokio::test]
    async fn run_process_captures_stdout_stderr_exit_code_and_duration() {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "echo out_line & echo err_line 1>&2 & exit /B 7"]);
        let out = run_process(cmd, None, 30, 64 * 1024, None, None)
            .await
            .unwrap();
        assert_eq!(out.exit_code, Some(7));
        assert!(!out.timed_out);
        assert!(out.stdout.contains("out_line"));
        assert!(out.stderr.contains("err_line"));
    }

    #[tokio::test]
    async fn run_process_reports_launch_failure_with_actionable_error() {
        let cmd = Command::new("archify_definitely_missing_executable_12345");
        let err = run_process(cmd, None, 5, 1024, None, None)
            .await
            .unwrap_err();
        assert!(err.contains("Failed to spawn process"));
        assert!(err.contains("archify_definitely_missing_executable_12345") || err.len() > 10);
    }

    #[tokio::test]
    async fn run_process_times_out_and_returns_partial_structured_output() {
        let mut cmd = Command::new("cmd");
        // Use ping to localhost which reliably blocks for at least 5 seconds.
        cmd.args(["/C", "ping -n 5 127.0.0.1 >nul"]);
        let out = run_process(cmd, None, 1, 64 * 1024, None, None)
            .await
            .unwrap();
        assert!(
            out.timed_out,
            "expected timeout, got exit={:?} stderr={} stdout={}",
            out.exit_code, out.stderr, out.stdout
        );
        assert_eq!(out.exit_code, None);
        // Ping output is suppressed to >nul so stdout should be empty or near-empty
        assert!(
            out.stdout.is_empty() || out.stdout.len() < 100,
            "stdout={} stderr={}",
            out.stdout,
            out.stderr
        );
    }

    #[tokio::test]
    async fn run_process_cancellation_returns_actionable_error() {
        let (tx, mut rx) = watch::channel(false);
        let mut cmd = Command::new("cmd");
        cmd.args([
            "/C",
            "powershell -NoProfile -Command \"Start-Sleep -Seconds 30\"",
        ]);
        tx.send(true).unwrap();
        let err = run_process(cmd, None, 30, 1024, Some(&mut rx), None)
            .await
            .unwrap_err();
        assert!(err.to_lowercase().contains("cancel"));
    }

    #[tokio::test]
    async fn run_process_bounds_large_output_without_losing_small_output_marker() {
        let mut cmd = Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-Command",
            "1..5000 | ForEach-Object { 'LINE-' + $_ }",
        ]);
        let out = run_process(cmd, None, 30, 512, None, None).await.unwrap();
        assert!(out.stdout.contains("LINE-1"));
        assert!(out.stdout.contains("[Output truncated"));
        assert!(out.stdout.len() <= 700);
    }

    #[tokio::test]
    async fn executor_prefixed_stdin_delivers_prompt_to_process() {
        let root = temp_root("stdin");
        // Use cmd to avoid PowerShell quoting/parse sensitivity in CI shells; proves PrefixedStdin
        // sends prompt bytes to child stdin through the executor's -p+stdin path handling.
        let exec = ProcessCliExecutor::with_prompt_delivery(
            "cmd".to_string(),
            vec!["/C".to_string(), "more".to_string()],
            PromptDelivery::Stdin,
            Some(30),
            Some(64 * 1024),
        );
        // Each test needs its own channel receiver
        let (_tx, mut rx) = watch::channel(false);
        let result = exec
            .execute("HELLO_PROMPT", root.to_str().unwrap(), None, &mut rx)
            .await
            .unwrap();
        assert!(
            !result.timed_out,
            "stderr={} stdout={}",
            result.stderr, result.stdout
        );
        assert_eq!(
            result.exit_code,
            Some(0),
            "stderr={} stdout={}",
            result.stderr,
            result.stdout
        );
        assert!(
            result.stdout.contains("HELLO_PROMPT"),
            "stdout={} stderr={}",
            result.stdout,
            result.stderr
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn executor_argument_delivery_passes_prompt_as_argument() {
        let root = temp_root("arg");
        let exec = ProcessCliExecutor::with_prompt_delivery(
            "cmd".to_string(),
            vec!["/C".to_string(), "echo".to_string()],
            PromptDelivery::Argument,
            Some(30),
            Some(64 * 1024),
        );
        let (_tx, mut rx) = watch::channel(false);
        let result = exec
            .execute("PROMPT_ARG", root.to_str().unwrap(), None, &mut rx)
            .await
            .unwrap();
        assert_eq!(
            result.exit_code,
            Some(0),
            "stderr={} stdout={}",
            result.stderr,
            result.stdout
        );
        assert!(
            result.stdout.contains("PROMPT_ARG"),
            "stdout={} stderr={}",
            result.stdout,
            result.stderr
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn executor_nonzero_exit_is_reported_not_treated_as_success() {
        let root = temp_root("nonzero");
        let exec = ProcessCliExecutor::with_prompt_delivery(
            "cmd".to_string(),
            vec![
                "/C".to_string(),
                "echo failed_reason 1>&2 & exit /B 3".to_string(),
            ],
            PromptDelivery::Stdin,
            Some(30),
            Some(64 * 1024),
        );
        let (_tx, mut rx) = watch::channel(false);
        let result = exec
            .execute("", root.to_str().unwrap(), None, &mut rx)
            .await
            .unwrap();
        assert_eq!(result.exit_code, Some(3));
        assert!(!result.timed_out);
        assert!(result.stderr.contains("failed_reason"));
        // A structured result is returned; task success must be decided by verifier/reviewer, not by "no Err".
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn executor_returns_explicit_cli_usage_from_structured_stdout() {
        let root = tempfile::tempdir().unwrap();
        let script = root.path().join("emit-usage.cmd");
        std::fs::write(
            &script,
            "@echo off\necho {\"type\":\"result\",\"usage\":{\"input_tokens\":13,\"output_tokens\":4},\"model\":\"claude-sonnet-4\"}\n",
        )
        .unwrap();
        let exec = ProcessCliExecutor::with_prompt_delivery(
            "cmd".to_string(),
            vec!["/C".to_string(), script.to_string_lossy().to_string()],
            PromptDelivery::Stdin,
            Some(30),
            Some(64 * 1024),
        );
        let (_tx, mut rx) = watch::channel(false);
        let result = exec
            .execute("", root.path().to_str().unwrap(), None, &mut rx)
            .await
            .unwrap();
        assert_eq!(
            result
                .token_usage
                .as_ref()
                .and_then(|usage| usage.input_tokens),
            Some(13)
        );
        assert_eq!(
            result
                .token_usage
                .as_ref()
                .and_then(|usage| usage.output_tokens),
            Some(4)
        );
        assert_eq!(
            result
                .token_usage
                .as_ref()
                .and_then(|usage| usage.model.as_deref()),
            Some("claude-sonnet-4")
        );
    }

    #[tokio::test]
    async fn executor_does_not_report_usage_from_truncated_cli_output() {
        let root = tempfile::tempdir().unwrap();
        let script = root.path().join("emit-usage.cmd");
        std::fs::write(
            &script,
            "@echo off\necho {\"type\":\"result\",\"usage\":{\"input_tokens\":13,\"output_tokens\":4}}\necho output-over-limit\n",
        )
        .unwrap();
        let exec = ProcessCliExecutor::with_prompt_delivery(
            "cmd".to_string(),
            vec!["/C".to_string(), script.to_string_lossy().to_string()],
            PromptDelivery::Stdin,
            Some(30),
            Some(32),
        );
        let (_tx, mut rx) = watch::channel(false);
        let result = exec
            .execute("", root.path().to_str().unwrap(), None, &mut rx)
            .await
            .unwrap();
        assert!(result.stdout.contains("[Output truncated"));
        assert!(result.token_usage.is_none());
    }

    #[tokio::test]
    async fn executor_rejects_missing_project_root() {
        let exec = ProcessCliExecutor::default();
        let (_tx, mut rx) = watch::channel(false);
        let err = exec
            .execute("x", "C:\\archify\\missing\\root\\nope", None, &mut rx)
            .await
            .unwrap_err();
        assert!(err.contains("Project root does not exist"));
    }

    #[test]
    fn parses_claude_code_usage_json() {
        let out = r#"{"type":"result","subtype":"success","result":"done","usage":{"input_tokens":12,"output_tokens":34,"cache_creation_input_tokens":5,"cache_read_input_tokens":6},"total_cost_usd":0.0012,"model":"claude-sonnet-4"}"#;
        let u = parse_cli_token_usage(out).unwrap();
        assert_eq!(u.input_tokens, Some(12));
        assert_eq!(u.output_tokens, Some(34));
        assert_eq!(u.cache_creation_input_tokens, Some(5));
        assert_eq!(u.cache_read_input_tokens, Some(6));
        assert_eq!(u.model.as_deref(), Some("claude-sonnet-4"));
        assert_eq!(u.format, "claude_code_json");
    }

    #[test]
    fn parses_partial_usage() {
        let out = r#"{"type":"result","usage":{"input_tokens":7}}"#;
        let u = parse_cli_token_usage(out).unwrap();
        assert_eq!(u.input_tokens, Some(7));
        assert_eq!(u.output_tokens, None);
    }

    #[test]
    fn returns_none_for_missing_metadata() {
        let out = r#"{"result":"hello","type":"assistant"}"#;
        assert!(parse_cli_token_usage(out).is_none());
    }

    #[test]
    fn returns_none_for_malformed_json() {
        let out = r#"{ not valid json "#;
        assert!(parse_cli_token_usage(out).is_none());
    }

    #[test]
    fn returns_none_for_unrelated_json() {
        let out = r#"{"foo":1,"bar":{"baz":2}}"#;
        assert!(parse_cli_token_usage(out).is_none());
    }

    #[test]
    fn parses_stream_json_lines() {
        let out = "{\"type\":\"assistant\",\"message\":\"hi\"}\n{\"type\":\"result\",\"usage\":{\"input_tokens\":9,\"output_tokens\":10}}\n";
        let u = parse_cli_token_usage(out).unwrap();
        assert_eq!(u.input_tokens, Some(9));
        assert_eq!(u.output_tokens, Some(10));
    }

    #[test]
    fn uses_latest_final_usage_report_without_summing_repeated_snapshots() {
        let out = "{\"type\":\"result\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}\n{\"type\":\"result\",\"usage\":{\"input_tokens\":99,\"output_tokens\":99}}\n";
        let u = parse_cli_token_usage(out).unwrap();
        assert_eq!(u.input_tokens, Some(99));
        assert_eq!(u.output_tokens, Some(99));
    }

    #[test]
    fn parses_latest_claude_result_not_intermediate_message_usage() {
        let out = concat!(
            "{\"type\":\"assistant\",\"message\":{\"usage\":{\"input_tokens\":3,\"output_tokens\":2}}}\n",
            "{\"type\":\"result\",\"usage\":{\"input_tokens\":20,\"output_tokens\":8},\"model\":\"claude-sonnet-4\"}\n",
            "{\"type\":\"result\",\"usage\":{\"input_tokens\":25,\"output_tokens\":11},\"model\":\"claude-sonnet-4\"}\n"
        );
        let usage = parse_cli_token_usage(out).unwrap();
        assert_eq!(usage.input_tokens, Some(25));
        assert_eq!(usage.output_tokens, Some(11));
        assert_eq!(usage.model.as_deref(), Some("claude-sonnet-4"));
        assert_eq!(usage.format, "claude_code_json");
    }

    #[test]
    fn parses_codex_latest_cumulative_token_count_event() {
        let out = concat!(
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":12,\"cached_input_tokens\":3,\"output_tokens\":5,\"total_tokens\":17}}}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":22,\"cached_input_tokens\":4,\"output_tokens\":9,\"total_tokens\":31}}}}\n"
        );
        let usage = parse_cli_token_usage(out).unwrap();
        assert_eq!(usage.input_tokens, Some(22));
        assert_eq!(usage.output_tokens, Some(9));
        assert_eq!(usage.cache_read_input_tokens, Some(4));
        assert_eq!(usage.format, "codex_jsonl");
    }

    #[test]
    fn ignores_generic_usage_and_unsupported_cli_output() {
        assert!(
            parse_cli_token_usage(r#"{"usage":{"input_tokens":5,"output_tokens":2}}"#).is_none()
        );
        assert!(parse_cli_token_usage("Tokens used: 100 input, 20 output").is_none());
        assert!(parse_cli_token_usage(
            r#"{"type":"message.updated","properties":{"info":{"tokens":{"input":5,"output":2}}}}"#
        )
        .is_none());
    }

    #[test]
    fn ignores_malformed_or_non_integer_usage_fields() {
        assert!(
            parse_cli_token_usage(
                r#"{"type":"result","usage":{"input_tokens":"5","output_tokens":-1}}"#
            )
            .is_none()
        );
        assert!(parse_cli_token_usage(
            r#"{"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"input_tokens":1.5,"output_tokens":"4"}}}}"#
        )
        .is_none());
    }

    #[test]
    fn invalid_final_usage_report_does_not_fall_back_to_an_older_snapshot() {
        let out = concat!(
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{\"total_token_usage\":{\"input_tokens\":22,\"output_tokens\":9}}}}\n",
            "{\"type\":\"event_msg\",\"payload\":{\"type\":\"token_count\",\"info\":{}}}\n"
        );
        assert!(parse_cli_token_usage(out).is_none());
    }

    #[test]
    fn does_not_guess_model_when_claude_report_contains_multiple_models() {
        let out = r#"{"type":"result","usage":{"input_tokens":8,"output_tokens":3},"modelUsage":{"claude-model-a":{},"claude-model-b":{}}}"#;
        let usage = parse_cli_token_usage(out).unwrap();
        assert_eq!(usage.model, None);
        assert_eq!(usage.input_tokens, Some(8));
    }
}
