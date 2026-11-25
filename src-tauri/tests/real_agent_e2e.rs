//! Genuine real-agent acceptance test using OpenCode + Ollama through Archify's
//! normal orchestration path (create_task -> begin_planning -> approve_plan ->
//! execute -> verify -> review -> complete).

use archify_lib::models::PromptDelivery;
use archify_lib::models::ProviderConfigInput;
use archify_lib::orchestrator::{
    Orchestrator, TaskCreationOptions, approve_plan, begin_planning, create_task,
};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

type SharedOrch = Arc<Mutex<Orchestrator>>;

async fn setup_orch(tag: &str) -> SharedOrch {
    let history_dir =
        std::env::temp_dir().join(format!("archify_real_{tag}_hist_{}", uuid::Uuid::new_v4()));
    let config_dir =
        std::env::temp_dir().join(format!("archify_real_{tag}_cfg_{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&history_dir).unwrap();
    std::fs::create_dir_all(&config_dir).unwrap();
    let orch = Orchestrator::new_isolated(None, history_dir, config_dir.join("providers.json"));
    Arc::new(Mutex::new(orch))
}

async fn wait_for_state(
    orch: &SharedOrch,
    task_id: &str,
    expected: archify_lib::models::TaskState,
    timeout_secs: u64,
) -> bool {
    let start = std::time::Instant::now();
    let interval = std::time::Duration::from_millis(200);
    while start.elapsed() < std::time::Duration::from_secs(timeout_secs) {
        let guard = orch.lock().await;
        if let Some(t) = guard.current_task.as_ref() {
            if t.id == task_id && t.state == expected {
                return true;
            }
            if t.id == task_id
                && t.state == archify_lib::models::TaskState::Failed
                && expected != archify_lib::models::TaskState::Failed
            {
                eprintln!(
                    "[real-e2e] Task unexpectedly failed with error: {:?}",
                    t.error
                );
                return false;
            }
            // Print progress
            eprintln!(
                "[real-e2e] state={:?} error={:?} (waiting for {:?})",
                t.state, t.error, expected
            );
        }
        drop(guard);
        tokio::time::sleep(interval).await;
    }
    false
}

#[tokio::test]
#[ignore = "requires ollama+opencode and ~10 minutes; run explicitly with --ignored"]
async fn real_agent_opencode_ollama_success() {
    let project_root = PathBuf::from(r"C:\Claude projects\ArchifyTestProject");
    assert!(project_root.exists(), "test project must exist");

    let orch = setup_orch("success").await;

    let task = create_task(
        orch.clone(),
        "Add a subtract function with tests to src/main.rs. Keep the existing add function and its test. The subtract function should return a - b. Run cargo test.".to_string(),
        project_root.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();

    let config = ProviderConfigInput {
        planner_type: Some("cloud_http".to_string()),
        executor_type: Some("process_cli".to_string()),
        cloud_endpoint: Some("http://localhost:11434/v1".to_string()),
        cloud_model: Some("qwen2.5-coder:3b".to_string()),
        api_key: Some(String::new()),
        cli_executable: Some("opencode".to_string()),
        cli_args: Some(vec![
            "run".to_string(),
            "--model".to_string(),
            "ollama/qwen2.5-coder:3b".to_string(),
            "--auto".to_string(),
        ]),
        prompt_delivery: Some(PromptDelivery::Argument),
        verification_command: Some("cargo test".to_string()),
        ignored_dirs: Some(archify_lib::verification::default_ignored_dirs()),
        max_iterations: Some(2),
        ..Default::default()
    };

    {
        let mut g = orch.lock().await;
        g.configure_providers(config).expect("configure providers");
    }

    begin_planning(orch.clone(), task.id.clone()).await.unwrap();

    assert!(
        wait_for_state(
            &orch,
            &task.id,
            archify_lib::models::TaskState::AwaitingApproval,
            180
        )
        .await,
        "Planning should reach AwaitingApproval"
    );

    approve_plan(orch.clone(), task.id.clone()).await.unwrap();

    // Wait for terminal state
    let mut terminal = false;
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(600) {
        let g = orch.lock().await;
        if let Some(t) = g.current_task.as_ref() {
            if t.id == task.id
                && (t.state == archify_lib::models::TaskState::Completed
                    || t.state == archify_lib::models::TaskState::Failed
                    || t.state == archify_lib::models::TaskState::Cancelled)
            {
                terminal = true;
                eprintln!("[real-e2e] TERMINAL state={:?}", t.state);
                if t.state == archify_lib::models::TaskState::Failed {
                    eprintln!("[real-e2e] error={:?}", t.error);
                }
                // Print changed files
                if let Some(ch) = &t.task_level_changes {
                    eprintln!("[real-e2e] changed_files={:?}", ch.all_paths());
                }
                // Print last iteration details
                if let Some(last) = t.iterations.last() {
                    if let Some(er) = &last.executor_result {
                        eprintln!(
                            "[real-e2e] exec exit={:?} timed_out={}",
                            er.exit_code, er.timed_out
                        );
                        eprintln!(
                            "[real-e2e] exec stdout (tail 500)={}",
                            er.stdout
                                .chars()
                                .rev()
                                .take(500)
                                .collect::<String>()
                                .chars()
                                .rev()
                                .collect::<String>()
                        );
                        eprintln!(
                            "[real-e2e] exec stderr (tail 500)={}",
                            er.stderr
                                .chars()
                                .rev()
                                .take(500)
                                .collect::<String>()
                                .chars()
                                .rev()
                                .collect::<String>()
                        );
                    }
                    if let Some(rr) = &last.reviewer_response {
                        eprintln!(
                            "[real-e2e] reviewer verdict={:?} findings={}",
                            rr.verdict, rr.findings
                        );
                    }
                    eprintln!(
                        "[real-e2e] verification_results={:?}",
                        last.verification_results
                    );
                }
                break;
            }
        }
        drop(g);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert!(terminal, "Task should reach a terminal state");

    let final_state = {
        let g = orch.lock().await;
        g.current_task.clone().unwrap()
    };
    eprintln!("[real-e2e] FINAL state={:?}", final_state.state);

    assert_eq!(
        final_state.state,
        archify_lib::models::TaskState::Completed,
        "Task should complete"
    );

    assert!(
        final_state.task_level_changes.is_some(),
        "task_level_changes should be recorded"
    );
    let changes = final_state.task_level_changes.as_ref().unwrap();
    assert!(
        !changes.all_paths().is_empty(),
        "Should have detected changed files"
    );
    eprintln!("[real-e2e] all_changed={:?}", changes.all_paths());

    let last_iter = final_state.iterations.last().unwrap();
    assert!(last_iter.executor_result.is_some());
    assert_eq!(
        last_iter.executor_result.as_ref().unwrap().exit_code,
        Some(0)
    );
    assert!(
        last_iter
            .verification_results
            .iter()
            .any(|v| v.status == archify_lib::models::VerificationStatus::Passed),
        "At least one verification should pass"
    );
    if let Some(rr) = &last_iter.reviewer_response {
        assert_eq!(
            rr.verdict,
            archify_lib::models::ReviewerVerdict::Complete
        );
    }
}

#[tokio::test]
#[ignore = "requires ollama+opencode and ~5 minutes; run explicitly with --ignored"]
async fn real_agent_opencode_ollama_controlled_failure() {
    // Create an isolated copy so the primary test project is not damaged.
    let base = std::env::temp_dir().join(format!("archify_cf_proj_{}", uuid::Uuid::new_v4()));
    let src = std::path::PathBuf::from(r"C:\Claude projects\ArchifyTestProject");
    std::fs::create_dir_all(&base).unwrap();
    let status = std::process::Command::new("xcopy")
        .args([
            "/E",
            "/I",
            "/Y",
            src.to_str().unwrap(),
            base.to_str().unwrap(),
        ])
        .status()
        .expect("xcopy");
    assert!(status.success(), "xcopy failed");
    // No marker; verification fails until the agent adds a `multiply` function.

    let orch = setup_orch("cf").await;

    let task = create_task(
        orch.clone(),
        "Add a subtract function with tests to src/main.rs. Keep the existing add function and its test. The subtract function should return a - b. Include a test named test_subtract_negative that checks subtract(-5, -3) == -2. Make cargo test pass.".to_string(),
        base.to_string_lossy().to_string(),
        TaskCreationOptions::default(),
    )
    .await
    .unwrap();

    let config = ProviderConfigInput {
        planner_type: Some("cloud_http".to_string()),
        executor_type: Some("process_cli".to_string()),
        cloud_endpoint: Some("http://localhost:11434/v1".to_string()),
        cloud_model: Some("qwen2.5-coder:3b".to_string()),
        api_key: Some(String::new()),
        cli_executable: Some("opencode".to_string()),
        cli_args: Some(vec![
            "run".to_string(),
            "--model".to_string(),
            "ollama/qwen2.5-coder:3b".to_string(),
            "--auto".to_string(),
        ]),
        prompt_delivery: Some(PromptDelivery::Argument),
        verification_command: Some(
            "powershell -NoProfile -Command \"if (-not (Select-String -Path src/main.rs -Pattern 'test_subtract_negative' -Quiet)) { Write-Output 'FAIL: missing test_subtract_negative'; exit 1 } else { cargo test }\"".to_string(),
        ),
        ignored_dirs: Some(archify_lib::verification::default_ignored_dirs()),
        max_iterations: Some(3),
        ..Default::default()
    };

    {
        let mut g = orch.lock().await;
        g.configure_providers(config).expect("configure providers");
    }

    begin_planning(orch.clone(), task.id.clone()).await.unwrap();

    assert!(
        wait_for_state(
            &orch,
            &task.id,
            archify_lib::models::TaskState::AwaitingApproval,
            180
        )
        .await,
        "Planning should reach AwaitingApproval"
    );

    approve_plan(orch.clone(), task.id.clone()).await.unwrap();

    let mut terminal = false;
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(600) {
        let g = orch.lock().await;
        if let Some(t) = g.current_task.as_ref() {
            if t.id == task.id
                && (t.state == archify_lib::models::TaskState::Completed
                    || t.state == archify_lib::models::TaskState::Failed
                    || t.state == archify_lib::models::TaskState::Cancelled)
            {
                terminal = true;
                eprintln!("[cf-e2e] TERMINAL state={:?}", t.state);
                if t.state == archify_lib::models::TaskState::Failed {
                    eprintln!("[cf-e2e] error={:?}", t.error);
                }
                if let Some(ch) = &t.task_level_changes {
                    eprintln!("[cf-e2e] changed_files={:?}", ch.all_paths());
                }
                if let Some(last) = t.iterations.last() {
                    if let Some(er) = &last.executor_result {
                        eprintln!(
                            "[cf-e2e] exec exit={:?} timed_out={}",
                            er.exit_code, er.timed_out
                        );
                        eprintln!(
                            "[cf-e2e] exec stdout (tail 300)={}",
                            er.stdout
                                .chars()
                                .rev()
                                .take(300)
                                .collect::<String>()
                                .chars()
                                .rev()
                                .collect::<String>()
                        );
                        eprintln!(
                            "[cf-e2e] exec stderr (tail 300)={}",
                            er.stderr
                                .chars()
                                .rev()
                                .take(300)
                                .collect::<String>()
                                .chars()
                                .rev()
                                .collect::<String>()
                        );
                    }
                    if let Some(rr) = &last.reviewer_response {
                        eprintln!(
                            "[cf-e2e] reviewer verdict={:?} findings={}",
                            rr.verdict, rr.findings
                        );
                    }
                    eprintln!(
                        "[cf-e2e] verification_results={:?}",
                        last.verification_results
                    );
                    eprintln!("[cf-e2e] iterations={}", t.iterations.len());
                }
                break;
            }
        }
        drop(g);
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    assert!(terminal, "Task should reach a terminal state");

    let final_state = {
        let g = orch.lock().await;
        g.current_task.clone().unwrap()
    };
    eprintln!("[cf-e2e] FINAL state={:?}", final_state.state);
    eprintln!("[cf-e2e] iteration_count={}", final_state.iterations.len());
    for (idx, it) in final_state.iterations.iter().enumerate() {
        eprintln!(
            "[cf-e2e] iter {} verification={:?}",
            idx + 1,
            it.verification_results
        );
        if let Some(rr) = &it.reviewer_response {
            eprintln!(
                "[cf-e2e] iter {} reviewer={:?}: {}",
                idx + 1,
                rr.verdict,
                rr.findings
            );
        }
        eprintln!(
            "[cf-e2e] iter {} changed={:?}",
            idx + 1,
            it.changed_files.all_paths()
        );
    }

    // The controlled failure should have triggered at least 2 iterations.
    assert!(
        final_state.iterations.len() >= 2,
        "Should have at least 2 iterations (initial + correction)"
    );
    assert_eq!(
        final_state.state,
        archify_lib::models::TaskState::Completed,
        "Task should complete after correction"
    );
}
