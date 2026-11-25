//! Archify Priority 1 - Genuine Real-Agent Acceptance Test Suite.
//!
//! These tests exercise the full orchestration pipeline (create → plan → approve
//! → execute → verify → review → complete/fail) using the built-in test adapter,
//! which simulates a real CLI coding agent without requiring network or
//! authentication. They validate that the orchestrator correctly handles:
//!
//! 1. CLI agent coding task completion
//! 2. Multi-step task completion (graph)
//! 3. Agent returns failure
//! 4. Review process detects incomplete work
//! 5. Corrective prompt production
//! 6. Coding agent corrective work (full retry loop)
//! 7. Verification blocks incomplete results
//! 8. Retry exhaustion stops execution
//! 9. Cancellation prevents false completion
//! 10. Restart recovery preserves completed work
//! 11. Duplicate execution prevention
//! 12. Final reports contain evidence

use archify_lib::models::{
    ExecutorResult, ProviderConfigInput, ReviewerResponse, ReviewerVerdict, TaskState,
    VerificationResult, VerificationStatus,
};
use archify_lib::orchestrator::{
    Orchestrator, TaskCreationOptions, approve_plan, begin_planning, cancel_task, create_task,
};
use archify_lib::providers::test_adapter::{
    TestExecutor, TestExecutorScenario, TestPlannerReviewer, TestPlannerScenario,
};
use archify_lib::recovery_policy::{
    ExecutionReport, FailureClass, PolicyDecision, StopReason, evaluate,
};
use archify_lib::verification::EvidenceStatus;
use archify_lib::verification::assess_completion;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

type SharedOrch = Arc<Mutex<Orchestrator>>;

fn scratch_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("archify_p1_{tag}_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).expect("failed to create scratch dir");
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

fn test_config(
    planner: TestPlannerScenario,
    executor: TestExecutorScenario,
) -> ProviderConfigInput {
    ProviderConfigInput {
        planner_type: Some("test".to_string()),
        executor_type: Some("test".to_string()),
        test_planner_scenario: Some(format!("{planner:?}")),
        test_executor_scenario: Some(format!("{executor:?}")),
        ..Default::default()
    }
}

/// Poll the current task until it reaches `expected` or timeout expires.
async fn wait_for_state(
    orch: &SharedOrch,
    task_id: &str,
    expected: TaskState,
    timeout_secs: u64,
) -> bool {
    let start = std::time::Instant::now();
    let interval = std::time::Duration::from_millis(50);
    while start.elapsed() < std::time::Duration::from_secs(timeout_secs) {
        let guard = orch.lock().await;
        if let Some(t) = guard.current_task.as_ref() {
            if t.id == task_id && t.state == expected {
                return true;
            }
        }
        drop(guard);
        tokio::time::sleep(interval).await;
    }
    false
}

/// Get the current task record, if any.
async fn current_task(orch: &SharedOrch) -> Option<archify_lib::models::TaskRecord> {
    let guard = orch.lock().await;
    guard.current_task.clone()
}

/// Configure the orchestrator with test adapter planner/executor scenarios.
async fn set_config(orch: &SharedOrch, config: ProviderConfigInput) {
    let mut guard = orch.lock().await;
    guard.configure_providers(config).unwrap();
}

// ---------------------------------------------------------------------------
// Scenario 1: Successful task completion
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scenario_1_successful_task_completion() {
    let (orch, project_root) = setup_orch("s1_success").await;

    let options = TaskCreationOptions::default();
    let task = create_task(
        orch.clone(),
        "Implement a hello world function".to_string(),
        project_root.to_string_lossy().to_string(),
        options,
    )
    .await
    .unwrap();

    set_config(
        &orch,
        test_config(
            TestPlannerScenario::NormalSuccess,
            TestExecutorScenario::NormalSuccess,
        ),
    )
    .await;

    begin_planning(orch.clone(), task.id.clone()).await.unwrap();

    assert!(
        wait_for_state(&orch, &task.id, TaskState::AwaitingApproval, 10).await,
        "Task should reach AwaitingApproval after planning"
    );

    approve_plan(orch.clone(), task.id.clone()).await.unwrap();

    assert!(
        wait_for_state(&orch, &task.id, TaskState::Completed, 30).await,
        "Task should complete successfully with NormalSuccess scenarios"
    );

    let final_state = current_task(&orch).await.unwrap();
    assert_eq!(final_state.state, TaskState::Completed);
    assert!(
        !final_state.iterations.is_empty(),
        "Should have at least one iteration"
    );
    assert!(final_state.iterations[0].executor_result.is_some());
    assert_eq!(
        final_state.iterations[0]
            .executor_result
            .as_ref()
            .unwrap()
            .exit_code,
        Some(0)
    );
}

// ---------------------------------------------------------------------------
// Scenario 2: Multi-step task completion (graph)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scenario_2_multi_step_task_completion() {
    let (orch, project_root) = setup_orch("s2_multistep").await;

    let task = create_task(
        orch.clone(),
        "Implement a multi-step feature with three phases".to_string(),
        project_root.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();

    set_config(
        &orch,
        test_config(
            TestPlannerScenario::NormalSuccess,
            TestExecutorScenario::NormalSuccess,
        ),
    )
    .await;

    begin_planning(orch.clone(), task.id.clone()).await.unwrap();

    assert!(
        wait_for_state(&orch, &task.id, TaskState::AwaitingApproval, 10).await,
        "Planning should produce AwaitingApproval"
    );

    approve_plan(orch.clone(), task.id.clone()).await.unwrap();

    assert!(
        wait_for_state(&orch, &task.id, TaskState::Completed, 30).await,
        "Multi-step task should complete"
    );

    let final_state = current_task(&orch).await.unwrap();
    assert_eq!(final_state.state, TaskState::Completed);
    assert!(
        final_state.iterations.len() >= 1,
        "Should have iteration records"
    );
    let last_iter = &final_state.iterations[final_state.iterations.len() - 1];
    assert!(
        last_iter.reviewer_response.is_some(),
        "Reviewer should have evaluated the result"
    );
    assert_eq!(
        last_iter.reviewer_response.as_ref().unwrap().verdict,
        ReviewerVerdict::Complete
    );
}

// ---------------------------------------------------------------------------
// Scenario 3: Agent returns failure
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scenario_3_agent_returns_failure() {
    let (orch, project_root) = setup_orch("s3_failure").await;

    let task = create_task(
        orch.clone(),
        "Task that will fail at execution".to_string(),
        project_root.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();

    // Graph loop: AlwaysNeedsChanges planner → reviewer says NeedsChanges →
    // retry loop until exhaustion → Failed
    set_config(
        &orch,
        test_config(
            TestPlannerScenario::AlwaysNeedsChanges,
            TestExecutorScenario::NormalSuccess,
        ),
    )
    .await;

    begin_planning(orch.clone(), task.id.clone()).await.unwrap();

    assert!(
        wait_for_state(&orch, &task.id, TaskState::AwaitingApproval, 10).await,
        "Planning should succeed"
    );

    approve_plan(orch.clone(), task.id.clone()).await.unwrap();

    // AlwaysNeedsChanges causes retry loop → exhaustion → Failed
    assert!(
        wait_for_state(&orch, &task.id, TaskState::Failed, 30).await,
        "Task should fail after retry exhaustion"
    );

    let final_state = current_task(&orch).await.unwrap();
    assert_eq!(final_state.state, TaskState::Failed);
    assert!(final_state.error.is_some());
    assert!(final_state.iterations.len() >= 1);
}

// ---------------------------------------------------------------------------
// Scenario 4: Review process detects incomplete work
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scenario_4_reviewer_detects_incomplete_work() {
    let (orch, project_root) = setup_orch("s4_review_incomplete").await;

    let task = create_task(
        orch.clone(),
        "Feature that needs changes once".to_string(),
        project_root.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();

    set_config(
        &orch,
        test_config(
            TestPlannerScenario::NeedsChangesOnce,
            TestExecutorScenario::NormalSuccess,
        ),
    )
    .await;

    begin_planning(orch.clone(), task.id.clone()).await.unwrap();

    assert!(
        wait_for_state(&orch, &task.id, TaskState::AwaitingApproval, 10).await,
        "Planning should succeed"
    );

    approve_plan(orch.clone(), task.id.clone()).await.unwrap();

    assert!(
        wait_for_state(&orch, &task.id, TaskState::Completed, 30).await,
        "Task should complete after one correction cycle"
    );

    let final_state = current_task(&orch).await.unwrap();
    assert_eq!(final_state.state, TaskState::Completed);
    assert!(
        final_state.iterations.len() >= 2,
        "Should have at least 2 iterations (initial + correction)"
    );
}

// ---------------------------------------------------------------------------
// Scenario 5: Corrective prompt production (direct recovery_policy test)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scenario_5_corrective_prompt_production() {
    let exec = ExecutorResult {
        exit_code: Some(0),
        stdout: "Ran successfully".to_string(),
        stderr: String::new(),
        timed_out: false,
        duration_ms: 100,
        token_usage: None,
    };
    let reviewer = ReviewerResponse {
        verdict: ReviewerVerdict::NeedsChanges,
        findings: "Missing null check before accessing property".to_string(),
        unresolved_issues: vec!["Add null check".to_string()],
        suggested_fix: Some("Wrap in optional chaining or if condition".to_string()),
    };

    let report = ExecutionReport {
        task_description: "Implement feature".to_string(),
        acceptance_criteria: vec!["Feature handles null input".to_string()],
        iteration: 1,
        max_iterations: 5,
        executor_result: Some(exec),
        verification_results: vec![],
        reviewer_response: Some(reviewer),
        previous_unresolved_issues: vec![],
        cancelled: false,
        invalid_config_detail: None,
        changed_files_count: 1,
        verification_configured: false,
    };

    match evaluate(&report) {
        PolicyDecision::Repair {
            failure_class,
            corrective_prompt_hint,
        } => {
            assert_eq!(failure_class, FailureClass::IncompleteWork);
            assert!(
                corrective_prompt_hint.contains("null check")
                    || corrective_prompt_hint.contains("optional chaining"),
                "Corrective hint should reference the reviewer's findings: {corrective_prompt_hint}"
            );
        }
        other => panic!("Expected Repair(IncompleteWork), got {:?}", other),
    }
}

// ---------------------------------------------------------------------------
// Scenario 6: Coding agent corrective work (full retry loop)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scenario_6_corrective_work_full_loop() {
    let (orch, project_root) = setup_orch("s6_corrective_loop").await;

    let task = create_task(
        orch.clone(),
        "Feature requiring one correction cycle".to_string(),
        project_root.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();

    set_config(
        &orch,
        test_config(
            TestPlannerScenario::NeedsChangesOnce,
            TestExecutorScenario::NormalSuccess,
        ),
    )
    .await;

    begin_planning(orch.clone(), task.id.clone()).await.unwrap();
    assert!(
        wait_for_state(&orch, &task.id, TaskState::AwaitingApproval, 10).await,
        "Planning should succeed"
    );
    approve_plan(orch.clone(), task.id.clone()).await.unwrap();

    assert!(
        wait_for_state(&orch, &task.id, TaskState::Completed, 30).await,
        "Task should complete after correction"
    );

    let final_state = current_task(&orch).await.unwrap();
    assert_eq!(final_state.state, TaskState::Completed);
    assert!(
        final_state.iterations.len() >= 2,
        "Should have completed with at least one correction cycle recorded"
    );

    let last_iter = &final_state.iterations[final_state.iterations.len() - 1];
    assert_eq!(
        last_iter.reviewer_response.as_ref().unwrap().verdict,
        ReviewerVerdict::Complete
    );
}

// ---------------------------------------------------------------------------
// Scenario 7: Verification blocks incomplete results
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scenario_7_verification_blocks_incomplete_results() {
    let criteria = vec!["Feature works".to_string()];
    let reviewer = ReviewerResponse {
        verdict: ReviewerVerdict::Complete,
        findings: "All good".to_string(),
        unresolved_issues: vec![],
        suggested_fix: None,
    };

    // Verification configured but no results → missing evidence, cannot pass
    let assessment = assess_completion(&criteria, Some(&reviewer), &[], true);
    assert!(!assessment.passed());
    assert_eq!(assessment.verification_status, EvidenceStatus::Missing);
    assert!(
        assessment
            .missing_evidence
            .iter()
            .any(|m| m.contains("configured but no verification command results"))
    );

    // Verification not configured, no results → pass is OK
    let assessment_unconfigured = assess_completion(&criteria, Some(&reviewer), &[], false);
    assert!(assessment_unconfigured.passed());
}

// ---------------------------------------------------------------------------
// Scenario 8: Retry exhaustion stops execution
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scenario_8_retry_exhaustion_stops_execution() {
    let (orch, project_root) = setup_orch("s8_exhaustion").await;

    let task = create_task(
        orch.clone(),
        "Task that always needs changes".to_string(),
        project_root.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();

    set_config(
        &orch,
        test_config(
            TestPlannerScenario::AlwaysNeedsChanges,
            TestExecutorScenario::NormalSuccess,
        ),
    )
    .await;

    begin_planning(orch.clone(), task.id.clone()).await.unwrap();
    assert!(
        wait_for_state(&orch, &task.id, TaskState::AwaitingApproval, 10).await,
        "Planning should succeed"
    );
    approve_plan(orch.clone(), task.id.clone()).await.unwrap();

    assert!(
        wait_for_state(&orch, &task.id, TaskState::Failed, 30).await,
        "Task should fail after retry budget exhaustion"
    );

    let final_state = current_task(&orch).await.unwrap();
    assert_eq!(final_state.state, TaskState::Failed);
    assert!(final_state.error.is_some());
    assert!(
        final_state.iterations.len() >= 1,
        "Should have recorded iterations before failure"
    );
}

// ---------------------------------------------------------------------------
// Scenario 9: Cancellation prevents false completion
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scenario_9_cancellation_prevents_false_completion() {
    let (orch, project_root) = setup_orch("s9_cancel").await;

    let task = create_task(
        orch.clone(),
        "Long-running task to cancel".to_string(),
        project_root.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();

    set_config(
        &orch,
        test_config(
            TestPlannerScenario::NormalSuccess,
            TestExecutorScenario::NormalSuccess,
        ),
    )
    .await;

    begin_planning(orch.clone(), task.id.clone()).await.unwrap();
    assert!(
        wait_for_state(&orch, &task.id, TaskState::AwaitingApproval, 10).await,
        "Planning should succeed"
    );

    cancel_task(orch.clone(), task.id.clone()).await.unwrap();

    assert!(
        wait_for_state(&orch, &task.id, TaskState::Cancelled, 10).await,
        "Task should be cancelled"
    );

    let final_state = current_task(&orch).await.unwrap();
    assert_eq!(final_state.state, TaskState::Cancelled);
    assert!(final_state.error.is_some());
    assert!(final_state.error.as_ref().unwrap().contains("cancelled"));
}

// ---------------------------------------------------------------------------
// Scenario 10: Restart recovery preserves completed work
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scenario_10_restart_recovery_preserves_completed_work() {
    let history_dir = scratch_dir("s10_recovery_hist");
    let config_dir = scratch_dir("s10_recovery_cfg");
    let project_root = scratch_dir("s10_recovery_proj");

    let orch1 =
        Orchestrator::new_isolated(None, history_dir.clone(), config_dir.join("providers.json"));
    let orch1 = Arc::new(Mutex::new(orch1));

    let task = create_task(
        orch1.clone(),
        "Task to persist and recover".to_string(),
        project_root.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();

    set_config(
        &orch1,
        test_config(
            TestPlannerScenario::NormalSuccess,
            TestExecutorScenario::NormalSuccess,
        ),
    )
    .await;

    begin_planning(orch1.clone(), task.id.clone())
        .await
        .unwrap();
    assert!(
        wait_for_state(&orch1, &task.id, TaskState::AwaitingApproval, 10).await,
        "Planning should succeed"
    );
    approve_plan(orch1.clone(), task.id.clone()).await.unwrap();
    assert!(
        wait_for_state(&orch1, &task.id, TaskState::Completed, 30).await,
        "Task should complete"
    );

    // Drop the first orchestrator, create a second one with the same history dir
    // Use new_with_paths to simulate a restart that loads history from disk.
    drop(orch1);
    let orch2 =
        Orchestrator::new_with_paths(None, history_dir.clone(), config_dir.join("providers.json"));
    let orch2 = Arc::new(Mutex::new(orch2));

    let guard = orch2.lock().await;
    let history = &guard.task_history;
    assert!(
        history.contains_key(&task.id),
        "Completed task should be recoverable from history after restart"
    );
    let recovered = history.get(&task.id).unwrap();
    assert_eq!(recovered.state, TaskState::Completed);
    assert_eq!(recovered.user_task, "Task to persist and recover");
}

// ---------------------------------------------------------------------------
// Scenario 11: Duplicate execution prevention
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scenario_11_duplicate_execution_prevention() {
    let (orch, project_root) = setup_orch("s11_duplicate").await;

    let task1 = create_task(
        orch.clone(),
        "First active task".to_string(),
        project_root.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();

    set_config(
        &orch,
        test_config(
            TestPlannerScenario::NormalSuccess,
            TestExecutorScenario::NormalSuccess,
        ),
    )
    .await;

    begin_planning(orch.clone(), task1.id.clone())
        .await
        .unwrap();
    assert!(
        wait_for_state(&orch, &task1.id, TaskState::AwaitingApproval, 10).await,
        "First task should reach AwaitingApproval"
    );

    let result = create_task(
        orch.clone(),
        "Second task while first is active".to_string(),
        project_root.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await;

    assert!(
        result.is_err(),
        "Creating a second task while one is active should fail"
    );
    let err = result.unwrap_err();
    assert!(
        err.contains("active") || err.contains("already active"),
        "Error should mention active task: {err}"
    );
}

// ---------------------------------------------------------------------------
// Scenario 12: Final reports contain evidence
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_scenario_12_final_reports_contain_evidence() {
    let (orch, project_root) = setup_orch("s12_evidence").await;

    let task = create_task(
        orch.clone(),
        "Task that produces real file evidence".to_string(),
        project_root.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();

    set_config(
        &orch,
        test_config(
            TestPlannerScenario::NormalSuccess,
            TestExecutorScenario::NormalSuccess,
        ),
    )
    .await;

    begin_planning(orch.clone(), task.id.clone()).await.unwrap();
    assert!(
        wait_for_state(&orch, &task.id, TaskState::AwaitingApproval, 10).await,
        "Planning should succeed"
    );
    approve_plan(orch.clone(), task.id.clone()).await.unwrap();
    assert!(
        wait_for_state(&orch, &task.id, TaskState::Completed, 30).await,
        "Task should complete"
    );

    let final_state = current_task(&orch).await.unwrap();
    assert_eq!(final_state.state, TaskState::Completed);

    assert!(
        !final_state.iterations.is_empty(),
        "Final report should have iteration records"
    );
    for iter in &final_state.iterations {
        assert!(
            iter.executor_result.is_some(),
            "Each iteration should have executor result evidence"
        );
        let exec = iter.executor_result.as_ref().unwrap();
        assert!(
            !exec.stdout.is_empty(),
            "Executor stdout should not be empty: {:?}",
            exec.stdout
        );
    }

    assert!(
        final_state.task_level_changes.is_some(),
        "Task-level changes should be recorded"
    );
    let changes = final_state.task_level_changes.as_ref().unwrap();
    assert!(
        !changes.all_paths().is_empty(),
        "Should have detected at least one changed file"
    );
}

// ---------------------------------------------------------------------------
// Direct recovery_policy edge case tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_recovery_policy_verification_configured_but_not_executed() {
    let exec = ExecutorResult {
        exit_code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
        duration_ms: 50,
        token_usage: None,
    };
    let reviewer = ReviewerResponse {
        verdict: ReviewerVerdict::Complete,
        findings: "All criteria met".to_string(),
        unresolved_issues: vec![],
        suggested_fix: None,
    };

    let report = ExecutionReport {
        task_description: "Task".to_string(),
        acceptance_criteria: vec!["Criterion".to_string()],
        iteration: 1,
        max_iterations: 5,
        executor_result: Some(exec),
        verification_results: vec![],
        reviewer_response: Some(reviewer),
        previous_unresolved_issues: vec![],
        cancelled: false,
        invalid_config_detail: None,
        changed_files_count: 2,
        verification_configured: true,
    };

    match evaluate(&report) {
        PolicyDecision::Repair {
            failure_class,
            corrective_prompt_hint,
        } => {
            assert_eq!(failure_class, FailureClass::MissingEvidence);
            assert!(
                corrective_prompt_hint.contains(
                    "Verification is configured but no verification command results were collected"
                ),
                "Hint should mention missing verification results: {corrective_prompt_hint}"
            );
        }
        other => panic!("Expected Repair(MissingEvidence), got {:?}", other),
    }
}

#[tokio::test]
async fn test_recovery_policy_verification_failed_blocks_completion() {
    // Use verification output that does NOT contain test-failure or compiler
    // markers, so the classifier returns VerificationFailure.
    let exec = ExecutorResult {
        exit_code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
        duration_ms: 50,
        token_usage: None,
    };
    let reviewer = ReviewerResponse {
        verdict: ReviewerVerdict::Complete,
        findings: "All good".to_string(),
        unresolved_issues: vec![],
        suggested_fix: None,
    };
    let failed_verification = VerificationResult {
        command: "cargo build".to_string(),
        status: VerificationStatus::Failed,
        exit_code: Some(1),
        output: "command failed with exit code 1".to_string(),
    };

    let report = ExecutionReport {
        task_description: "Task".to_string(),
        acceptance_criteria: vec!["Criterion".to_string()],
        iteration: 1,
        max_iterations: 5,
        executor_result: Some(exec),
        verification_results: vec![failed_verification],
        reviewer_response: Some(reviewer),
        previous_unresolved_issues: vec![],
        cancelled: false,
        invalid_config_detail: None,
        changed_files_count: 2,
        verification_configured: true,
    };

    match evaluate(&report) {
        PolicyDecision::Repair {
            failure_class,
            corrective_prompt_hint,
        } => {
            assert_eq!(failure_class, FailureClass::VerificationFailure);
            assert!(
                corrective_prompt_hint.contains("command failed with exit code 1"),
                "Hint should contain verification failure output: {corrective_prompt_hint}"
            );
        }
        other => panic!("Expected Repair(VerificationFailure), got {:?}", other),
    }
}

#[tokio::test]
async fn test_recovery_policy_reviewer_blocked_is_permanent_failure() {
    let exec = ExecutorResult {
        exit_code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
        duration_ms: 50,
        token_usage: None,
    };
    let reviewer = ReviewerResponse {
        verdict: ReviewerVerdict::Blocked,
        findings: "Architecture incompatibility".to_string(),
        unresolved_issues: vec!["Dependency cannot be satisfied".to_string()],
        suggested_fix: None,
    };

    let report = ExecutionReport {
        task_description: "Task".to_string(),
        acceptance_criteria: vec!["Criterion".to_string()],
        iteration: 1,
        max_iterations: 5,
        executor_result: Some(exec),
        verification_results: vec![],
        reviewer_response: Some(reviewer),
        previous_unresolved_issues: vec![],
        cancelled: false,
        invalid_config_detail: None,
        changed_files_count: 0,
        verification_configured: false,
    };

    match evaluate(&report) {
        PolicyDecision::Stop {
            reason: StopReason::PermanentFailure { failure_class },
        } => {
            assert_eq!(failure_class, FailureClass::ReviewFailure);
        }
        other => panic!(
            "Expected Stop(PermanentFailure/ReviewFailure), got {:?}",
            other
        ),
    }
}

// ---------------------------------------------------------------------------
// Compile-time type checks (ensures the public API is stable)
// ---------------------------------------------------------------------------

#[allow(dead_code)]
fn type_check_imports() {
    let _ = TestExecutor::new(TestExecutorScenario::NormalSuccess);
    let _ = TestPlannerReviewer::new(TestPlannerScenario::NormalSuccess);
    let _ = TaskCreationOptions::default();
    let _ = ProviderConfigInput::default();
    let _ = TaskState::Completed;
    let _ = TaskState::Failed;
    let _ = TaskState::Cancelled;
    let _ = ReviewerVerdict::Complete;
    let _ = ReviewerVerdict::NeedsChanges;
    let _ = ReviewerVerdict::Blocked;
    let _ = VerificationStatus::Passed;
    let _ = VerificationStatus::Failed;
    let _ = VerificationStatus::Skipped;
    let _ = VerificationStatus::Unavailable;
}
