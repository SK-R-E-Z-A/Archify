use std::time::Duration;
use tokio::process::Command as TokioCommand;

/// Result of a connection test.
#[derive(serde::Serialize, Clone, Debug)]
pub struct ConnectionTestResult {
    pub ok: bool,
    pub message: String,
}

/// Test a Cloud HTTP provider endpoint with a lightweight /models request.
/// Returns Ok(message) on success, Err(diagnostic) on failure. Never includes secrets.
pub async fn test_provider_connection(
    endpoint: String,
    model: String,
    api_key: Option<String>,
) -> Result<String, String> {
    let endpoint = endpoint.trim().to_string();
    if endpoint.is_empty() {
        return Err("Endpoint URL is required.".to_string());
    }

    // Lightweight validation: reuse model discovery (GET /models), which is
    // cheap and validates both reachability and authentication.
    match crate::providers::cloud_http::CloudHttpPlannerReviewer::list_models(
        &endpoint,
        api_key.as_deref(),
    )
    .await
    {
        Ok(models) => {
            if models.is_empty() {
                Ok(format!(
                    "Connected to {} (no models listed). Model '{}' will be used as configured.",
                    endpoint, model
                ))
            } else if models.iter().any(|m| m == &model) {
                Ok(format!(
                    "Connected successfully. Model '{}' is available ({} model(s) listed).",
                    model,
                    models.len()
                ))
            } else {
                Ok(format!(
                    "Connected, but model '{}' was not found in the server's model list ({} available). \
                     Check the model name.",
                    model,
                    models.len()
                ))
            }
        }
        Err(e) => Err(e),
    }
}

/// Test a coding agent executable: resolve it, launch with --version (and
/// fallbacks), bounded timeout, no real task execution.
pub async fn test_cli_agent(executable: String) -> Result<String, String> {
    let path = match crate::providers::resolve_executable(&executable) {
        Some(p) => p,
        None => {
            return Err(format!(
                "Executable '{}' not found on PATH or as a file path.",
                executable
            ));
        }
    };

    let candidates: [&str; 3] = ["--version", "-version", "version"];
    let mut last_err = String::new();
    for flag in candidates {
        let mut cmd = TokioCommand::new(&path);
        cmd.arg(flag);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let result = tokio::time::timeout(Duration::from_secs(10), cmd.output()).await;
        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let text = if stdout.is_empty() { stderr } else { stdout };
                if output.status.success() && !text.is_empty() {
                    let first_line = text.lines().next().unwrap_or(&text).trim();
                    return Ok(format!("{} — {}", path.display(), first_line));
                }
                if !text.is_empty() {
                    last_err = format!("Exited with status {}: {}", output.status, text);
                } else {
                    last_err = format!("Exited with status {} (no output).", output.status);
                }
            }
            Ok(Err(e)) => {
                last_err = format!("Failed to launch '{}': {}", path.display(), e);
            }
            Err(_) => {
                // kill the process tree best-effort
                last_err = format!(
                    "Timed out after 10 seconds running '{} {}'.",
                    path.display(),
                    flag
                );
            }
        }
    }

    if last_err.is_empty() {
        Err(format!(
            "Executable '{}' could not be queried for a version.",
            executable
        ))
    } else {
        Err(last_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_endpoint_fails() {
        let res = test_provider_connection("".to_string(), "m".to_string(), None).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("required"));
    }

    #[tokio::test]
    async fn missing_cli_agent_fails() {
        let res = test_cli_agent("definitely-not-a-real-executable-xyz".to_string()).await;
        assert!(res.is_err());
        let msg = res.unwrap_err();
        assert!(msg.contains("not found"));
        assert!(!msg.contains("secret"));
    }

    #[tokio::test]
    async fn installed_cli_agent_reports_version() {
        let res = test_cli_agent("git".to_string()).await;
        assert!(res.is_ok(), "git should be testable: {:?}", res);
        let msg = res.unwrap();
        assert!(msg.contains("git version") || msg.contains("version"));
    }

    #[tokio::test]
    async fn provider_connection_does_not_leak_secret() {
        // Invalid endpoint; ensure no secret appears in the error.
        let res = test_provider_connection(
            "http://127.0.0.1:1/".to_string(),
            "m".to_string(),
            Some("super-secret-token-12345678".to_string()),
        )
        .await;
        // Either ok (unlikely) or err; in both cases no secret in message.
        let msg = match res {
            Ok(m) => m,
            Err(e) => e,
        };
        assert!(!msg.contains("super-secret-token-12345678"));
    }
}
