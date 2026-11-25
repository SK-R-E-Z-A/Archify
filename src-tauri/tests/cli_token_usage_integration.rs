//! Dedicated integration tests for connecting CLI-reported token usage to the
//! aggregate, persistent token-usage tracker.
//!
//! These exercise the real `Orchestrator::record_cli_usage` wiring rather than
//! only the tracker primitives, so they catch regressions where CLI results are
//! extracted but never reach analytics.

use archify_lib::models::{CliTokenUsage, ExecutorResult, TaskRecord};
use archify_lib::orchestrator::Orchestrator;
use archify_lib::token_usage::TokenUsageTracker;
use std::path::Path;
use std::sync::{Arc, Mutex};

const ITERATION_STARTED_AT: &str = "2026-01-01T00:00:00Z";
const TASK_ID: &str = "task-1";
const DEDUP_KEY: &str = "cli:task-1:task-iteration:1:2026-01-01T00:00:00Z";

fn task_record(id: &str, cloud_model: Option<&str>) -> TaskRecord {
    let mut value = serde_json::json!({
        "id": id,
        "user_task": "implement the feature",
        "project_root": ".",
        "state": "executing",
        "plan": null,
        "iteration": 1,
        "max_iterations": 3,
        "iterations": [],
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "error": null,
        "project_id": "project-1",
        "provider_profile_id": "provider-1",
        "source_session_id": "session-1"
    });
    if let Some(model) = cloud_model {
        value["execution_config"] = serde_json::json!({ "cloud_model": model });
    }
    serde_json::from_value(value).unwrap()
}

fn executor_result(token_usage: Option<CliTokenUsage>) -> ExecutorResult {
    ExecutorResult {
        exit_code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
        duration_ms: 4321,
        token_usage,
    }
}

fn claude_usage() -> CliTokenUsage {
    CliTokenUsage {
        input_tokens: Some(100),
        output_tokens: Some(20),
        cache_creation_input_tokens: Some(30),
        cache_read_input_tokens: Some(40),
        total_cost_usd: Some("0.05".to_string()),
        model: Some("claude-sonnet-4".to_string()),
        format: "claude_code_json".to_string(),
    }
}

fn orchestrator_with_tracker(
    dir: &Path,
    task: TaskRecord,
) -> (Orchestrator, Arc<Mutex<TokenUsageTracker>>) {
    let mut orch =
        Orchestrator::new_isolated(None, dir.join("history"), dir.join("provider_config.json"));
    orch.current_task = Some(task);
    let tracker = Arc::new(Mutex::new(TokenUsageTracker::new(
        dir.join("token_usage.json"),
    )));
    orch.set_token_usage_tracker(Some(tracker.clone()));
    (orch, tracker)
}

#[test]
fn claude_cli_usage_reaches_aggregates_with_full_attribution() {
    let dir = tempfile::tempdir().unwrap();
    let (orch, tracker) = orchestrator_with_tracker(dir.path(), task_record(TASK_ID, None));

    let recorded = orch.record_cli_usage(
        TASK_ID,
        "task-iteration:1",
        ITERATION_STARTED_AT,
        &executor_result(Some(claude_usage())),
    );
    assert!(recorded, "CLI usage should be recorded");

    let guard = tracker.lock().unwrap();
    let records = guard.get_all_records();
    assert_eq!(records.len(), 1);
    let record = &records[0];

    // Counts come only from reported values; Claude cache tokens are included
    // because they are reported in addition to input_tokens.
    assert_eq!(record.usage.input_tokens, Some(170));
    assert_eq!(record.usage.output_tokens, Some(20));
    assert_eq!(record.usage.total_tokens, Some(190));
    assert!(!record.usage.tokens_unknown);
    assert_eq!(record.usage.duration_ms, Some(4321));

    // Attribution is preserved end to end.
    assert_eq!(record.context.provider_id.as_deref(), Some("provider-1"));
    assert_eq!(record.context.model_id.as_deref(), Some("claude-sonnet-4"));
    assert_eq!(record.context.project_id.as_deref(), Some("project-1"));
    assert_eq!(record.context.task_id.as_deref(), Some(TASK_ID));
    assert_eq!(record.context.session_id.as_deref(), Some("session-1"));
    assert_eq!(record.context.responsibility.as_deref(), Some("coding"));
    assert_eq!(record.operation_type, "execute");
    assert_eq!(record.dedup_key.as_deref(), Some(DEDUP_KEY));
    assert_eq!(
        record.context.metadata.get("source").map(String::as_str),
        Some("cli")
    );
    assert_eq!(
        record
            .context
            .metadata
            .get("usage_source")
            .map(String::as_str),
        Some("reported")
    );
    assert_eq!(
        record
            .context
            .metadata
            .get("usage_format")
            .map(String::as_str),
        Some("claude_code_json")
    );
    assert_eq!(
        record
            .context
            .metadata
            .get("usage_completeness")
            .map(String::as_str),
        Some("complete")
    );
    assert_eq!(
        record.context.metadata.get("exit_code").map(String::as_str),
        Some("0")
    );
    assert_eq!(
        record
            .context
            .metadata
            .get("total_cost_usd")
            .map(String::as_str),
        Some("0.05")
    );

    // Aggregate statistics reflect the same record.
    let stats = guard.get_stats();
    assert_eq!(stats.total_input_tokens, 170);
    assert_eq!(stats.total_output_tokens, 20);
    assert_eq!(stats.total_tokens, 190);
    assert_eq!(stats.request_count, 1);
    assert_eq!(stats.unknown_request_count, 0);
    assert_eq!(
        stats.by_provider.get("provider-1").map(|s| s.input_tokens),
        Some(170)
    );
    assert_eq!(
        stats
            .by_model
            .get("claude-sonnet-4")
            .map(|s| s.output_tokens),
        Some(20)
    );
    assert_eq!(
        stats.by_project.get("project-1").map(|s| s.total_tokens),
        Some(190)
    );
    assert_eq!(
        stats.by_task.get(TASK_ID).map(|s| s.total_tokens),
        Some(190)
    );
}

#[test]
fn codex_cli_usage_is_recorded_without_double_counting_cached_input() {
    let dir = tempfile::tempdir().unwrap();
    let (orch, tracker) = orchestrator_with_tracker(dir.path(), task_record(TASK_ID, None));

    let usage = CliTokenUsage {
        input_tokens: Some(100),
        output_tokens: Some(25),
        cache_read_input_tokens: Some(60),
        model: Some("gpt-5-codex".to_string()),
        format: "codex_jsonl".to_string(),
        ..Default::default()
    };
    assert!(orch.record_cli_usage(
        TASK_ID,
        "task-iteration:1",
        ITERATION_STARTED_AT,
        &executor_result(Some(usage)),
    ));

    let guard = tracker.lock().unwrap();
    let record = &guard.get_all_records()[0];
    // Cached input is a subset of input, so it is metadata, not extra tokens.
    assert_eq!(record.usage.input_tokens, Some(100));
    assert_eq!(record.usage.output_tokens, Some(25));
    assert_eq!(record.usage.total_tokens, Some(125));
    assert_eq!(
        record
            .context
            .metadata
            .get("cache_read_input_tokens")
            .map(String::as_str),
        Some("60")
    );
    let stats = guard.get_stats();
    assert_eq!(stats.total_input_tokens, 100);
    assert_eq!(stats.total_output_tokens, 25);
}

#[test]
fn missing_cli_usage_is_recorded_as_unknown_not_estimated() {
    let dir = tempfile::tempdir().unwrap();
    let (orch, tracker) = orchestrator_with_tracker(dir.path(), task_record(TASK_ID, None));

    assert!(orch.record_cli_usage(
        TASK_ID,
        "task-iteration:1",
        ITERATION_STARTED_AT,
        &executor_result(None),
    ));

    let guard = tracker.lock().unwrap();
    let record = &guard.get_all_records()[0];
    assert!(record.usage.tokens_unknown);
    assert_eq!(record.usage.input_tokens, None);
    assert_eq!(record.usage.output_tokens, None);
    assert_eq!(record.usage.total_tokens, None);
    assert_eq!(
        record
            .context
            .metadata
            .get("usage_source")
            .map(String::as_str),
        Some("unsupported")
    );
    let stats = guard.get_stats();
    assert_eq!(stats.request_count, 1);
    assert_eq!(stats.unknown_request_count, 1);
    assert_eq!(stats.total_input_tokens, 0);
    assert_eq!(stats.total_output_tokens, 0);
}

#[test]
fn repeated_result_is_counted_once_but_a_new_iteration_is_counted() {
    let dir = tempfile::tempdir().unwrap();
    let (orch, tracker) = orchestrator_with_tracker(dir.path(), task_record(TASK_ID, None));
    let result = executor_result(Some(claude_usage()));

    assert!(orch.record_cli_usage(TASK_ID, "task-iteration:1", ITERATION_STARTED_AT, &result,));
    // Same result replayed (e.g. repeated event handling) must not double-count.
    assert!(!orch.record_cli_usage(TASK_ID, "task-iteration:1", ITERATION_STARTED_AT, &result,));
    assert_eq!(tracker.lock().unwrap().count(), 1);

    // A genuinely new iteration has a new timestamp and must be counted.
    assert!(orch.record_cli_usage(TASK_ID, "task-iteration:2", "2026-01-01T00:10:00Z", &result,));
    assert_eq!(tracker.lock().unwrap().count(), 2);
    let stats = tracker.lock().unwrap().get_stats();
    assert_eq!(stats.total_input_tokens, 340);
    assert_eq!(stats.total_output_tokens, 40);
    assert_eq!(stats.request_count, 2);
}

#[test]
fn persisted_cli_usage_survives_reload_and_stays_deduplicated() {
    let dir = tempfile::tempdir().unwrap();
    {
        let (orch, tracker) = orchestrator_with_tracker(dir.path(), task_record(TASK_ID, None));
        assert!(orch.record_cli_usage(
            TASK_ID,
            "task-iteration:1",
            ITERATION_STARTED_AT,
            &executor_result(Some(claude_usage())),
        ));
        // Recording already persists; assert the file exists.
        assert!(dir.path().join("token_usage.json").exists());
        assert_eq!(tracker.lock().unwrap().count(), 1);
    }

    // Simulate an app restart: a fresh tracker at the same path loads history.
    let mut reloaded = TokenUsageTracker::new(dir.path().join("token_usage.json"));
    reloaded.load().unwrap();
    assert_eq!(reloaded.count(), 1);
    assert!(reloaded.has_dedup_key(DEDUP_KEY));
    let stats = reloaded.get_stats();
    assert_eq!(stats.total_input_tokens, 170);
    assert_eq!(stats.request_count, 1);

    // A replayed result after restart is still deduplicated.
    let tracker = Arc::new(Mutex::new(reloaded));
    let mut orch = Orchestrator::new_isolated(
        None,
        dir.path().join("history"),
        dir.path().join("provider_config.json"),
    );
    orch.current_task = Some(task_record(TASK_ID, None));
    orch.set_token_usage_tracker(Some(tracker.clone()));
    assert!(!orch.record_cli_usage(
        TASK_ID,
        "task-iteration:1",
        ITERATION_STARTED_AT,
        &executor_result(Some(claude_usage())),
    ));
    assert_eq!(tracker.lock().unwrap().count(), 1);
}

#[test]
fn configured_model_is_used_when_cli_reports_none() {
    let dir = tempfile::tempdir().unwrap();
    let (orch, tracker) =
        orchestrator_with_tracker(dir.path(), task_record(TASK_ID, Some("configured-model")));

    let usage = CliTokenUsage {
        input_tokens: Some(10),
        output_tokens: Some(5),
        format: "claude_code_json".to_string(),
        ..Default::default()
    };
    assert!(orch.record_cli_usage(
        TASK_ID,
        "task-iteration:1",
        ITERATION_STARTED_AT,
        &executor_result(Some(usage)),
    ));

    let guard = tracker.lock().unwrap();
    let record = &guard.get_all_records()[0];
    assert_eq!(record.context.model_id.as_deref(), Some("configured-model"));
}

/// End-to-end: a real child process emits Claude Code structured output, the
/// executor extracts it, and the orchestrator records it in aggregates.
#[cfg(windows)]
#[tokio::test]
async fn real_process_executor_usage_flows_into_aggregates() {
    use archify_lib::models::PromptDelivery;
    use archify_lib::providers::CodingExecutor;
    use archify_lib::providers::process_cli::ProcessCliExecutor;
    use tokio::sync::watch;

    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("emit-usage.cmd");
    std::fs::write(
        &script,
        "@echo off\r\necho {\"type\":\"result\",\"usage\":{\"input_tokens\":13,\"output_tokens\":4},\"model\":\"claude-sonnet-4\"}\r\n",
    )
    .unwrap();

    let executor = ProcessCliExecutor::with_prompt_delivery(
        "cmd".to_string(),
        vec!["/C".to_string(), script.to_string_lossy().to_string()],
        PromptDelivery::Stdin,
        Some(30),
        Some(64 * 1024),
    );
    let (_tx, mut rx) = watch::channel(false);
    let result = executor
        .execute("", dir.path().to_str().unwrap(), None, &mut rx)
        .await
        .unwrap();
    assert!(result.token_usage.is_some());

    let (orch, tracker) = orchestrator_with_tracker(dir.path(), task_record(TASK_ID, None));
    assert!(orch.record_cli_usage(TASK_ID, "task-iteration:1", ITERATION_STARTED_AT, &result,));

    let guard = tracker.lock().unwrap();
    let record = &guard.get_all_records()[0];
    assert_eq!(record.usage.input_tokens, Some(13));
    assert_eq!(record.usage.output_tokens, Some(4));
    assert_eq!(record.usage.total_tokens, Some(17));
    assert_eq!(record.context.model_id.as_deref(), Some("claude-sonnet-4"));
    assert_eq!(guard.get_stats().total_tokens, 17);
}

#[test]
fn no_tracker_means_no_recording_and_no_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut orch = Orchestrator::new_isolated(
        None,
        dir.path().join("history"),
        dir.path().join("provider_config.json"),
    );
    orch.current_task = Some(task_record(TASK_ID, None));
    assert!(!orch.record_cli_usage(
        TASK_ID,
        "task-iteration:1",
        ITERATION_STARTED_AT,
        &executor_result(Some(claude_usage())),
    ));
}
