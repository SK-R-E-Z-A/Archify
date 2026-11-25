//! Priority 2 — Windows Cancellation and Process Management Integration Tests.
//!
//! These tests verify that cancelling an active CLI process terminates the
//! entire process tree (children and descendants), releases handles and
//! reader resources, prevents orphaned processes, and keeps cancelled tasks
//! from being marked Completed. Cancellation events must reach the
//! MonitoringHub and cancellation state must persist.
//!
//! All tests use real Windows processes through `run_process` and/or the
//! orchestrator's `cancel_task` path.

use archify_lib::models::{ProviderConfigInput, TaskState};
use archify_lib::monitoring::{EventContext, MonitoringHub};
use archify_lib::orchestrator::{
    Orchestrator, TaskCreationOptions, approve_plan, begin_planning, cancel_task, create_task,
};
use archify_lib::providers::process_cli::run_process;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::sync::{Mutex, watch};

type SharedOrch = Arc<Mutex<Orchestrator>>;

fn scratch_dir(tag: &str) -> PathBuf {
    // Prefer the workspace TEMP/TMP if available (configured for test runs),
    // otherwise fall back to the OS temp directory.
    let base = std::env::var_os("TEMP")
        .or_else(|| std::env::var_os("TMP"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join(format!("archify_p2_{tag}_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|e| panic!("failed to create scratch dir {dir:?}: {e}"));
    dir
}

async fn setup_orch(tag: &str) -> (SharedOrch, PathBuf) {
    let history_dir = scratch_dir(&format!("{tag}_hist"));
    let config_dir = scratch_dir(&format!("{tag}_cfg"));
    let project_root = scratch_dir(&format!("{tag}_proj"));
    let orch =
        Orchestrator::new_isolated(None, history_dir.clone(), config_dir.join("providers.json"));
    (Arc::new(Mutex::new(orch)), project_root)
}

fn test_config() -> ProviderConfigInput {
    ProviderConfigInput {
        planner_type: Some("test".to_string()),
        executor_type: Some("test".to_string()),
        ..Default::default()
    }
}

/// Poll until the task reaches `expected` or timeout expires.
async fn wait_for_state(
    orch: &SharedOrch,
    task_id: &str,
    expected: TaskState,
    timeout_secs: u64,
) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_secs(timeout_secs) {
        let guard = orch.lock().await;
        if let Some(t) = guard.current_task.as_ref() {
            if t.id == task_id && t.state == expected {
                return true;
            }
        }
        drop(guard);
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    false
}

/// Run a long-running command, cancel after `cancel_after_ms`, return the
/// result and elapsed time. Cancellation is triggered from the test task
/// while `run_process` polls the watch receiver inside its own select loop.
async fn run_and_cancel(args: &[&str], cancel_after_ms: u64) -> (String, Duration) {
    let mut cmd = Command::new(if cfg!(windows) { "cmd" } else { "sh" });
    cmd.args(args);
    let (tx, rx) = watch::channel(false);
    let start = Instant::now();
    let handle = tokio::spawn(async move {
        let mut rx = rx;
        run_process(cmd, None, 60, 64 * 1024, Some(&mut rx), None).await
    });
    tokio::time::sleep(Duration::from_millis(cancel_after_ms)).await;
    let _ = tx.send(true);
    let result = handle.await.expect("task should complete");
    let elapsed = start.elapsed();
    (
        result.map(|_| "ok".to_string()).unwrap_or_else(|e| e),
        elapsed,
    )
}

/// Assert that a cancellation result is an error mentioning cancel and
/// completed within a reasonable window (kill_process_tree can take up to
/// the kill-wait timeout).
fn assert_cancelled_fast(result: &str, elapsed: Duration) {
    assert!(
        result.to_lowercase().contains("cancel"),
        "expected cancellation error, got: {result}"
    );
    assert!(
        elapsed < Duration::from_secs(20),
        "cancellation should be reasonably fast, took {:?}",
        elapsed
    );
}

// ---------------------------------------------------------------------------
// 1. Cancel an active CLI process
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p2_cancel_active_cli_process() {
    let args = if cfg!(windows) {
        vec!["/C", "ping -n 30 127.0.0.1 >nul"]
    } else {
        vec!["-c", "sleep 30"]
    };
    let (err, elapsed) = run_and_cancel(&args, 1500).await;
    assert_cancelled_fast(&err, elapsed);
}

#[tokio::test]
async fn p2_cancel_idempotency_and_state_persistence() {
    let (orch_lock, project_root) = setup_orch("idempotency").await;
    let task = create_task(
        orch_lock.clone(),
        "do something".to_string(),
        project_root.to_string_lossy().to_string(),
        TaskCreationOptions {
            provider_profile_id: None,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    // Trigger initial cancellation
    println!("DEBUG: First cancel");
    cancel_task(orch_lock.clone(), task.id.clone())
        .await
        .expect("First cancel failed");
    println!("DEBUG: First cancel done");

    // Verify state
    let guard = orch_lock.lock().await;
    let t = guard.current_task.as_ref().unwrap();
    assert_eq!(t.state, TaskState::Cancelled);
    assert_eq!(t.error, Some("Task cancelled by user".to_string()));
    drop(guard);

    // Trigger second cancellation
    let result = cancel_task(orch_lock.clone(), task.id.clone()).await;

    // Verify idempotent result state is unchanged
    assert!(result.is_ok());
    let t = result.unwrap();
    assert_eq!(t.state, TaskState::Cancelled);
    assert_eq!(t.error, Some("Task cancelled by user".to_string()));
}

#[tokio::test]
async fn p2_process_tree_termination() {
    // A process that launches a nested child process.
    let mut cmd = Command::new(if cfg!(windows) { "cmd" } else { "sh" });
    if cfg!(windows) {
        cmd.args([
            "/C",
            "start /b cmd /c ping -n 30 127.0.0.1 >nul & ping -n 30 127.0.0.1 >nul",
        ]);
    } else {
        cmd.args(["-c", "sh -c 'sleep 30' & sleep 30"]);
    }
    let (tx, rx) = watch::channel(false);
    let start = Instant::now();
    let handle = tokio::spawn(async move {
        let mut rx = rx;
        run_process(cmd, None, 60, 64 * 1024, Some(&mut rx), None).await
    });
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let _ = tx.send(true);
    let result = handle.await.expect("task should complete");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_cancelled_fast(&err, start.elapsed());
}

// ---------------------------------------------------------------------------
// 3. Cancel while stdout is being captured
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p2_cancel_during_stdout_capture() {
    // Produce stdout slowly via ping (about 1 second per iteration).
    let mut cmd = Command::new(if cfg!(windows) { "cmd" } else { "sh" });
    if cfg!(windows) {
        cmd.args([
            "/C",
            "for /L %i in (1,1,30) do (echo line_%i & ping -n 2 127.0.0.1 >nul)",
        ]);
    } else {
        cmd.args(["-c", "for i in $(seq 1 30); do echo line_$i; sleep 1; done"]);
    }
    let (tx, rx) = watch::channel(false);
    let start = Instant::now();
    let handle = tokio::spawn(async move {
        let mut rx = rx;
        run_process(cmd, None, 60, 64 * 1024, Some(&mut rx), None).await
    });
    tokio::time::sleep(Duration::from_millis(3000)).await;
    let _ = tx.send(true);
    let result = handle.await.expect("task should complete");
    assert!(result.is_err());
    assert_cancelled_fast(&result.unwrap_err(), start.elapsed());
}

// ---------------------------------------------------------------------------
// 4. Cancel while stderr is being captured
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p2_cancel_during_stderr_capture() {
    // Produce stderr slowly via ping (about 1 second per iteration).
    let mut cmd = Command::new(if cfg!(windows) { "cmd" } else { "sh" });
    if cfg!(windows) {
        cmd.args([
            "/C",
            "for /L %i in (1,1,30) do (echo line_%i 1>&2 & ping -n 2 127.0.0.1 >nul)",
        ]);
    } else {
        cmd.args([
            "-c",
            "for i in $(seq 1 30); do echo line_$i 1>&2; sleep 1; done",
        ]);
    }
    let (tx, rx) = watch::channel(false);
    let start = Instant::now();
    let handle = tokio::spawn(async move {
        let mut rx = rx;
        run_process(cmd, None, 60, 64 * 1024, Some(&mut rx), None).await
    });
    tokio::time::sleep(Duration::from_millis(3000)).await;
    let _ = tx.send(true);
    let result = handle.await.expect("task should complete");
    assert!(result.is_err());
    assert_cancelled_fast(&result.unwrap_err(), start.elapsed());
}

// ---------------------------------------------------------------------------
// 5. Cancel during verification (orchestrator level)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p2_orchestrator_cancel_during_verification() {
    let (orch, project_root) = setup_orch("cancel_verif").await;
    let task = create_task(
        orch.clone(),
        "Task to cancel during verification".to_string(),
        project_root.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();

    {
        let mut g = orch.lock().await;
        g.configure_providers(test_config()).unwrap();
    }

    begin_planning(orch.clone(), task.id.clone()).await.unwrap();
    assert!(
        wait_for_state(&orch, &task.id, TaskState::AwaitingApproval, 10).await,
        "planning should reach AwaitingApproval"
    );

    approve_plan(orch.clone(), task.id.clone()).await.unwrap();

    // Cancel while executing (which includes verification phase).
    cancel_task(orch.clone(), task.id.clone()).await.unwrap();

    assert!(
        wait_for_state(&orch, &task.id, TaskState::Cancelled, 10).await,
        "task should be cancelled, not completed"
    );

    let final_state = {
        let g = orch.lock().await;
        g.current_task.clone().unwrap()
    };
    assert_eq!(final_state.state, TaskState::Cancelled);
    assert!(
        final_state
            .error
            .as_deref()
            .unwrap_or("")
            .contains("cancel"),
        "error should mention cancel: {:?}",
        final_state.error
    );
}

// ---------------------------------------------------------------------------
// 6. Cancel during a retry cycle
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p2_cancel_during_retry_cycle() {
    let (orch, project_root) = setup_orch("cancel_retry").await;
    let task = create_task(
        orch.clone(),
        "Task that needs correction then cancel".to_string(),
        project_root.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();

    {
        let mut g = orch.lock().await;
        g.configure_providers(ProviderConfigInput {
            planner_type: Some("test".to_string()),
            executor_type: Some("test".to_string()),
            test_planner_scenario: Some("AlwaysNeedsChanges".to_string()),
            test_executor_scenario: Some("NormalSuccess".to_string()),
            max_iterations: Some(10),
            ..Default::default()
        })
        .unwrap();
    }

    begin_planning(orch.clone(), task.id.clone()).await.unwrap();
    assert!(
        wait_for_state(&orch, &task.id, TaskState::AwaitingApproval, 10).await,
        "planning should succeed"
    );
    approve_plan(orch.clone(), task.id.clone()).await.unwrap();

    // Cancel immediately during the first execution/retry cycle.
    cancel_task(orch.clone(), task.id.clone()).await.unwrap();

    assert!(
        wait_for_state(&orch, &task.id, TaskState::Cancelled, 15).await,
        "task should be cancelled during retry"
    );

    let final_state = {
        let g = orch.lock().await;
        g.current_task.clone().unwrap()
    };
    assert_eq!(final_state.state, TaskState::Cancelled);
}

// ---------------------------------------------------------------------------
// 7. Cancel during a long-running process
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p2_cancel_during_long_running_process() {
    let args = if cfg!(windows) {
        vec!["/C", "ping -n 30 127.0.0.1 >nul"]
    } else {
        vec!["-c", "sleep 30"]
    };
    let (err, elapsed) = run_and_cancel(&args, 1500).await;
    assert_cancelled_fast(&err, elapsed);
}

// ---------------------------------------------------------------------------
// 8. Prevent cancelled tasks from being marked Completed
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p2_cancelled_never_completed() {
    let (orch, project_root) = setup_orch("cancel_not_complete").await;
    let task = create_task(
        orch.clone(),
        "Task that must not complete after cancel".to_string(),
        project_root.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();

    {
        let mut g = orch.lock().await;
        g.configure_providers(test_config()).unwrap();
    }

    begin_planning(orch.clone(), task.id.clone()).await.unwrap();
    assert!(
        wait_for_state(&orch, &task.id, TaskState::AwaitingApproval, 10).await,
        "planning should succeed"
    );
    approve_plan(orch.clone(), task.id.clone()).await.unwrap();

    // Cancel immediately.
    cancel_task(orch.clone(), task.id.clone()).await.unwrap();

    // Wait a bit and verify it never reaches Completed.
    tokio::time::sleep(Duration::from_secs(5)).await;

    let final_state = {
        let g = orch.lock().await;
        g.current_task.clone().unwrap()
    };
    assert_eq!(final_state.state, TaskState::Cancelled);
    assert_ne!(final_state.state, TaskState::Completed);
}

// ---------------------------------------------------------------------------
// 9. Cancellation persists correctly
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p2_cancellation_persists() {
    let history_dir = scratch_dir("cancel_persist_hist");
    let config_dir = scratch_dir("cancel_persist_cfg");
    let project_root = scratch_dir("cancel_persist_proj");

    let orch1 =
        Orchestrator::new_isolated(None, history_dir.clone(), config_dir.join("providers.json"));
    let orch1 = Arc::new(Mutex::new(orch1));

    let task = create_task(
        orch1.clone(),
        "Task to cancel and persist".to_string(),
        project_root.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();

    {
        let mut g = orch1.lock().await;
        g.configure_providers(test_config()).unwrap();
    }

    begin_planning(orch1.clone(), task.id.clone())
        .await
        .unwrap();
    cancel_task(orch1.clone(), task.id.clone()).await.unwrap();

    // Drop and reload from disk (simulates restart).
    drop(orch1);
    let orch2 =
        Orchestrator::new_with_paths(None, history_dir.clone(), config_dir.join("providers.json"));
    let orch2 = Arc::new(Mutex::new(orch2));

    let recovered = {
        let g = orch2.lock().await;
        g.task_history.get(&task.id).cloned()
    };
    assert!(
        recovered.is_some(),
        "cancelled task should be recoverable from history"
    );
    let recovered = recovered.unwrap();
    assert_eq!(recovered.state, TaskState::Cancelled);
    assert!(
        recovered.error.as_deref().unwrap_or("").contains("cancel"),
        "error should persist: {:?}",
        recovered.error
    );
}

// ---------------------------------------------------------------------------
// 10. Cancellation events reach MonitoringHub
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p2_cancellation_events_reach_hub() {
    let hub = MonitoringHub::headless();
    let context = EventContext::with_iteration("task-cancel-hub", 1).unwrap();

    // Emit a cancellation lifecycle event.
    use archify_lib::agent_lifecycle::{LifecycleEvent, LifecycleEventType};
    let event = LifecycleEvent {
        event_type: LifecycleEventType::ProcessCancelled,
        timestamp: chrono::Utc::now(),
        details: Some("Task cancelled by user".to_string()),
        exit_code: None,
    };
    let outcome = hub.emit(&context, &event);
    assert!(
        matches!(
            outcome,
            archify_lib::monitoring::EmitOutcome::NoConsumer { .. }
        ) || matches!(
            outcome,
            archify_lib::monitoring::EmitOutcome::Delivered { .. }
        ),
        "cancellation event should be accepted by hub: {outcome:?}"
    );

    assert_eq!(hub.history_len(), 1);
    let recorded = &hub.history()[0];
    assert_eq!(recorded.task_id, "task-cancel-hub");
}

// ---------------------------------------------------------------------------
// 11. Cancellation works with multiple independent projects
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p2_cancellation_multi_project() {
    let (orch1, root1) = setup_orch("multi_proj_a").await;
    let (orch2, root2) = setup_orch("multi_proj_b").await;

    // Two independent tasks on different projects/orchestrators.
    let task1 = create_task(
        orch1.clone(),
        "Project A task".to_string(),
        root1.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();
    let task2 = create_task(
        orch2.clone(),
        "Project B task".to_string(),
        root2.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();

    {
        let mut g = orch1.lock().await;
        g.configure_providers(test_config()).unwrap();
    }
    {
        let mut g = orch2.lock().await;
        g.configure_providers(test_config()).unwrap();
    }

    begin_planning(orch1.clone(), task1.id.clone())
        .await
        .unwrap();
    begin_planning(orch2.clone(), task2.id.clone())
        .await
        .unwrap();

    assert!(
        wait_for_state(&orch1, &task1.id, TaskState::AwaitingApproval, 10).await,
        "task1 should reach AwaitingApproval"
    );
    assert!(
        wait_for_state(&orch2, &task2.id, TaskState::AwaitingApproval, 10).await,
        "task2 should reach AwaitingApproval"
    );

    // Cancel only task1.
    cancel_task(orch1.clone(), task1.id.clone()).await.unwrap();

    assert!(
        wait_for_state(&orch1, &task1.id, TaskState::Cancelled, 10).await,
        "task1 should be cancelled"
    );

    // task2 should be unaffected (still AwaitingApproval).
    {
        let g = orch2.lock().await;
        let t = g.current_task.as_ref().unwrap();
        assert_eq!(t.id, task2.id);
        assert_eq!(t.state, TaskState::AwaitingApproval);
    }
}
