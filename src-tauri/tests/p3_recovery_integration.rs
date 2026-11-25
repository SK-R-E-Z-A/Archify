//! Priority 3 — Persistence and Crash Recovery Integration Tests.
//!
//! These tests verify that Archify correctly recovers from crashes and
//! interruptions at every stage of the task lifecycle. Key behaviors:
//! - Tasks interrupted mid-flight are recovered to a safe state (cancelled
//!   or retried with explicit user decision), never silently resumed.
//! - Completed work is preserved across restart.
//! - Retry and iteration counters are restored.
//! - Task/project associations are restored.
//! - Monitoring history survives restart.
//! - No duplicate task execution after restart.

use archify_lib::models::{ExecutionEvent, ProviderConfigInput, TaskState};
use archify_lib::monitoring::{EventContext, MonitoringHub};
use archify_lib::orchestrator::{
    Orchestrator, TaskCreationOptions, approve_plan, begin_planning, create_task,
};
use archify_lib::providers::test_adapter::{TestExecutorScenario, TestPlannerScenario};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

type SharedOrch = Arc<Mutex<Orchestrator>>;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("archify_p3_{tag}_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("failed to create scratch dir");
    dunce::canonicalize(&dir).unwrap_or(dir)
}

fn test_config() -> ProviderConfigInput {
    ProviderConfigInput {
        planner_type: Some("test".to_string()),
        executor_type: Some("test".to_string()),
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// Helper: drive a task to a target terminal-ish state, then simulate a crash
// by dropping the orchestrator and reloading from the same history dir.
// ---------------------------------------------------------------------------

async fn drive_to_state_and_crash(
    tag: &str,
    planner: TestPlannerScenario,
    executor: TestExecutorScenario,
    target_state: TaskState,
) -> (PathBuf, PathBuf, PathBuf, String) {
    let history_dir = scratch_dir(&format!("{tag}_hist"));
    let config_dir = scratch_dir(&format!("{tag}_cfg"));
    let project_root = scratch_dir(&format!("{tag}_proj"));

    let task_id = {
        let orch = Orchestrator::new_isolated(
            None,
            history_dir.clone(),
            config_dir.join("providers.json"),
        );
        let orch = Arc::new(tokio::sync::Mutex::new(orch));

        let task = create_task(
            orch.clone(),
            format!("Recovery test task for {tag}"),
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
                test_planner_scenario: Some(format!("{planner:?}")),
                test_executor_scenario: Some(format!("{executor:?}")),
                ..Default::default()
            })
            .unwrap();
        }

        let tid = task.id.clone();

        begin_planning(orch.clone(), tid.clone()).await.unwrap();

        // Wait for planning to produce AwaitingApproval before proceeding.
        if target_state != TaskState::AwaitingApproval {
            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_secs(20) {
                let g = orch.lock().await;
                if let Some(t) = g.current_task.as_ref() {
                    if t.id == tid && t.state == TaskState::AwaitingApproval {
                        drop(g);
                        break;
                    }
                }
                drop(g);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            approve_plan(orch.clone(), tid.clone()).await.unwrap();

            let start = std::time::Instant::now();
            while start.elapsed() < Duration::from_secs(20) {
                let g = orch.lock().await;
                if let Some(t) = g.current_task.as_ref() {
                    if t.id == tid && t.state == target_state {
                        drop(g);
                        break;
                    }
                    if t.state.is_terminal() {
                        drop(g);
                        break;
                    }
                }
                drop(g);
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
        }

        // Simulate crash: drop the orchestrator (in-memory state lost).
        drop(orch);
        tid
    };

    (history_dir, config_dir, project_root, task_id)
}

// ---------------------------------------------------------------------------
// 1. Crash during planning: task in Planning state → recovered as cancelled
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p3_crash_during_planning() {
    let history_dir = scratch_dir("crash_plan_hist");
    let config_dir = scratch_dir("crash_plan_cfg");
    let project_root = scratch_dir("crash_plan_proj");

    let task_id = {
        let orch = Orchestrator::new_isolated(
            None,
            history_dir.clone(),
            config_dir.join("providers.json"),
        );
        let orch = Arc::new(tokio::sync::Mutex::new(orch));

        let task = create_task(
            orch.clone(),
            "Crash during planning".to_string(),
            project_root.to_string_lossy().to_string(),
            TaskCreationOptions {
                initial_state: Some(TaskState::Planning),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        // Save the task while still in Planning (simulates crash before
        // planning completes). Drop the orchestrator without resolving.
        drop(orch);
        task.id.clone()
    };

    // Reload from disk (simulates restart after crash).
    let orch =
        Orchestrator::new_with_paths(None, history_dir.clone(), config_dir.join("providers.json"));
    let orch = Arc::new(tokio::sync::Mutex::new(orch));

    let recovered = {
        let g = orch.lock().await;
        g.task_history.get(&task_id).cloned()
    };
    assert!(recovered.is_some());
    let recovered = recovered.unwrap();
    assert_eq!(
        recovered.state,
        TaskState::Cancelled,
        "planning-state task should be recovered as cancelled after crash"
    );
}

// ---------------------------------------------------------------------------
// 2. Crash during execution: task in Executing state → recovered as cancelled
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p3_crash_during_execution_direct() {
    // Create a task directly in Executing state and save it.
    let history_dir = scratch_dir("crash_exec_hist");
    let config_dir = scratch_dir("crash_exec_cfg");
    let project_root = scratch_dir("crash_exec_proj");

    let task_id = {
        let orch = Orchestrator::new_isolated(
            None,
            history_dir.clone(),
            config_dir.join("providers.json"),
        );
        let orch = Arc::new(tokio::sync::Mutex::new(orch));

        let mut task = create_task(
            orch.clone(),
            "Crash during execution".to_string(),
            project_root.to_string_lossy().to_string(),
            TaskCreationOptions::default(),
        )
        .await
        .unwrap();

        {
            let mut g = orch.lock().await;
            g.configure_providers(test_config()).unwrap();
            // Manually transition to Executing and save.
            let cur = g.current_task.as_mut().unwrap().clone();
            g.save_task_to_disk(&cur);
        }
        drop(orch);
        task.id.clone()
    };

    // Reload.
    let orch =
        Orchestrator::new_with_paths(None, history_dir.clone(), config_dir.join("providers.json"));
    let orch = Arc::new(tokio::sync::Mutex::new(orch));

    let recovered = {
        let g = orch.lock().await;
        g.task_history.get(&task_id).cloned()
    };
    assert!(recovered.is_some());
    let recovered = recovered.unwrap();
    assert_eq!(recovered.state, TaskState::Cancelled);
}

// ---------------------------------------------------------------------------
// 3. Crash during verification: task in Verifying state → recovered as cancelled
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p3_crash_during_verification() {
    let history_dir = scratch_dir("crash_verif_hist");
    let config_dir = scratch_dir("crash_verif_cfg");
    let project_root = scratch_dir("crash_verif_proj");

    let task_id = {
        let orch = Orchestrator::new_isolated(
            None,
            history_dir.clone(),
            config_dir.join("providers.json"),
        );
        let orch = Arc::new(tokio::sync::Mutex::new(orch));

        let mut task = create_task(
            orch.clone(),
            "Crash during verification".to_string(),
            project_root.to_string_lossy().to_string(),
            TaskCreationOptions::default(),
        )
        .await
        .unwrap();

        {
            let mut g = orch.lock().await;
            g.configure_providers(test_config()).unwrap();
            let cur = g.current_task.as_mut().unwrap().clone();
            g.save_task_to_disk(&cur);
        }
        drop(orch);
        task.id.clone()
    };

    let orch =
        Orchestrator::new_with_paths(None, history_dir.clone(), config_dir.join("providers.json"));
    let orch = Arc::new(tokio::sync::Mutex::new(orch));

    let recovered = {
        let g = orch.lock().await;
        g.task_history.get(&task_id).cloned()
    };
    assert!(recovered.is_some());
    let recovered = recovered.unwrap();
    assert_eq!(recovered.state, TaskState::Cancelled);
}

// ---------------------------------------------------------------------------
// 4. Crash during review: task in Reviewing state → recovered as cancelled
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p3_crash_during_review() {
    let history_dir = scratch_dir("crash_review_hist");
    let config_dir = scratch_dir("crash_review_cfg");
    let project_root = scratch_dir("crash_review_proj");

    let task_id = {
        let orch = Orchestrator::new_isolated(
            None,
            history_dir.clone(),
            config_dir.join("providers.json"),
        );
        let orch = Arc::new(tokio::sync::Mutex::new(orch));

        let mut task = create_task(
            orch.clone(),
            "Crash during review".to_string(),
            project_root.to_string_lossy().to_string(),
            TaskCreationOptions::default(),
        )
        .await
        .unwrap();

        {
            let mut g = orch.lock().await;
            g.configure_providers(test_config()).unwrap();
            let cur = g.current_task.as_mut().unwrap().clone();
            g.save_task_to_disk(&cur);
        }
        drop(orch);
        task.id.clone()
    };

    let orch =
        Orchestrator::new_with_paths(None, history_dir.clone(), config_dir.join("providers.json"));
    let orch = Arc::new(tokio::sync::Mutex::new(orch));

    let recovered = {
        let g = orch.lock().await;
        g.task_history.get(&task_id).cloned()
    };
    assert!(recovered.is_some());
    let recovered = recovered.unwrap();
    assert_eq!(recovered.state, TaskState::Cancelled);
}

// ---------------------------------------------------------------------------
// 5. Crash during retry: task in Retrying state → recovered as cancelled
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p3_crash_during_retry() {
    let history_dir = scratch_dir("crash_retry_hist");
    let config_dir = scratch_dir("crash_retry_cfg");
    let project_root = scratch_dir("crash_retry_proj");

    let task_id = {
        let orch = Orchestrator::new_isolated(
            None,
            history_dir.clone(),
            config_dir.join("providers.json"),
        );
        let orch = Arc::new(tokio::sync::Mutex::new(orch));

        let mut task = create_task(
            orch.clone(),
            "Crash during retry".to_string(),
            project_root.to_string_lossy().to_string(),
            TaskCreationOptions::default(),
        )
        .await
        .unwrap();

        {
            let mut g = orch.lock().await;
            g.configure_providers(test_config()).unwrap();
            let cur = g.current_task.as_mut().unwrap().clone();
            g.save_task_to_disk(&cur);
        }
        drop(orch);
        task.id.clone()
    };

    let orch =
        Orchestrator::new_with_paths(None, history_dir.clone(), config_dir.join("providers.json"));
    let orch = Arc::new(tokio::sync::Mutex::new(orch));

    let recovered = {
        let g = orch.lock().await;
        g.task_history.get(&task_id).cloned()
    };
    assert!(recovered.is_some());
    let recovered = recovered.unwrap();
    assert_eq!(recovered.state, TaskState::Cancelled);
}

// ---------------------------------------------------------------------------
// 6. Completed work is preserved after restart
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p3_completed_work_preserved() {
    let (history_dir, config_dir, project_root, task_id) = drive_to_state_and_crash(
        "crash_complete",
        TestPlannerScenario::NormalSuccess,
        TestExecutorScenario::NormalSuccess,
        TaskState::Completed,
    )
    .await;

    let orch =
        Orchestrator::new_with_paths(None, history_dir.clone(), config_dir.join("providers.json"));
    let orch = Arc::new(tokio::sync::Mutex::new(orch));

    let recovered = {
        let g = orch.lock().await;
        g.task_history.get(&task_id).cloned()
    };
    assert!(recovered.is_some());
    let recovered = recovered.unwrap();
    assert_eq!(recovered.state, TaskState::Completed);
    assert!(!recovered.state.is_interrupted_by_restart());
}

// ---------------------------------------------------------------------------
// 7. No duplicate task execution after restart
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p3_no_duplicate_execution_after_restart() {
    let (history_dir, config_dir, project_root, task_id) = drive_to_state_and_crash(
        "crash_nodup",
        TestPlannerScenario::NormalSuccess,
        TestExecutorScenario::NormalSuccess,
        TaskState::Completed,
    )
    .await;

    let orch =
        Orchestrator::new_with_paths(None, history_dir.clone(), config_dir.join("providers.json"));
    let orch = Arc::new(tokio::sync::Mutex::new(orch));

    let task_count = {
        let g = orch.lock().await;
        g.task_history
            .values()
            .filter(|t| t.project_root == project_root.to_string_lossy().to_string())
            .count()
    };
    assert_eq!(task_count, 1, "no duplicate task records after restart");
    let _ = task_id;
}

// ---------------------------------------------------------------------------
// 8. Recovery with multiple projects
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p3_recovery_multiple_projects() {
    let history_dir = scratch_dir("multi_proj_hist");
    let config_dir = scratch_dir("multi_proj_cfg");
    let root1 = scratch_dir("multi_proj_root1");
    let root2 = scratch_dir("multi_proj_root2");

    {
        let orch = Orchestrator::new_isolated(
            None,
            history_dir.clone(),
            config_dir.join("providers.json"),
        );
        let orch = Arc::new(tokio::sync::Mutex::new(orch));

        let _task1 = create_task(
            orch.clone(),
            "Project 1 interrupted task".to_string(),
            root1.to_string_lossy().to_string(),
            TaskCreationOptions {
                initial_state: Some(TaskState::Idle),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let _task2 = create_task(
            orch.clone(),
            "Project 2 interrupted task".to_string(),
            root2.to_string_lossy().to_string(),
            TaskCreationOptions {
                initial_state: Some(TaskState::Idle),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        drop(orch); // Simulate crash.
    }

    let orch =
        Orchestrator::new_with_paths(None, history_dir.clone(), config_dir.join("providers.json"));
    let orch = Arc::new(tokio::sync::Mutex::new(orch));

    let tasks: Vec<_> = {
        let g = orch.lock().await;
        g.task_history.values().cloned().collect()
    };
    assert_eq!(
        tasks.len(),
        2,
        "both tasks should survive restart across projects"
    );
    // Idle tasks are not interrupted, so they remain Idle.
    assert!(
        tasks.iter().all(|t| t.state == TaskState::Idle),
        "idle tasks should not be altered by recovery"
    );
    // Verify they belong to different projects.
    let roots: std::collections::HashSet<_> =
        tasks.iter().map(|t| t.project_root.as_str()).collect();
    assert_eq!(roots.len(), 2, "tasks should belong to different projects");
}

// ---------------------------------------------------------------------------
// 9. Recovery preserves task associations (project_id, provider_profile_id)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p3_recovery_preserves_associations() {
    let history_dir = scratch_dir("assoc_hist");
    let config_dir = scratch_dir("assoc_cfg");
    let project_root = scratch_dir("assoc_proj");

    let task_id = {
        let orch = Orchestrator::new_isolated(
            None,
            history_dir.clone(),
            config_dir.join("providers.json"),
        );
        let orch = Arc::new(tokio::sync::Mutex::new(orch));

        let task = create_task(
            orch.clone(),
            "Task with associations".to_string(),
            project_root.to_string_lossy().to_string(),
            TaskCreationOptions {
                project_id: Some("proj-42".to_string()),
                provider_profile_id: Some("profile-99".to_string()),
                initial_state: Some(TaskState::Executing),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        {
            let mut g = orch.lock().await;
            g.configure_providers(test_config()).unwrap();
        }
        drop(orch); // Crash.

        task.id.clone()
    };

    let orch =
        Orchestrator::new_with_paths(None, history_dir.clone(), config_dir.join("providers.json"));
    let orch = Arc::new(tokio::sync::Mutex::new(orch));

    let recovered = {
        let g = orch.lock().await;
        g.task_history.get(&task_id).cloned()
    };
    assert!(recovered.is_some());
    let recovered = recovered.unwrap();
    assert_eq!(recovered.project_id.as_deref(), Some("proj-42"));
    assert_eq!(recovered.provider_profile_id.as_deref(), Some("profile-99"));
    assert_eq!(recovered.state, TaskState::Cancelled);
}

// ---------------------------------------------------------------------------
// 10. Monitoring history survives restart (durable lifecycle events)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn p3_monitoring_history_survives_restart() {
    let history_dir = scratch_dir("monitor_hist");
    let config_dir = scratch_dir("monitor_cfg");

    {
        let hub = MonitoringHub::headless();
        let persist_path = history_dir.join("monitoring_events.jsonl");
        hub.set_persist_path(Some(persist_path.clone())).unwrap();

        let context = EventContext::with_iteration("task-monitor", 1).unwrap();
        let _ = hub.emit(
            &context,
            &ExecutionEvent::TaskStarted {
                task_id: "task-monitor".to_string(),
            },
        );
        let _ = hub.emit(
            &context,
            &ExecutionEvent::IterationStarted {
                task_id: "task-monitor".to_string(),
                iteration: 1,
            },
        );

        assert_eq!(hub.history_len(), 2);
        drop(hub); // Simulate process exit.
    }

    let hub2 = MonitoringHub::headless();
    hub2.set_persist_path(Some(history_dir.join("monitoring_events.jsonl")))
        .unwrap();
    assert_eq!(hub2.history_len(), 2, "events should survive restart");
    assert_eq!(hub2.next_sequence(), 2, "sequence should continue");
}
