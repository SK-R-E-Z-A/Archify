pub mod cloud_http;
pub mod presets;
pub mod process_cli;
pub mod test_adapter;
#[cfg(windows)]
pub mod windows_job;

use crate::models::{ExecutorResult, Plan, ReviewerResponse, VerificationResult};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Return the lower-cased host from an `http`/`https` URL, handling bracketed
/// IPv6 literals and rejecting embedded credentials or malformed authorities.
pub fn endpoint_host(url: &str) -> Option<String> {
    let trimmed = url.trim();
    let after_scheme = trimmed
        .strip_prefix("http://")
        .or_else(|| trimmed.strip_prefix("https://"))?;
    let authority = after_scheme.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() || authority.contains('@') {
        return None;
    }

    let host = if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']')?;
        &rest[..end]
    } else {
        authority.split(':').next().unwrap_or("")
    };
    let host = host.trim();
    if host.is_empty() || host.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// Local model servers (Ollama, LM Studio, llama.cpp, ...) normally listen on
/// loopback and do not require an API key.
pub fn is_local_endpoint_url(url: &str) -> bool {
    matches!(
        endpoint_host(url).as_deref(),
        Some("localhost" | "127.0.0.1" | "::1")
    )
}

/// Resolve a configured CLI executable to an existing file.
///
/// Bare names are searched on `PATH` (honouring `PATHEXT` on Windows); names
/// containing a path separator are treated as filesystem paths.
pub fn resolve_executable(executable: &str) -> Option<PathBuf> {
    let name = executable.trim().trim_matches('"');
    if name.is_empty() {
        return None;
    }

    let as_path = Path::new(name);
    if as_path.is_absolute() || as_path.components().count() > 1 {
        return as_path.is_file().then(|| as_path.to_path_buf());
    }

    let path_var = std::env::var_os("PATH")?;
    let mut candidates = vec![name.to_string()];
    #[cfg(windows)]
    {
        let pathext =
            std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string());
        for ext in pathext.split(';').map(str::trim).filter(|e| !e.is_empty()) {
            candidates.push(format!("{name}{ext}"));
            candidates.push(format!("{name}{}", ext.to_ascii_lowercase()));
        }
    }

    for dir in std::env::split_paths(&path_var) {
        if dir.as_os_str().is_empty() {
            continue;
        }
        for candidate in &candidates {
            let full = dir.join(candidate);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

pub trait PlannerReviewer: Send + Sync {
    fn plan<'a>(
        &'a self,
        task: &'a str,
        project_root: &'a str,
        context: &'a str,
    ) -> BoxFuture<'a, Result<Plan, String>>;

    fn generate_coding_prompt<'a>(
        &'a self,
        task: &'a str,
        plan: &'a Plan,
        iteration: u32,
        feedback: Option<&'a str>,
    ) -> BoxFuture<'a, Result<String, String>>;

    fn review<'a>(
        &'a self,
        task: &'a str,
        plan: &'a Plan,
        criteria: &'a [String],
        changed_files: &'a [String],
        diff_summary: Option<&'a str>,
        exec_result: &'a ExecutorResult,
        verifications: &'a [VerificationResult],
    ) -> BoxFuture<'a, Result<ReviewerResponse, String>>;

    fn name(&self) -> &str;
    fn is_test_adapter(&self) -> bool;
}

pub trait CodingExecutor: Send + Sync {
    fn execute<'a>(
        &'a self,
        prompt: &'a str,
        project_root: &'a str,
        task_id: Option<&'a str>,
        cancel_rx: &'a mut tokio::sync::watch::Receiver<bool>,
    ) -> BoxFuture<'a, Result<ExecutorResult, String>>;

    fn name(&self) -> &str;
    fn is_test_adapter(&self) -> bool;
}
