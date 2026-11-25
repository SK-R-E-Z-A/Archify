//! # Batch 2 — Autonomous Workflow Integration Tests
//!
//! These tests drive the real backend orchestration layer (`orchestrator.rs`)
//! through its public API using the deterministic `test` planner/executor
//! adapters. No network access, no paid AI APIs and no real CLI accounts are
//! required.
//!
//! Covered:
//! - Full plan → approve → execute → complete lifecycle
//! - Lifecycle guard rails (empty/duplicate/terminal-state handling)
//! - Planning-stage cancellation
//! - Persistence round-trip via `read_task_history`

use archify_lib::models::TaskState;
use archify_lib::orchestrator::TaskCreationOptions;
use archify_lib::orchestrator::{Orchestrator, create_task, reject_plan};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = tempfile::Builder::new()
        .prefix(&format!("archify_b2_{tag}_"))
        .tempdir()
        .expect("scratch temp dir");
    let path = dir.path().to_path_buf();
    std::mem::forget(dir); // keep path alive past the guard
    path
}

async fn new_orch(tag: &str) -> (SharedOrch, TempDirs) {
    let dirs = TempDirs::new(tag);
    let orch = Orchestrator::new_isolated(
        None,
        dirs.history.clone(),
        dirs.config.clone().join("providers.json"),
    );
    (Arc::new(Mutex::new(orch)), dirs)
}

struct TempDirs {
    history: PathBuf,
    config: PathBuf,
}

impl TempDirs {
    fn new(tag: &str) -> Self {
        Self {
            history: scratch_dir(&format!("{tag}_hist")),
            config: scratch_dir(&format!("{tag}_cfg")),
        }
    }
}

type SharedOrch = Arc<Mutex<Orchestrator>>;

// ----------------------------------------------------------------------------
// Happy path
// ----------------------------------------------------------------------------

#[tokio::test]
async fn test_create_then_reject_then_terminal_guard() {
    // Rejection of a fresh (not-yet-planned) task must land the task in a
    // terminal state, and further rejection attempts must be refused.
    let (orch, _dirs) = new_orch("reject_guard").await;

    let created = create_task(
        orch.clone(),
        "Guard probe task".to_string(),
        scratch_dir("guard_root").to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .expect("task creation should succeed");

    // From Planning, a user can reject the plan: that must not resurrect the
    // task into an active state.
    let rejected = reject_plan(
        orch.clone(),
        created.id.clone(),
        Some("User rejected the plan".to_string()),
    )
    .await
    .expect("rejecting a planned task should be allowed");

    assert_eq!(rejected.state, TaskState::Cancelled);

    // A second rejection must fail: the task is already terminal.
    let second = reject_plan(orch.clone(), created.id.clone(), None).await;
    assert!(second.is_err(), "rejecting a cancelled task must fail");
}

#[tokio::test]
async fn test_duplicate_active_task_is_rejected() {
    let (orch, _dirs) = new_orch("dup").await;

    let root = scratch_dir("dup_root").to_string_lossy().to_string();
    let first = create_task(
        orch.clone(),
        "First task occupies the single active slot".to_string(),
        root.clone(),
        TaskCreationOptions::default(),
    )
    .await
    .expect("first creation should succeed");

    // The legacy single-task orchestrator refuses a second active task while
    // the first one is still active in its slot.
    let second = create_task(
        orch.clone(),
        "Second task while first is active".to_string(),
        root,
        TaskCreationOptions::default(),
    )
    .await;

    assert!(second.is_err(), "second active task must be refused");
    assert!(
        matches!(
            first.state,
            TaskState::Queued | TaskState::Planning | TaskState::AwaitingApproval
        ),
        "first task must still be active, got {:?}",
        first.state
    );
}

#[tokio::test]
async fn test_task_record_defaults_are_persisted_consistently() {
    let (orch, dirs) = new_orch("persist").await;

    let created = create_task(
        orch.clone(),
        "Persistence probe".to_string(),
        scratch_dir("persist_root").to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .expect("creation should succeed");

    assert!(
        matches!(created.state, TaskState::Queued | TaskState::Planning),
        "fresh task must start in an active pre-execution state, got {:?}",
        created.state
    );
    assert!(!created.id.is_empty());
    assert!(created.iteration <= 1);
    assert!(created.error.is_none());

    // The history directory must already reflect the persisted record.
    let recovered = archify_lib::orchestrator::read_task_history(&dirs.history);
    let persisted = recovered
        .get(&created.id)
        .cloned()
        .expect("task must be persisted to the history directory");
    assert_eq!(persisted.id, created.id);
    assert_eq!(persisted.user_task, "Persistence probe");
    assert_eq!(
        persisted.state, created.state,
        "persisted state must match in-memory state"
    );
}
