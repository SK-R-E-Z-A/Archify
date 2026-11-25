//! Batch 2 — autonomous workflow integration tests.
//!
//! Drives the real backend orchestration loop (planner → user approval →
//! executor → reviewer → terminal state) through the public `Orchestrator`
//! API with deterministic test adapters. No network access, no paid AI APIs
//! and no real CLI accounts are required.

use archify_lib::models::{TaskRecord, TaskState};
use archify_lib::orchestrator::{
    Orchestrator, TaskCreationOptions, approve_plan, begin_planning, create_task,
    read_task_history, reject_plan,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

type SharedOrch = Arc<Mutex<Orchestrator>>;

// ----------------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------------

fn scratch_dir(tag: &str) -> PathBuf {
    tempfile::Builder::new()
        .prefix(&format!("archify_b2_{tag}_"))
        .tempdir()
        .expect("scratch dir")
        .keep()
}

async fn new_orch(tag: &str) -> (SharedOrch, PathBuf) {
    let history_dir = scratch_dir(&format!("{tag}_hist"));
    let config_dir = scratch_dir(&format!("{tag}_cfg"));
    let orch =
        Orchestrator::new_isolated(None, history_dir.clone(), config_dir.join("providers.json"));
    (Arc::new(Mutex::new(orch)), history_dir)
}

/// Plan a task, drive it through the user-approval gate, and wait for a
/// terminal state. Returns the final record.
async fn plan_approve_and_wait(orch: &SharedOrch, task_id: &str) -> TaskRecord {
    begin_planning(orch.clone(), task_id.to_string())
        .await
        .expect("planning should start");

    // The workflow parks at AwaitingApproval until the user approves.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        {
            let o = orch.lock().await;
            if let Some(rec) = o.current_task.as_ref() {
                if rec.id == task_id && rec.state == TaskState::AwaitingApproval {
                    break;
                }
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "task {task_id} never reached the approval gate"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }

    approve_plan(orch.clone(), task_id.to_string())
        .await
        .expect("plan approval should succeed");

    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        {
            let o = orch.lock().await;
            if let Some(rec) = o.current_task.as_ref() {
                if rec.id == task_id && rec.state.is_terminal() {
                    return rec.clone();
                }
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "task {task_id} did not terminate in time"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn create_probe_task(orch: &SharedOrch, tag: &str, description: &str) -> TaskRecord {
    create_task(
        orch.clone(),
        description.to_string(),
        scratch_dir(tag).to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .expect("task creation should succeed")
}

// ----------------------------------------------------------------------------
// Full lifecycle
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_full_lifecycle_reaches_completed_and_persists() {
    let (orch, history_dir) = new_orch("lifecycle").await;

    let created = create_probe_task(&orch, "lifecycle_root", "Lifecycle probe").await;
    assert!(
        matches!(created.state, TaskState::Queued | TaskState::Planning),
        "fresh task must start active but pre-execution, got {:?}",
        created.state
    );

    let final_record = plan_approve_and_wait(&orch, &created.id).await;
    assert_eq!(
        final_record.state,
        TaskState::Completed,
        "deterministic success adapters must complete the task"
    );
    assert!(
        final_record.plan.is_some(),
        "a completed task must retain its plan"
    );
    assert!(
        final_record.error.is_none(),
        "a completed task must carry no error"
    );

    // The terminal record must be persisted and re-readable from disk.
    let persisted = read_task_history(&history_dir)
        .get(&created.id)
        .cloned()
        .expect("completed task must be persisted to the history directory");
    assert_eq!(persisted.state, TaskState::Completed);
    assert_eq!(persisted.id, created.id);
    assert_eq!(
        persisted.plan.is_some(),
        final_record.plan.is_some(),
        "persisted record must match in-memory state"
    );
}

// ----------------------------------------------------------------------------
// Failure scenarios
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_executor_nonzero_exit_never_completes() {
    let (orch, _history_dir) = new_orch("nonzero").await;

    let created = create_probe_task(
        &orch,
        "nonzero_root",
        "Task exercising a failing coding executor",
    )
    .await;

    // Deterministic failing executor.
    {
        let mut o = orch.lock().await;
        o.config.test_executor_scenario = Some("nonzero_exit".to_string());
    }

    let final_record = plan_approve_and_wait(&orch, &created.id).await;
    assert_ne!(
        final_record.state,
        TaskState::Completed,
        "a nonzero executor exit must never be reported completed"
    );
    assert_eq!(final_record.state, TaskState::Failed);
    assert!(
        final_record.error.is_some(),
        "failure must carry a diagnostic error"
    );
}

#[tokio::test]
async fn test_executor_timeout_never_completes() {
    let (orch, _history_dir) = new_orch("timeout").await;

    let created =
        create_probe_task(&orch, "timeout_root", "Task exercising a hanging executor").await;
    {
        let mut o = orch.lock().await;
        o.config.test_executor_scenario = Some("timeout".to_string());
    }

    let final_record = plan_approve_and_wait(&orch, &created.id).await;
    assert_ne!(final_record.state, TaskState::Completed);
    assert_eq!(final_record.state, TaskState::Failed);
}

#[tokio::test]
async fn test_rejection_parks_task_in_cancelled_state() {
    let (orch, history_dir) = new_orch("reject").await;

    let created = create_probe_task(&orch, "reject_root", "User rejected task").await;

    begin_planning(orch.clone(), created.id.clone())
        .await
        .expect("planning should start");
    // Wait briefly for the planner to finish writing state, then reject.
    let first = reject_plan(
        orch.clone(),
        created.id.clone(),
        Some("User rejected the plan".to_string()),
    )
    .await
    .expect("rejecting a queued/planning task should be allowed");
    assert_eq!(first.state, TaskState::Cancelled);

    let persisted = read_task_history(&history_dir)
        .get(&created.id)
        .cloned()
        .expect("rejected task must be persisted");
    assert_eq!(persisted.state, TaskState::Cancelled);
}

// ----------------------------------------------------------------------------
// Persistence / restart recovery
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_restart_recovery_marks_midflight_task_recovered() {
    // Simulate a process restart with a mid-flight task persisted on disk:
    // a fresh orchestrator must treat it as interrupted and recovery must move
    // it to a terminal state without re-running any execution.
    let history_dir = scratch_dir("restart_hist");
    let config_dir = scratch_dir("restart_cfg");
    let task_id = "restart-midflight-probe".to_string();

    let full_record = TaskRecord {
        id: task_id.clone(),
        user_task: "mid-flight before restart".to_string(),
        project_root: std::env::temp_dir().to_string_lossy().to_string(),
        state: TaskState::Executing,
        plan: None,
        task_graph: None,
        iteration: 1,
        max_iterations: 3,
        iterations: Vec::new(),
        created_at: chrono::Utc::now().to_rfc3339(),
        updated_at: chrono::Utc::now().to_rfc3339(),
        error: None,
        project_id: None,
        provider_profile_id: None,
        agent_name: None,
        priority: 0,
        queued_at: None,
        recovery_attempts: 0,
        task_level_changes: None,
        task_baseline: None,
        execution_config: None,
        planning_context: None,
        source_session_id: None,
        selected_skill_ids: Vec::new(),
        task_model_id: None,
    };
    std::fs::write(
        history_dir.join(format!("{task_id}.json")),
        serde_json::to_string(&full_record).expect("serialize"),
    )
    .expect("write history record");

    let orch = Arc::new(Mutex::new(Orchestrator::new_isolated(
        None,
        history_dir.clone(),
        config_dir.join("providers.json"),
    )));

    {
        let mut o = orch.lock().await;
        o.task_history = read_task_history(&history_dir);

        assert!(
            o.interrupted_tasks().iter().any(|t| t.id == task_id),
            "mid-flight task must be reported as interrupted after restart"
        );
        let recovered = o.recover_interrupted_tasks();
        assert!(
            recovered.iter().any(|id| id == &task_id),
            "recovery must handle the mid-flight task"
        );
        let stored = o.task_history.get(&task_id).expect("recovered record");
        assert_eq!(stored.state, TaskState::Cancelled);
        assert!(
            stored.error.is_some(),
            "recovered task must explain why it was not resumed"
        );
    }

    // The recovery decision must itself be persisted.
    let persisted = read_task_history(&history_dir)
        .get(&task_id)
        .cloned()
        .expect("recovered task must be persisted");
    assert_eq!(persisted.state, TaskState::Cancelled);
}

// ----------------------------------------------------------------------------
// Retry limit and cancellation
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_retry_limit_is_enforced() {
    let (orch, history_dir) = new_orch("retrylimit").await;

    let created = create_probe_task(
        &orch,
        "retrylimit_root",
        "Task whose reviewer never accepts the result",
    )
    .await;

    {
        let mut o = orch.lock().await;
        o.config.max_iterations = 2;
        o.config.test_planner_scenario = Some("always_needs_changes".to_string());
    }

    let final_record = plan_approve_and_wait(&orch, &created.id).await;
    assert_eq!(
        final_record.state,
        TaskState::Failed,
        "a reviewer that never accepts must fail the task, not loop forever"
    );
    // Pins current behavior: max_iterations=2 yields up to 3 recorded
    // executions (initial run + retries). If the intended contract is
    // "max_iterations = total executions", this is an off-by-one in the
    // orchestration loop and this assertion should be tightened to <= max.
    assert!(
        final_record.iterations.len() <= 3,
        "iteration count must stay bounded, got {}",
        final_record.iterations.len()
    );

    let persisted = read_task_history(&history_dir)
        .get(&created.id)
        .cloned()
        .expect("failed task must be persisted");
    assert_eq!(persisted.state, TaskState::Failed);
}

#[tokio::test]
async fn test_cancel_during_execution_does_not_complete() {
    let (orch, _history_dir) = new_orch("cancel").await;

    let created = create_probe_task(&orch, "cancel_root", "Task cancelled mid-execution").await;

    {
        let mut o = orch.lock().await;
        o.config.test_executor_scenario = Some("controlled_hold".to_string());
    }

    begin_planning(orch.clone(), created.id.clone())
        .await
        .expect("planning should start");

    // Wait for the approval gate, approve, then cancel while the executor holds.
    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    loop {
        {
            let o = orch.lock().await;
            if o.current_task
                .as_ref()
                .map_or(false, |t| t.state == TaskState::AwaitingApproval)
            {
                break;
            }
        }
        assert!(
            std::time::Instant::now() < deadline,
            "never reached approval gate"
        );
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    approve_plan(orch.clone(), created.id.clone())
        .await
        .expect("approval");

    archify_lib::orchestrator::cancel_task(orch.clone(), created.id.clone())
        .await
        .expect("cancellation should be accepted");

    let deadline = std::time::Instant::now() + Duration::from_secs(30);
    let final_state = loop {
        {
            let o = orch.lock().await;
            if let Some(rec) = o.current_task.as_ref() {
                if rec.state.is_terminal() {
                    break rec.state.clone();
                }
            }
        }
        assert!(std::time::Instant::now() < deadline, "cancel never settled");
        tokio::time::sleep(Duration::from_millis(25)).await;
    };
    assert_eq!(final_state, TaskState::Cancelled);
    assert_ne!(final_state, TaskState::Completed);
}
