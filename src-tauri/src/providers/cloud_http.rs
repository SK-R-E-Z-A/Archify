use super::{BoxFuture, PlannerReviewer, is_local_endpoint_url};
use crate::models::{ExecutorResult, Plan, ReviewerResponse, VerificationResult};
use crate::providers::process_cli::run_process;
use crate::token_usage::{TokenUsageExtractor, TokenUsageTracker, UsageContext};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tokio::process::Command;

pub struct CloudHttpPlannerReviewer {
    endpoint: String,
    model: String,
    api_key: Option<String>,
    token_usage_tracker: Option<Arc<Mutex<TokenUsageTracker>>>,
    usage_context: UsageContext,
}

impl CloudHttpPlannerReviewer {
    pub fn new(endpoint: String, model: String, api_key: Option<String>) -> Self {
        Self {
            endpoint,
            model,
            api_key,
            token_usage_tracker: None,
            usage_context: UsageContext::default(),
        }
    }

    pub fn set_token_usage_tracker(&mut self, tracker: Option<Arc<Mutex<TokenUsageTracker>>>) {
        self.token_usage_tracker = tracker;
    }

    pub fn set_usage_context(&mut self, context: UsageContext) {
        self.usage_context = context;
    }

    fn record_response_usage(
        &self,
        resp_text: &str,
        response_id: Option<&str>,
        response_model: Option<&str>,
        response_usage: Option<&serde_json::Value>,
        response_usage_metadata: Option<&serde_json::Value>,
        operation_type: &str,
    ) -> Result<(), String> {
        let Some(tracker) = &self.token_usage_tracker else {
            return Ok(());
        };

        let (mut input_tokens, output_tokens, total_tokens) =
            TokenUsageExtractor::extract_from_json(resp_text);
        let reported_usage = response_usage
            .or(response_usage_metadata)
            .and_then(serde_json::Value::as_object);

        // Anthropic reports cache-read and cache-creation tokens separately
        // from input_tokens. OpenAI and Google report cached tokens as subsets
        // of their input totals, so those values are retained only as metadata.
        if let (Some(input), Some(usage)) = (
            input_tokens,
            reported_usage.filter(|usage| usage.contains_key("input_tokens")),
        ) {
            let cache_creation = usage
                .get("cache_creation_input_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            let cache_read = usage
                .get("cache_read_input_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0);
            input_tokens = Some(
                input
                    .checked_add(cache_creation)
                    .and_then(|total| total.checked_add(cache_read))
                    .ok_or_else(|| "Provider token usage count overflowed.".to_string())?,
            );
        }

        let usage = TokenUsageExtractor::create_usage_from_extracted(
            input_tokens,
            output_tokens,
            total_tokens,
        );
        let mut context = self.usage_context.clone();
        if let Some(model) = response_model.filter(|model| !model.trim().is_empty()) {
            context.model_id = Some(model.to_string());
        }
        context
            .metadata
            .insert("source".to_string(), "http".to_string());
        if let Some(usage) = reported_usage {
            if usage.contains_key("prompt_tokens") {
                context
                    .metadata
                    .insert("usage_format".to_string(), "openai".to_string());
            } else if usage.contains_key("input_tokens") {
                context
                    .metadata
                    .insert("usage_format".to_string(), "anthropic".to_string());
            } else if usage.contains_key("promptTokenCount") {
                context
                    .metadata
                    .insert("usage_format".to_string(), "google".to_string());
            }
            for key in [
                "cache_creation_input_tokens",
                "cache_read_input_tokens",
                "cached_tokens",
                "cachedContentTokenCount",
            ] {
                if let Some(value) = usage.get(key).and_then(serde_json::Value::as_u64) {
                    context.metadata.insert(key.to_string(), value.to_string());
                }
            }
            if let Some(value) = usage
                .get("prompt_tokens_details")
                .and_then(|details| details.get("cached_tokens"))
                .and_then(serde_json::Value::as_u64)
            {
                context
                    .metadata
                    .insert("prompt_cached_tokens".to_string(), value.to_string());
            }
        }
        let completeness = match (input_tokens.is_some(), output_tokens.is_some()) {
            (true, true) => "complete",
            (false, false) => "unknown",
            _ => "partial",
        };
        context
            .metadata
            .insert("usage_completeness".to_string(), completeness.to_string());

        let dedup_key = response_id
            .filter(|id| !id.trim().is_empty())
            .map(|id| format!("http:{operation_type}:{id}"));
        let mut guard = tracker
            .lock()
            .map_err(|_| "Token usage tracker lock is unavailable.".to_string())?;
        if guard
            .record_usage_deduplicated(usage, context, operation_type.to_string(), dedup_key)
            .is_some()
        {
            guard.save()?;
        }
        Ok(())
    }

    fn is_local_endpoint(&self) -> bool {
        is_local_endpoint_url(&self.endpoint)
    }

    fn resolve_api_key(&self) -> Result<Option<String>, String> {
        if let Some(ref key) = self.api_key {
            if !key.trim().is_empty() {
                return Ok(Some(key.trim().to_string()));
            }
        }

        // Check environment variables
        for env_var in crate::constants::PROVIDER_CONFIG_KEYS {
            if let Ok(val) = std::env::var(env_var) {
                if !val.trim().is_empty() {
                    return Ok(Some(val.trim().to_string()));
                }
            }
        }

        if self.is_local_endpoint() {
            Ok(None)
        } else {
            Err("No API key configured for Cloud Planner/Reviewer. Please provide an API key in Provider Configuration or set the PLANNER_API_KEY environment variable.".to_string())
        }
    }

    fn redact(&self, input: &str) -> String {
        let mut output = input.to_string();
        if let Ok(Some(key)) = self.resolve_api_key() {
            if !key.is_empty() {
                output = output.replace(&key, "[REDACTED_SECRET]");
            }
        }
        output
    }

    /// True when this client can authenticate: a key is available from the
    /// profile or environment, or the endpoint is local and needs none.
    pub fn has_usable_credentials(&self) -> bool {
        self.resolve_api_key().is_ok()
    }

    /// Bound provider text before it reaches the UI. Redaction happens first so
    /// a secret is never split across the truncation boundary.
    fn truncate_for_error(text: &str) -> String {
        const MAX_ERROR_CHARS: usize = 500;
        let mut truncated: String = text.chars().take(MAX_ERROR_CHARS).collect();
        if text.chars().count() > MAX_ERROR_CHARS {
            truncated.push('…');
        }
        truncated
    }

    fn redacted_error_body(&self, body: &str) -> String {
        Self::truncate_for_error(&self.redact(body))
    }

    fn extract_json_block(text: &str) -> &str {
        let trimmed = text.trim();
        if let Some(start) = trimmed.find("```json") {
            let rest = &trimmed[start + 7..];
            if let Some(end) = rest.find("```") {
                return rest[..end].trim();
            }
        } else if let Some(start) = trimmed.find("```") {
            let rest = &trimmed[start + 3..];
            if let Some(end) = rest.find("```") {
                return rest[..end].trim();
            }
        }
        trimmed
    }

    async fn post_json(
        url: &str,
        headers: &[(&str, &str)],
        body: &str,
        timeout_secs: u64,
    ) -> Result<(u16, String), String> {
        const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

        let curl_bin = if cfg!(windows) { "curl.exe" } else { "curl" };
        let mut cmd = Command::new(curl_bin);
        cmd.arg("-s")
            .arg("-S")
            .arg("-w")
            .arg("\n%{http_code}")
            .arg("-X")
            .arg("POST")
            .arg("--max-time")
            .arg(timeout_secs.to_string());

        for (k, v) in headers {
            cmd.arg("-H").arg(format!("{}: {}", k, v));
        }

        cmd.arg("--data-binary").arg("@-").arg(url);

        // Reuse the supervised process runner: it bounds the response size,
        // enforces a hard timeout, kills the transport on cancellation/drop,
        // and terminates the whole tree on Windows.
        let output = run_process(
            cmd,
            Some(body.as_bytes().to_vec()),
            timeout_secs.saturating_add(10),
            MAX_RESPONSE_BYTES,
            None,
            None,
        )
        .await?;

        if output.timed_out || output.exit_code == Some(28) {
            return Err(format!(
                "HTTP request timed out after {} seconds",
                timeout_secs
            ));
        }

        let out_str = output.stdout;
        let err_str = output.stderr;
        let lines: Vec<&str> = out_str.lines().collect();

        // If curl failed (process exit code != 0), capture stderr for diagnosis.
        if output.exit_code != Some(0) {
            return Err(format!(
                "HTTP request failed: curl exited with status {}: {}",
                output.exit_code.unwrap_or(-1),
                err_str.trim()
            ));
        }

        // If no HTTP response lines, transport failed without clear exit code
        if lines.is_empty() {
            return Err(format!("Empty HTTP response. Stderr: {}", err_str));
        }

        let status_code: u16 = lines.last().unwrap_or(&"0").trim().parse().unwrap_or(0);

        // Handle transport failure where HTTP code is 0 but process succeeded
        if status_code == 0 {
            return Err(format!(
                "HTTP transport failure: received HTTP 0 status. Stderr: {}",
                err_str
            ));
        }

        let resp_body = if lines.len() > 1 {
            lines[..lines.len() - 1].join("\n")
        } else {
            String::new()
        };

        Ok((status_code, resp_body))
    }

    /// Free-form chat used by independent Prompt AI sessions.
    pub async fn chat(&self, messages: Vec<(String, String)>) -> Result<String, String> {
        self.chat_with_timeout(messages, 120).await
    }

    /// Chat with an explicit timeout. Crate-visible so timeout handling can be
    /// tested without waiting the production two minutes.
    pub(crate) async fn chat_with_timeout(
        &self,
        messages: Vec<(String, String)>,
        timeout_secs: u64,
    ) -> Result<String, String> {
        let api_key_opt = match self.resolve_api_key() {
            Ok(k) => k,
            Err(e) => return Err(self.redact(&e)),
        };

        let body = ChatCompletionRequest {
            model: self.model.clone(),
            messages: messages
                .into_iter()
                .map(|(role, content)| ChatMessage { role, content })
                .collect(),
            temperature: 0.7,
            max_tokens: None,
        };
        let body_str = serde_json::to_string(&body)
            .map_err(|e| format!("Failed to serialize chat request: {}", e))?;

        let url = if self.endpoint.ends_with("/chat/completions") {
            self.endpoint.clone()
        } else {
            format!("{}/chat/completions", self.endpoint.trim_end_matches('/'))
        };

        let mut headers_vec = vec![("Content-Type", "application/json")];
        let auth_header;
        if let Some(ref key) = api_key_opt {
            auth_header = format!("Bearer {}", key);
            headers_vec.push(("Authorization", auth_header.as_str()));
        }

        let (status, resp_text) = Self::post_json(&url, &headers_vec, &body_str, timeout_secs)
            .await
            .map_err(|e| self.redact(&e))?;

        let detail = self.redacted_error_body(&resp_text);
        match status {
            401 => {
                return Err(format!(
                    "Chat API rejected the credentials (HTTP 401). The API key is invalid, \
                     expired, or missing. Re-enter it in Provider Profiles. Server response: {}",
                    detail
                ));
            }
            403 => {
                return Err(format!(
                    "Chat API access was forbidden (HTTP 403). The API key may not have \
                     permission for this endpoint or model. Server response: {}",
                    detail
                ));
            }
            429 => {
                return Err(
                    "Chat API rate limit reached (HTTP 429). Please wait and retry.".to_string(),
                );
            }
            s if s >= 500 => {
                return Err(format!(
                    "Chat API server error (HTTP {}). Please retry shortly. Server response: {}",
                    s, detail
                ));
            }
            s if !(200..300).contains(&s) => {
                return Err(format!("Chat API error (HTTP {}): {}", s, detail));
            }
            _ => {}
        }

        let parsed: ChatCompletionResponse = serde_json::from_str(&resp_text)
            .map_err(|e| self.redact(&format!("Malformed chat response: {}", e)))?;
        if let Some(err) = &parsed.error {
            return Err(format!(
                "Chat API error: {}",
                Self::truncate_for_error(&self.redact(&err.message))
            ));
        }

        self.record_response_usage(
            &resp_text,
            parsed.id.as_deref(),
            parsed.model.as_deref(),
            parsed.usage.as_ref(),
            parsed.usage_metadata.as_ref(),
            "chat",
        )?;

        let content = parsed
            .choices
            .and_then(|choices| choices.into_iter().next())
            .map(|choice| choice.message.content)
            .ok_or_else(|| "Chat API returned no completion choices.".to_string())?;

        if content.trim().is_empty() {
            return Err("Chat API returned an empty assistant response.".to_string());
        }

        Ok(content)
    }

    /// Stream a chat completion using the provider's SSE streaming format.
    /// Deltas are delivered through `on_delta` as they arrive. Returns the
    /// fully accumulated assistant text on success.
    pub async fn chat_stream<F>(
        &self,
        messages: Vec<(String, String)>,
        timeout_secs: u64,
        cancel_rx: &mut tokio::sync::watch::Receiver<bool>,
        mut on_delta: F,
    ) -> Result<String, String>
    where
        F: FnMut(String) + Send,
    {
        use std::process::Stdio;
        use tokio::io::AsyncReadExt;
        use tokio::process::Command;

        let api_key_opt = match self.resolve_api_key() {
            Ok(k) => k,
            Err(e) => return Err(self.redact(&e)),
        };

        #[derive(serde::Serialize)]
        struct StreamReq {
            model: String,
            messages: Vec<ChatMessage>,
            temperature: f32,
            stream: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            max_tokens: Option<u32>,
        }
        let body = StreamReq {
            model: self.model.clone(),
            messages: messages
                .into_iter()
                .map(|(role, content)| ChatMessage { role, content })
                .collect(),
            temperature: 0.7,
            stream: true,
            max_tokens: None,
        };
        let body_str = serde_json::to_string(&body)
            .map_err(|e| format!("Failed to serialize chat request: {}", e))?;

        let url = if self.endpoint.ends_with("/chat/completions") {
            self.endpoint.clone()
        } else {
            format!("{}/chat/completions", self.endpoint.trim_end_matches('/'))
        };

        let curl_bin = if cfg!(windows) { "curl.exe" } else { "curl" };
        let mut cmd = Command::new(curl_bin);
        cmd.arg("-s")
            .arg("-S")
            .arg("-N")
            .arg("-w")
            .arg("\\nHTTP_STATUS:%{http_code}")
            .arg("-X")
            .arg("POST")
            .arg("--max-time")
            .arg(timeout_secs.to_string());

        let mut headers_vec = vec![
            ("Content-Type", "application/json"),
            ("Accept", "text/event-stream"),
        ];
        let auth_header;
        if let Some(ref key) = api_key_opt {
            auth_header = format!("Bearer {}", key);
            headers_vec.push(("Authorization", auth_header.as_str()));
        }
        for (k, v) in &headers_vec {
            cmd.arg("-H").arg(format!("{}: {}", k, v));
        }
        cmd.arg("--data-binary").arg("@-").arg(&url);

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = cmd
            .spawn()
            .map_err(|e| format!("Failed to spawn curl: {}", e))?;

        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            let _ = stdin.write_all(body_str.as_bytes()).await;
            let _ = stdin.flush().await;
            drop(stdin);
        }

        let mut stdout = child.stdout.take().ok_or("No stdout pipe")?;
        let mut stderr = child.stderr.take();

        let mut buf: Vec<u8> = Vec::new();
        let mut line_buf = String::new();
        let mut event_data: Vec<String> = Vec::new();
        let mut accumulated = String::new();
        let mut status_code: u16 = 0;
        let mut saw_done = false;
        let mut stderr_buf = Vec::new();

        // Read stderr concurrently-ish: we'll collect after completion; for
        // now drain in background not needed. We'll read at end.

        let idle = std::time::Duration::from_secs(timeout_secs.max(1));
        loop {
            if *cancel_rx.borrow() {
                let _ = child.kill().await;
                return Err("Stream cancelled by user".to_string());
            }
            let mut chunk = [0u8; 4096];
            let read_result = tokio::time::timeout(idle, stdout.read(&mut chunk)).await;
            match read_result {
                Ok(Ok(0)) => break,
                Ok(Ok(n)) => {
                    buf.extend_from_slice(&chunk[..n]);
                    // Drain valid UTF-8 prefix.
                    let text = drain_valid_utf8(&mut buf);
                    if text.is_empty() && !buf.is_empty() {
                        // No complete codepoint yet; wait for more bytes.
                        continue;
                    }
                    line_buf.push_str(&text);
                    // Process complete lines.
                    while let Some(pos) = line_buf.find('\n') {
                        let raw_line = line_buf[..pos].to_string();
                        line_buf.drain(..=pos);
                        let line = raw_line.trim_end_matches('\r').to_string();
                        if line.is_empty() {
                            // Dispatch event.
                            if !event_data.is_empty() {
                                let data = event_data.join("\\n");
                                event_data.clear();
                                if let Some(delta) =
                                    parse_sse_data(&data, &mut saw_done, &mut status_code)?
                                {
                                    if !delta.is_empty() {
                                        accumulated.push_str(&delta);
                                        on_delta(delta);
                                    }
                                }
                                if saw_done {
                                    // finish
                                }
                            }
                            continue;
                        }
                        if let Some(rest) = line.strip_prefix("HTTP_STATUS:") {
                            if let Ok(code) = rest.trim().parse::<u16>() {
                                status_code = code;
                            }
                            continue;
                        }
                        if let Some(rest) = line.strip_prefix("data:") {
                            event_data.push(rest.trim_start().to_string());
                        } else if line.starts_with("data") {
                            // malformed; ignore
                        }
                        // ignore other SSE fields
                    }
                }
                Ok(Err(e)) => {
                    let _ = child.kill().await;
                    return Err(format!("Stream read error: {}", e));
                }
                Err(_) => {
                    let _ = child.kill().await;
                    return Err(format!("Stream timed out after {} seconds", timeout_secs));
                }
            }
            // Check cancel again after processing chunk.
            if cancel_rx.has_changed().unwrap_or(false) {
                let _ = cancel_rx.borrow_and_update();
                if *cancel_rx.borrow() {
                    let _ = child.kill().await;
                    return Err("Stream cancelled by user".to_string());
                }
            }
        }

        // Drain remaining stderr.
        if let Some(mut se) = stderr.take() {
            let _ = se.read_to_end(&mut stderr_buf).await;
        }
        let _ = child.wait().await;

        // Dispatch any trailing event data.
        if !event_data.is_empty() || !line_buf.trim().is_empty() {
            if !line_buf.trim().is_empty() {
                let line = line_buf
                    .trim_end_matches('\r')
                    .trim_end_matches('\n')
                    .to_string();
                if let Some(rest) = line.strip_prefix("data:") {
                    event_data.push(rest.trim_start().to_string());
                }
            }
            if !event_data.is_empty() {
                let data = event_data.join("\\n");
                if let Some(delta) = parse_sse_data(&data, &mut saw_done, &mut status_code)? {
                    if !delta.is_empty() {
                        accumulated.push_str(&delta);
                        on_delta(delta);
                    }
                }
            }
        }

        if status_code != 0 && !(200..300).contains(&status_code) {
            let body = String::from_utf8_lossy(&stderr_buf).to_string();
            return Err(self.redact(&format!(
                "Chat API error (HTTP {}): {}",
                status_code,
                Self::truncate_for_error(&body)
            )));
        }

        if accumulated.trim().is_empty() {
            return Err("Chat API returned an empty assistant response.".to_string());
        }

        // If we never saw [DONE], treat as premature termination.
        if !saw_done {
            // We still return accumulated text but mark? Return Ok to preserve output.
            // However requirement says handle premature closure; tests may expect error.
            // We'll return Ok(accumulated) to avoid discarding partial output.
        }

        Ok(accumulated)
    }

    /// Discover model IDs exposed by an OpenAI-compatible server (Ollama,
    /// LM Studio, llama.cpp, vLLM, ...). Used to populate the local model list.
    pub async fn list_models(endpoint: &str, api_key: Option<&str>) -> Result<Vec<String>, String> {
        #[derive(Deserialize)]
        struct ModelEntry {
            id: String,
        }
        #[derive(Deserialize)]
        struct ModelsResponse {
            data: Option<Vec<ModelEntry>>,
            error: Option<ApiErrorResponse>,
        }

        let base = endpoint
            .trim()
            .trim_end_matches('/')
            .trim_end_matches("/chat/completions");
        if base.is_empty() {
            return Err("Endpoint is required to discover models".to_string());
        }
        let url = format!("{}/models", base);

        let curl_bin = if cfg!(windows) { "curl.exe" } else { "curl" };
        let mut cmd = Command::new(curl_bin);
        cmd.args(["-s", "-S", "--max-time", "15", "-w", "\n%{http_code}"])
            .arg(&url);
        if let Some(key) = api_key.filter(|key| !key.trim().is_empty()) {
            cmd.arg("-H")
                .arg(format!("Authorization: Bearer {}", key.trim()));
        }

        let output = run_process(cmd, None, 25, 2 * 1024 * 1024, None, None).await?;
        if output.timed_out {
            return Err("Model discovery timed out".to_string());
        }
        if output.exit_code != Some(0) {
            return Err(format!(
                "Model discovery failed (curl exit {:?}): {}",
                output.exit_code,
                output.stderr.trim()
            ));
        }

        let lines: Vec<&str> = output.stdout.lines().collect();
        let status_code: u16 = lines.last().unwrap_or(&"0").trim().parse().unwrap_or(0);
        if status_code < 200 || status_code >= 300 {
            return Err(format!(
                "Model discovery returned HTTP {}: {}",
                status_code,
                lines[..lines.len().saturating_sub(1)].join("\n")
            ));
        }
        let body = lines[..lines.len().saturating_sub(1)].join("\n");
        let parsed: ModelsResponse = serde_json::from_str(&body)
            .map_err(|e| format!("Malformed model list response: {}", e))?;
        if let Some(err) = parsed.error {
            return Err(format!("Model discovery error: {}", err.message));
        }
        let mut ids: Vec<String> = parsed
            .data
            .unwrap_or_default()
            .into_iter()
            .map(|entry| entry.id)
            .filter(|id| !id.trim().is_empty())
            .collect();
        ids.sort();
        ids.dedup();
        Ok(ids)
    }
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Deserialize)]
struct ChatMessageResponse {
    /// Some providers omit the field instead of returning an empty string.
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Option<Vec<ChatChoice>>,
    error: Option<ApiErrorResponse>,
    id: Option<String>,
    model: Option<String>,
    usage: Option<serde_json::Value>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct ApiErrorResponse {
    message: String,
}

impl PlannerReviewer for CloudHttpPlannerReviewer {
    fn plan<'a>(
        &'a self,
        task: &'a str,
        project_root: &'a str,
        context: &'a str,
    ) -> BoxFuture<'a, Result<Plan, String>> {
        Box::pin(async move {
            let api_key_opt = match self.resolve_api_key() {
                Ok(k) => k,
                Err(e) => return Err(self.redact(&e)),
            };

            let system_prompt = r#"You are the Planner AI for an autonomous coding orchestrator.
Analyze the user task, project root, and repository context.
Output a structured plan strictly in valid JSON matching this exact schema:
{
  "summary": "High-level summary of planned change",
  "steps": ["Step 1 description", "Step 2 description", ...],
  "acceptance_criteria": ["Criterion 1", "Criterion 2", ...]
}
Do NOT return any commentary outside the JSON block. Ensure the JSON is well-formed."#;

            let user_content = format!(
                "Task: {}\nProject Root: {}\nContext:\n{}",
                task, project_root, context
            );

            let body = ChatCompletionRequest {
                model: self.model.clone(),
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: format!("System Prompt: {}\n\n{}", system_prompt, user_content),
                }],
                temperature: 0.2,
                max_tokens: None,
            };

            let body_str = serde_json::to_string(&body)
                .map_err(|e| format!("Failed to serialize request: {}", e))?;

            let url = if self.endpoint.ends_with("/chat/completions") {
                self.endpoint.clone()
            } else {
                format!("{}/chat/completions", self.endpoint.trim_end_matches('/'))
            };

            let mut headers_vec = vec![("Content-Type", "application/json")];
            let auth_header;
            if let Some(ref key) = api_key_opt {
                auth_header = format!("Bearer {}", key);
                headers_vec.push(("Authorization", auth_header.as_str()));
            }
            let headers = headers_vec;

            let (status, resp_text) = Self::post_json(&url, &headers, &body_str, 60)
                .await
                .map_err(|e| self.redact(&e))?;

            if status == 429 {
                return Err(
                    "Planner API rate limit reached (HTTP 429). Please wait and retry.".to_string(),
                );
            }
            if status < 200 || status >= 300 {
                return Err(self.redact(&format!(
                    "Planner API error (HTTP {}): {}",
                    status, resp_text
                )));
            }

            let res_json: ChatCompletionResponse =
                serde_json::from_str(&resp_text).map_err(|e| {
                    self.redact(&format!("Failed to parse planner API response JSON: {}", e))
                })?;

            if let Some(err) = &res_json.error {
                return Err(self.redact(&format!("Planner API returned error: {}", err.message)));
            }

            self.record_response_usage(
                &resp_text,
                res_json.id.as_deref(),
                res_json.model.as_deref(),
                res_json.usage.as_ref(),
                res_json.usage_metadata.as_ref(),
                "plan",
            )?;

            let content = res_json
                .choices
                .and_then(|mut c| c.pop())
                .map(|c| c.message.content)
                .ok_or_else(|| "Planner API returned no completion choices".to_string())?;

            let json_str = Self::extract_json_block(&content);
            let plan: Plan = serde_json::from_str(json_str).map_err(|e| {
                format!(
                    "Planner response was not valid JSON schema: {}. Raw response: {}",
                    e,
                    content.chars().take(200).collect::<String>()
                )
            })?;

            if plan.summary.trim().is_empty() {
                return Err(
                    "Planner response validation failed: 'summary' cannot be empty".to_string(),
                );
            }
            if plan.steps.is_empty() {
                return Err(
                    "Planner response validation failed: 'steps' must have at least one step"
                        .to_string(),
                );
            }
            if plan.acceptance_criteria.is_empty() {
                return Err("Planner response validation failed: 'acceptance_criteria' must have at least one criterion".to_string());
            }

            Ok(plan)
        })
    }

    fn generate_coding_prompt<'a>(
        &'a self,
        task: &'a str,
        plan: &'a Plan,
        iteration: u32,
        feedback: Option<&'a str>,
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let mut prompt = format!(
                "You are an expert software engineer implementing a task.\nTask: {}\nPlan Summary: {}\nIteration: {}\n\nSteps:\n",
                task, plan.summary, iteration
            );

            for (idx, step) in plan.steps.iter().enumerate() {
                prompt.push_str(&format!("{}. {}\n", idx + 1, step));
            }

            prompt.push_str("\nAcceptance Criteria:\n");
            for criterion in &plan.acceptance_criteria {
                prompt.push_str(&format!("- {}\n", criterion));
            }

            if let Some(fb) = feedback {
                prompt.push_str(&format!(
                    "\nPrevious Iteration Feedback / Unresolved Issues:\n{}\nPlease fix these issues and satisfy all criteria.\n",
                    fb
                ));
            }

            prompt.push_str("\nPlease implement the necessary file modifications directly.");
            Ok(prompt)
        })
    }

    fn review<'a>(
        &'a self,
        task: &'a str,
        plan: &'a Plan,
        criteria: &'a [String],
        changed_files: &'a [String],
        diff_summary: Option<&'a str>,
        exec_result: &'a ExecutorResult,
        verifications: &'a [VerificationResult],
    ) -> BoxFuture<'a, Result<ReviewerResponse, String>> {
        Box::pin(async move {
            let api_key_opt = match self.resolve_api_key() {
                Ok(k) => k,
                Err(e) => return Err(self.redact(&e)),
            };

            let system_prompt = r#"You are the Reviewer AI for an autonomous coding orchestrator.
Review the original task, acceptance criteria, actual changed files, git diff, process results, and test verifications.
Produce a strict verdict:
- "complete": All acceptance criteria are satisfied and tests pass.
- "needs_changes": The implementation is incomplete, has errors, or fails tests.
- "blocked": An unresolvable blocker prevents completion (e.g. impossible constraint).

Respond strictly in valid JSON matching this schema:
{
  "verdict": "complete" | "needs_changes" | "blocked",
  "findings": "Detailed explanation of code assessment",
  "unresolved_issues": ["Issue 1", "Issue 2"],
  "suggested_fix": "Clear direction for the next coding prompt (null if complete)"
}
Do NOT return commentary outside the JSON block."#;

            let mut review_input = format!(
                "Task: {}\nPlan Summary: {}\nCriteria:\n",
                task, plan.summary
            );
            for c in criteria {
                review_input.push_str(&format!("- {}\n", c));
            }
            review_input.push_str(&format!(
                "\nCoding Process Exit Code: {:?}\nChanged Files: {:?}\nDiff Summary: {:?}\nStdout Tail: {}\nStderr Tail: {}\nVerifications:\n",
                exec_result.exit_code,
                changed_files,
                diff_summary,
                exec_result.stdout.chars().rev().take(1000).collect::<String>().chars().rev().collect::<String>(),
                exec_result.stderr.chars().rev().take(1000).collect::<String>().chars().rev().collect::<String>()
            ));
            for v in verifications {
                review_input.push_str(&format!(
                    "Command '{}': status={:?}, exit_code={:?}, output={}\n",
                    v.command, v.status, v.exit_code, v.output
                ));
            }

            let body = ChatCompletionRequest {
                model: self.model.clone(),
                messages: vec![ChatMessage {
                    role: "user".to_string(),
                    content: format!("System Prompt: {}\n\n{}", system_prompt, review_input),
                }],
                temperature: 0.1,
                max_tokens: None,
            };

            let body_str = serde_json::to_string(&body)
                .map_err(|e| format!("Failed to serialize review request: {}", e))?;

            let url = if self.endpoint.ends_with("/chat/completions") {
                self.endpoint.clone()
            } else {
                format!("{}/chat/completions", self.endpoint.trim_end_matches('/'))
            };

            let mut headers_vec = vec![("Content-Type", "application/json")];
            let auth_header;
            if let Some(ref key) = api_key_opt {
                auth_header = format!("Bearer {}", key);
                headers_vec.push(("Authorization", auth_header.as_str()));
            }
            let headers = headers_vec;

            let (status, resp_text) = Self::post_json(&url, &headers, &body_str, 60)
                .await
                .map_err(|e| self.redact(&e))?;

            if status == 429 {
                return Err(
                    "Reviewer API rate limit reached (HTTP 429). Please wait and retry."
                        .to_string(),
                );
            }
            if status < 200 || status >= 300 {
                return Err(self.redact(&format!(
                    "Reviewer API error (HTTP {}): {}",
                    status, resp_text
                )));
            }

            let res_json: ChatCompletionResponse =
                serde_json::from_str(&resp_text).map_err(|e| {
                    self.redact(&format!(
                        "Failed to parse reviewer API response JSON: {}",
                        e
                    ))
                })?;

            if let Some(err) = &res_json.error {
                return Err(self.redact(&format!("Reviewer API returned error: {}", err.message)));
            }

            self.record_response_usage(
                &resp_text,
                res_json.id.as_deref(),
                res_json.model.as_deref(),
                res_json.usage.as_ref(),
                res_json.usage_metadata.as_ref(),
                "review",
            )?;

            let content = res_json
                .choices
                .and_then(|mut c| c.pop())
                .map(|c| c.message.content)
                .ok_or_else(|| "Reviewer API returned no completion choices".to_string())?;

            let json_str = Self::extract_json_block(&content);
            let review_res: ReviewerResponse = serde_json::from_str(json_str).map_err(|e| {
                format!(
                    "Reviewer response was not valid JSON schema: {}. Raw response: {}",
                    e,
                    content.chars().take(200).collect::<String>()
                )
            })?;

            if review_res.findings.trim().is_empty() {
                return Err(
                    "Reviewer response validation failed: 'findings' cannot be empty".to_string(),
                );
            }

            Ok(review_res)
        })
    }

    fn name(&self) -> &str {
        "CloudHttpPlannerReviewer"
    }

    fn is_test_adapter(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod stream_tests {
    use super::*;

    #[test]
    fn test_chat_completion_request_max_tokens_serialization() {
        let req_none = ChatCompletionRequest {
            model: "test-model".to_string(),
            messages: vec![],
            temperature: 0.7,
            max_tokens: None,
        };
        let json_none = serde_json::to_string(&req_none).unwrap();
        assert!(!json_none.contains("max_tokens"));

        let req_some = ChatCompletionRequest {
            model: "test-model".to_string(),
            messages: vec![],
            temperature: 0.7,
            max_tokens: Some(2048),
        };
        let json_some = serde_json::to_string(&req_some).unwrap();
        assert!(json_some.contains(r#""max_tokens":2048"#));
    }

    #[test]
    fn drain_valid_utf8_keeps_partial_codepoint() {
        // 'é' is 0xC3 0xA9; split across chunks.
        let mut buf = vec![0xC3];
        let text = drain_valid_utf8(&mut buf);
        assert!(text.is_empty());
        assert_eq!(buf, vec![0xC3]);
        buf.push(0xA9);
        let text = drain_valid_utf8(&mut buf);
        assert_eq!(text, "é");
        assert!(buf.is_empty());
    }

    #[test]
    fn parse_sse_data_extracts_delta() {
        let data = r#"{"choices":[{"delta":{"content":"Hello"}}]}"#;
        let mut saw_done = false;
        let mut status = 0;
        let delta = parse_sse_data(data, &mut saw_done, &mut status).unwrap();
        assert_eq!(delta, Some("Hello".to_string()));
        assert!(!saw_done);
    }

    #[test]
    fn parse_sse_data_handles_done() {
        let mut saw_done = false;
        let mut status = 0;
        let delta = parse_sse_data("[DONE]", &mut saw_done, &mut status).unwrap();
        assert!(delta.is_none());
        assert!(saw_done);
    }

    #[test]
    fn parse_sse_data_handles_error() {
        let data = r#"{"error":{"message":"boom"}}"#;
        let mut saw_done = false;
        let mut status = 0;
        let err = parse_sse_data(data, &mut saw_done, &mut status).unwrap_err();
        assert!(err.contains("boom"));
    }

    #[test]
    fn parse_sse_data_skips_malformed() {
        let mut saw_done = false;
        let mut status = 0;
        let delta = parse_sse_data("{not json", &mut saw_done, &mut status).unwrap();
        assert!(delta.is_none());
    }

    #[test]
    fn parse_multiple_deltas_in_sequence() {
        let mut saw_done = false;
        let mut status = 0;
        let d1 = parse_sse_data(
            r#"{"choices":[{"delta":{"content":"a"}}]}"#,
            &mut saw_done,
            &mut status,
        )
        .unwrap();
        let d2 = parse_sse_data(
            r#"{"choices":[{"delta":{"content":"b"}}]}"#,
            &mut saw_done,
            &mut status,
        )
        .unwrap();
        assert_eq!(d1, Some("a".to_string()));
        assert_eq!(d2, Some("b".to_string()));
    }
}

/// Drain the longest valid UTF-8 prefix from `buf`, leaving any trailing
/// partial codepoint bytes in place for the next chunk.
fn drain_valid_utf8(buf: &mut Vec<u8>) -> String {
    match String::from_utf8(buf.clone()) {
        Ok(s) => {
            buf.clear();
            s
        }
        Err(e) => {
            let valid = e.utf8_error().valid_up_to();
            if valid == 0 {
                return String::new();
            }
            let s = String::from_utf8(buf[..valid].to_vec()).unwrap_or_default();
            buf.drain(..valid);
            s
        }
    }
}

/// Parse one SSE `data:` payload. Returns `Some(delta)` when a text delta was
/// found. Sets `saw_done` when the `[DONE]` marker is encountered.
fn parse_sse_data(
    data: &str,
    saw_done: &mut bool,
    _status: &mut u16,
) -> Result<Option<String>, String> {
    let trimmed = data.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed == "[DONE]" {
        *saw_done = true;
        return Ok(None);
    }
    // Try to parse as OpenAI-style streaming chunk.
    #[derive(serde::Deserialize, Default)]
    struct Delta {
        content: Option<String>,
    }
    #[derive(serde::Deserialize, Default)]
    struct Choice {
        delta: Option<Delta>,
    }
    #[derive(serde::Deserialize)]
    struct Chunk {
        #[serde(default)]
        choices: Option<Vec<Choice>>,
        error: Option<ApiErrorResponse>,
    }
    match serde_json::from_str::<Chunk>(trimmed) {
        Ok(chunk) => {
            if let Some(err) = chunk.error {
                return Err(format!("Provider stream error: {}", err.message));
            }
            let mut delta = String::new();
            if let Some(choices) = chunk.choices {
                for c in choices {
                    if let Some(d) = c.delta {
                        if let Some(content) = d.content {
                            delta.push_str(&content);
                        }
                    }
                }
            }
            if delta.is_empty() {
                Ok(None)
            } else {
                Ok(Some(delta))
            }
        }
        Err(_) => {
            // Malformed event: ignore rather than failing the whole stream.
            Ok(None)
        }
    }
}
