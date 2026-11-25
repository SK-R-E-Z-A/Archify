use archify_lib::models::{ExecutorResult, Plan, VerificationResult};
use archify_lib::providers::PlannerReviewer;
use archify_lib::providers::cloud_http::CloudHttpPlannerReviewer;
use archify_lib::token_usage::{TokenUsageTracker, UsageContext};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn mock_openai_response(
    body: String,
    status: &str,
    requests: usize,
) -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let status = status.to_string();
    let task = tokio::spawn(async move {
        let mut captured = Vec::new();
        for _ in 0..requests {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0; 1024];
            loop {
                let count = socket.read(&mut chunk).await.unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..count]);
                if let Some(header_end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_len = headers
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    if request.len() >= header_end + 4 + content_len {
                        break;
                    }
                }
            }
            captured.push(String::from_utf8_lossy(&request).into_owned());
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            socket.flush().await.unwrap();
        }
        captured
    });
    (format!("http://{address}/v1"), task)
}

fn client(endpoint: String, tracker: Arc<Mutex<TokenUsageTracker>>) -> CloudHttpPlannerReviewer {
    let mut client = CloudHttpPlannerReviewer::new(endpoint, "configured-model".to_string(), None);
    let context = UsageContext {
        provider_id: Some("provider-1".to_string()),
        provider_name: Some("Test provider".to_string()),
        model_id: Some("configured-model".to_string()),
        project_id: Some("project-1".to_string()),
        project_name: Some("Test project".to_string()),
        task_id: Some("task-1".to_string()),
        session_id: Some("session-1".to_string()),
        responsibility: Some("coding".to_string()),
        ..Default::default()
    };
    client.set_token_usage_tracker(Some(tracker));
    client.set_usage_context(context);
    client
}

fn tracker(path: &std::path::Path) -> Arc<Mutex<TokenUsageTracker>> {
    Arc::new(Mutex::new(TokenUsageTracker::new(path.join("usage.json"))))
}

fn openai_reply(content: &str, usage: Option<serde_json::Value>, id: &str) -> String {
    let mut response = serde_json::json!({
        "id": id,
        "model": "reported-model",
        "choices": [{"message": {"content": content}}]
    });
    if let Some(usage) = usage {
        response["usage"] = usage;
    }
    response.to_string()
}

#[tokio::test]
async fn chat_response_records_usage_attributed_and_persisted() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = tracker(dir.path());
    let response = openai_reply(
        "chat answer",
        Some(serde_json::json!({
            "prompt_tokens": 40,
            "completion_tokens": 12,
            "total_tokens": 52,
            "prompt_tokens_details": {"cached_tokens": 10}
        })),
        "chat-response-1",
    );
    let (endpoint, mock) = mock_openai_response(response, "200 OK", 1).await;
    let result = client(endpoint, tracker.clone())
        .chat(vec![("user".to_string(), "hello".to_string())])
        .await
        .unwrap();
    assert_eq!(result, "chat answer");

    let guard = tracker.lock().unwrap();
    let records = guard.get_all_records();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.usage.input_tokens, Some(40));
    assert_eq!(record.usage.output_tokens, Some(12));
    assert_eq!(record.usage.total_tokens, Some(52));
    assert_eq!(record.context.provider_id.as_deref(), Some("provider-1"));
    assert_eq!(record.context.model_id.as_deref(), Some("reported-model"));
    assert_eq!(record.context.project_id.as_deref(), Some("project-1"));
    assert_eq!(record.context.task_id.as_deref(), Some("task-1"));
    assert_eq!(record.context.session_id.as_deref(), Some("session-1"));
    assert_eq!(record.context.responsibility.as_deref(), Some("coding"));
    assert_eq!(record.operation_type, "chat");
    assert_eq!(
        record
            .context
            .metadata
            .get("prompt_cached_tokens")
            .map(String::as_str),
        Some("10")
    );
    assert!(dir.path().join("usage.json").exists());
    assert!(mock.await.unwrap()[0].contains("POST /v1/chat/completions"));
}

#[tokio::test]
async fn planning_response_records_usage_once_with_plan_attribution() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = tracker(dir.path());
    let plan = serde_json::json!({
        "summary": "Implement feature",
        "steps": ["Edit source"],
        "acceptance_criteria": ["Tests pass"]
    })
    .to_string();
    let response = openai_reply(
        &plan,
        Some(serde_json::json!({"prompt_tokens": 31, "completion_tokens": 9, "total_tokens": 40})),
        "plan-response-1",
    );
    let (endpoint, mock) = mock_openai_response(response, "200 OK", 1).await;
    let client = client(endpoint, tracker.clone());
    let plan_result = client.plan("implement", ".", "context").await.unwrap();
    assert_eq!(plan_result.summary, "Implement feature");

    let guard = tracker.lock().unwrap();
    assert_eq!(guard.count(), 1);
    let record = &guard.get_all_records()[0];
    assert_eq!(record.usage.input_tokens, Some(31));
    assert_eq!(record.usage.output_tokens, Some(9));
    assert_eq!(record.usage.total_tokens, Some(40));
    assert_eq!(record.operation_type, "plan");
    assert_eq!(record.context.model_id.as_deref(), Some("reported-model"));
    assert!(mock.await.unwrap()[0].contains("\"temperature\":0.2"));
}

#[tokio::test]
async fn partial_usage_records_only_reported_counts_without_inventing_totals() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = tracker(dir.path());
    let response = openai_reply(
        "partial",
        Some(serde_json::json!({"prompt_tokens": 17})),
        "partial-usage-response",
    );
    let (endpoint, mock) = mock_openai_response(response, "200 OK", 1).await;
    client(endpoint, tracker.clone())
        .chat(vec![("user".to_string(), "hello".to_string())])
        .await
        .unwrap();

    let guard = tracker.lock().unwrap();
    let record = &guard.get_all_records()[0];
    assert_eq!(record.usage.input_tokens, Some(17));
    assert_eq!(record.usage.output_tokens, None);
    assert_eq!(record.usage.total_tokens, None);
    assert_eq!(
        record
            .context
            .metadata
            .get("usage_completeness")
            .map(String::as_str),
        Some("partial")
    );
    assert_eq!(mock.await.unwrap().len(), 1);
}

#[tokio::test]
async fn review_response_records_usage_with_review_attribution() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = tracker(dir.path());
    let review = serde_json::json!({
        "verdict": "complete",
        "findings": "Criteria are met",
        "unresolved_issues": [],
        "suggested_fix": null
    })
    .to_string();
    let response = openai_reply(
        &review,
        Some(serde_json::json!({"prompt_tokens": 26, "completion_tokens": 7, "total_tokens": 33})),
        "review-response-1",
    );
    let (endpoint, mock) = mock_openai_response(response, "200 OK", 1).await;
    let client = client(endpoint, tracker.clone());
    let plan = Plan {
        summary: "Implement feature".to_string(),
        steps: vec!["Edit source".to_string()],
        acceptance_criteria: vec!["Tests pass".to_string()],
    };
    let executor = ExecutorResult {
        exit_code: Some(0),
        stdout: String::new(),
        stderr: String::new(),
        timed_out: false,
        duration_ms: 10,
        token_usage: None,
    };
    let review_result = client
        .review(
            "implement",
            &plan,
            &plan.acceptance_criteria,
            &[],
            None,
            &executor,
            &[] as &[VerificationResult],
        )
        .await
        .unwrap();
    assert_eq!(review_result.findings, "Criteria are met");

    let guard = tracker.lock().unwrap();
    assert_eq!(guard.count(), 1);
    let record = &guard.get_all_records()[0];
    assert_eq!(record.usage.input_tokens, Some(26));
    assert_eq!(record.usage.output_tokens, Some(7));
    assert_eq!(record.usage.total_tokens, Some(33));
    assert_eq!(record.operation_type, "review");
    assert_eq!(record.context.provider_id.as_deref(), Some("provider-1"));
    assert!(mock.await.unwrap()[0].contains("\"temperature\":0.1"));
}

#[tokio::test]
async fn missing_usage_records_unknown_counts_and_failed_api_does_not_record() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = tracker(dir.path());
    let missing_usage = openai_reply("no usage", None, "missing-usage-response");
    let (endpoint, mock) = mock_openai_response(missing_usage, "200 OK", 1).await;
    client(endpoint, tracker.clone())
        .chat(vec![("user".to_string(), "hello".to_string())])
        .await
        .unwrap();

    let failed =
        serde_json::json!({"id":"failed-response","error":{"message":"bad request"}}).to_string();
    let (endpoint, failed_mock) = mock_openai_response(failed, "400 Bad Request", 1).await;
    assert!(
        client(endpoint, tracker.clone())
            .chat(vec![("user".to_string(), "hello".to_string())])
            .await
            .is_err()
    );

    let guard = tracker.lock().unwrap();
    assert_eq!(guard.count(), 1);
    let record = &guard.get_all_records()[0];
    assert!(record.usage.tokens_unknown);
    assert_eq!(record.usage.input_tokens, None);
    assert_eq!(record.usage.output_tokens, None);
    assert_eq!(record.usage.total_tokens, None);
    assert_eq!(guard.get_stats().unknown_request_count, 1);
    assert_eq!(mock.await.unwrap().len(), 1);
    assert_eq!(failed_mock.await.unwrap().len(), 1);
    drop(guard);

    let api_error = serde_json::json!({
        "id": "api-error-response",
        "error": {"message": "provider rejected the request"}
    })
    .to_string();
    let (endpoint, api_error_mock) = mock_openai_response(api_error, "200 OK", 1).await;
    assert!(
        client(endpoint, tracker.clone())
            .chat(vec![("user".to_string(), "hello".to_string())])
            .await
            .is_err()
    );
    assert_eq!(tracker.lock().unwrap().count(), 1);
    assert_eq!(api_error_mock.await.unwrap().len(), 1);
}

#[tokio::test]
async fn response_id_dedup_prevents_duplicate_accounting_and_preserves_cache_semantics() {
    let dir = tempfile::tempdir().unwrap();
    let tracker = tracker(dir.path());
    let anthropic_usage = serde_json::json!({
        "input_tokens": 40,
        "output_tokens": 8,
        "cache_creation_input_tokens": 5,
        "cache_read_input_tokens": 20
    });
    let response = openai_reply("answer", Some(anthropic_usage), "shared-response-id");
    let (endpoint, mock) = mock_openai_response(response, "200 OK", 2).await;
    let client = client(endpoint, tracker.clone());
    for _ in 0..2 {
        client
            .chat(vec![("user".to_string(), "same request".to_string())])
            .await
            .unwrap();
    }

    let guard = tracker.lock().unwrap();
    assert_eq!(guard.count(), 1);
    let record = &guard.get_all_records()[0];
    assert_eq!(record.usage.input_tokens, Some(65));
    assert_eq!(record.usage.output_tokens, Some(8));
    assert_eq!(record.usage.total_tokens, Some(73));
    assert_eq!(
        record
            .context
            .metadata
            .get("cache_creation_input_tokens")
            .map(String::as_str),
        Some("5")
    );
    assert_eq!(
        record
            .context
            .metadata
            .get("cache_read_input_tokens")
            .map(String::as_str),
        Some("20")
    );
    assert_eq!(guard.get_stats().request_count, 1);
    assert_eq!(mock.await.unwrap().len(), 2);
    drop(guard);

    let mut reloaded = TokenUsageTracker::new(dir.path().join("usage.json"));
    reloaded.load().unwrap();
    assert_eq!(reloaded.count(), 1);
    assert!(reloaded.has_dedup_key("http:chat:shared-response-id"));
}
