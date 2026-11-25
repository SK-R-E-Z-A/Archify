use crate::models::{
    ChangedFiles, ReviewerResponse, ReviewerVerdict, VerificationResult, VerificationStatus,
};
use crate::providers::process_cli::run_process;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, UNIX_EPOCH};
use tokio::process::Command;
use tokio::time::timeout;

/// Verification commands (test suites, type checks, builds) can legitimately
/// take several minutes in a large project, so this is a hang guard rather than
/// a normal working limit.
const VERIFICATION_TIMEOUT_SECS: u64 = 5 * 60;

/// Keep verification output bounded even if a command floods stdout.
const VERIFICATION_MAX_OUTPUT_BYTES: usize = 1024 * 1024;

/// Evidence status for an individual acceptance criterion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceStatus {
    Passed,
    Failed,
    Missing,
}

/// Reviewer evidence associated with one planned acceptance criterion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriterionEvidence {
    pub criterion: String,
    pub status: EvidenceStatus,
    pub evidence: Option<String>,
}

/// Combined reviewer and command evidence used to gate task completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionAssessment {
    pub criteria: Vec<CriterionEvidence>,
    pub verification_status: EvidenceStatus,
    pub missing_evidence: Vec<String>,
    pub failure_evidence: Vec<String>,
}

impl CompletionAssessment {
    pub fn passed(&self) -> bool {
        !self.criteria.is_empty()
            && self
                .criteria
                .iter()
                .all(|criterion| criterion.status == EvidenceStatus::Passed)
            && self.verification_status == EvidenceStatus::Passed
            && self.missing_evidence.is_empty()
            && self.failure_evidence.is_empty()
    }
}

/// Evaluates every acceptance criterion against the review verdict and checks
/// that at least one configured verification command produced valid passing
/// evidence. Missing or contradictory evidence never passes.
pub fn assess_completion(
    acceptance_criteria: &[String],
    reviewer: Option<&ReviewerResponse>,
    verification_results: &[VerificationResult],
    verification_configured: bool,
) -> CompletionAssessment {
    let mut missing_evidence = Vec::new();
    let mut failure_evidence = Vec::new();

    let criteria = if acceptance_criteria.is_empty() {
        missing_evidence.push("No acceptance criteria were provided.".to_string());
        Vec::new()
    } else {
        acceptance_criteria
            .iter()
            .enumerate()
            .map(|(index, criterion)| {
                if criterion.trim().is_empty() {
                    missing_evidence.push(format!("Acceptance criterion {} is empty.", index + 1));
                    return CriterionEvidence {
                        criterion: criterion.clone(),
                        status: EvidenceStatus::Missing,
                        evidence: None,
                    };
                }

                match reviewer {
                    Some(response) if response.findings.trim().is_empty() => {
                        missing_evidence.push(format!(
                            "Reviewer provided no evidence for criterion {}.",
                            index + 1
                        ));
                        CriterionEvidence {
                            criterion: criterion.clone(),
                            status: EvidenceStatus::Missing,
                            evidence: None,
                        }
                    }
                    Some(response) if response.verdict == ReviewerVerdict::Complete => {
                        CriterionEvidence {
                            criterion: criterion.clone(),
                            status: EvidenceStatus::Passed,
                            evidence: Some(response.findings.clone()),
                        }
                    }
                    Some(response) => {
                        let evidence = if response.unresolved_issues.is_empty() {
                            response.findings.clone()
                        } else {
                            format!(
                                "{} Unresolved issues: {}",
                                response.findings,
                                response.unresolved_issues.join("; ")
                            )
                        };
                        failure_evidence.push(format!(
                            "Criterion '{}': {}",
                            criterion.trim(),
                            evidence
                        ));
                        CriterionEvidence {
                            criterion: criterion.clone(),
                            status: EvidenceStatus::Failed,
                            evidence: Some(evidence),
                        }
                    }
                    None => {
                        missing_evidence.push(format!(
                            "No reviewer verdict was provided for criterion {}.",
                            index + 1
                        ));
                        CriterionEvidence {
                            criterion: criterion.clone(),
                            status: EvidenceStatus::Missing,
                            evidence: None,
                        }
                    }
                }
            })
            .collect()
    };

    let mut has_passing_verification = false;
    for result in verification_results {
        match &result.status {
            VerificationStatus::Passed
                if !result.command.trim().is_empty() && result.exit_code == Some(0) =>
            {
                has_passing_verification = true;
            }
            VerificationStatus::Passed => {
                missing_evidence.push(format!(
                    "Malformed passing verification result for '{}': a command and exit code 0 are required.",
                    result.command
                ));
            }
            VerificationStatus::Failed => {
                failure_evidence.push(format!(
                    "Verification command '{}' failed (exit code {:?}): {}",
                    result.command, result.exit_code, result.output
                ));
            }
            VerificationStatus::Skipped | VerificationStatus::Unavailable => {
                missing_evidence.push(format!(
                    "Verification command '{}' did not provide passing evidence: {}",
                    result.command, result.output
                ));
            }
        }
    }

    if verification_results.is_empty() {
        if verification_configured {
            missing_evidence.push(
                "Verification is configured but no verification command results were collected."
                    .to_string(),
            );
        }
        // Not configured and no results: verification is a no-op.
    } else if !has_passing_verification
        && !verification_results
            .iter()
            .any(|result| result.status == VerificationStatus::Failed)
    {
        missing_evidence.push("No verification command passed.".to_string());
    }

    let verification_status = if !verification_configured {
        // No verification command is configured: there is nothing to fail,
        // so verification does not block completion.
        EvidenceStatus::Passed
    } else if !failure_evidence.is_empty()
        && verification_results
            .iter()
            .any(|result| result.status == VerificationStatus::Failed)
    {
        EvidenceStatus::Failed
    } else if !missing_evidence.is_empty() || !has_passing_verification {
        EvidenceStatus::Missing
    } else {
        EvidenceStatus::Passed
    };

    CompletionAssessment {
        criteria,
        verification_status,
        missing_evidence,
        failure_evidence,
    }
}

// ---------------------------------------------------------------------------
// Structured verification run (Copilot parity)
// ---------------------------------------------------------------------------

/// Task-level verification state. A successful agent response is not evidence;
/// only completed command results can move a run to `Passed`.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationState {
    NotVerified,
    Running,
    Passed,
    Failed,
    Inconclusive,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VerificationCheckKind {
    Tests,
    Build,
    TypeCheck,
    Other,
}

impl VerificationCheckKind {
    pub fn classify(command: &str) -> Self {
        let command = command.to_ascii_lowercase();
        if ["test", "pytest", "vitest", "jest", "cargo test"]
            .iter()
            .any(|marker| command.contains(marker))
        {
            Self::Tests
        } else if [
            "typecheck",
            "type-check",
            "tsc",
            "mypy",
            "pyright",
            "cargo check",
        ]
        .iter()
        .any(|marker| command.contains(marker))
        {
            Self::TypeCheck
        } else if ["build", "webpack", "vite build", "cargo build", "make"]
            .iter()
            .any(|marker| command.contains(marker))
        {
            Self::Build
        } else {
            Self::Other
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionFailureKind {
    TestFailure,
    BuildFailure,
    TypeCheckFailure,
    CommandFailure,
    TimedOut,
    SpawnFailure,
    MissingEvidence,
    IncompleteEvidence,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ExecutionEvidence {
    pub command: String,
    pub check_kind: VerificationCheckKind,
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub execution_error: Option<String>,
    pub stdout: String,
    pub stderr: String,
    pub started_at: String,
    pub completed_at: String,
    pub duration_ms: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct StructuredFailure {
    pub kind: ExecutionFailureKind,
    pub command: Option<String>,
    pub exit_code: Option<i32>,
    pub message: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct VerificationEvent {
    pub schema_version: u32,
    pub event_id: String,
    pub task_id: String,
    pub verification_id: String,
    pub occurred_at: String,
    pub state: VerificationState,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct VerificationReport {
    pub schema_version: u32,
    pub task_id: String,
    pub verification_id: String,
    pub state: VerificationState,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub evidence: Vec<ExecutionEvidence>,
    pub failures: Vec<StructuredFailure>,
    pub events: Vec<VerificationEvent>,
}

/// Stateful task verification collector. It has no input for an agent's
/// success claim, so claims cannot substitute for process evidence.
pub struct VerificationRun {
    report: VerificationReport,
}

impl VerificationRun {
    pub fn new(task_id: impl Into<String>) -> Self {
        let task_id = task_id.into();
        let verification_id = uuid::Uuid::new_v4().to_string();
        let mut run = Self {
            report: VerificationReport {
                schema_version: 1,
                task_id,
                verification_id,
                state: VerificationState::NotVerified,
                started_at: None,
                completed_at: None,
                evidence: Vec::new(),
                failures: Vec::new(),
                events: Vec::new(),
            },
        };
        run.emit_state_event();
        run
    }

    pub fn state(&self) -> VerificationState {
        self.report.state
    }

    pub fn start(&mut self) -> Result<(), String> {
        if self.report.state != VerificationState::NotVerified {
            return Err(format!(
                "Cannot start verification from {:?}",
                self.report.state
            ));
        }
        self.report.started_at = Some(chrono::Utc::now().to_rfc3339());
        self.set_state(VerificationState::Running);
        Ok(())
    }

    pub fn record_evidence(&mut self, mut evidence: ExecutionEvidence) -> Result<(), String> {
        if self.report.state != VerificationState::Running {
            return Err(format!(
                "Cannot record verification evidence while {:?}",
                self.report.state
            ));
        }
        evidence.command = redact_known_secrets(&evidence.command);
        evidence.stdout = redact_known_secrets(&evidence.stdout);
        evidence.stderr = redact_known_secrets(&evidence.stderr);
        evidence.execution_error = evidence
            .execution_error
            .as_deref()
            .map(redact_known_secrets);
        self.report.evidence.push(evidence);
        Ok(())
    }

    pub fn finish(&mut self) -> Result<VerificationReport, String> {
        if self.report.state != VerificationState::Running {
            return Err(format!(
                "Cannot finish verification from {:?}",
                self.report.state
            ));
        }

        self.report.completed_at = Some(chrono::Utc::now().to_rfc3339());
        let mut has_incomplete_evidence = false;
        let mut has_definitive_failure = false;
        for evidence in &self.report.evidence {
            if evidence.timed_out {
                has_definitive_failure = true;
                self.report.failures.push(StructuredFailure {
                    kind: ExecutionFailureKind::TimedOut,
                    command: Some(evidence.command.clone()),
                    exit_code: evidence.exit_code,
                    message: "Verification command timed out".to_string(),
                });
            } else if let Some(error) = &evidence.execution_error {
                let unavailable = error.starts_with("Project root does not exist")
                    || error == "No verification command specified";
                let kind = if unavailable {
                    has_incomplete_evidence = true;
                    ExecutionFailureKind::IncompleteEvidence
                } else {
                    has_definitive_failure = true;
                    ExecutionFailureKind::SpawnFailure
                };
                self.report.failures.push(StructuredFailure {
                    kind,
                    command: Some(evidence.command.clone()),
                    exit_code: evidence.exit_code,
                    message: error.clone(),
                });
            } else if let Some(exit_code) = evidence.exit_code {
                if exit_code != 0 {
                    has_definitive_failure = true;
                    self.report.failures.push(StructuredFailure {
                        kind: failure_kind_for(evidence.check_kind),
                        command: Some(evidence.command.clone()),
                        exit_code: Some(exit_code),
                        message: format!(
                            "{:?} command exited unsuccessfully with code {}",
                            evidence.check_kind, exit_code
                        ),
                    });
                }
            } else {
                has_incomplete_evidence = true;
                self.report.failures.push(StructuredFailure {
                    kind: ExecutionFailureKind::IncompleteEvidence,
                    command: Some(evidence.command.clone()),
                    exit_code: None,
                    message: "Execution ended without an exit code or timeout result".to_string(),
                });
            }
        }

        let next_state = if self.report.evidence.is_empty() {
            self.report.failures.push(StructuredFailure {
                kind: ExecutionFailureKind::MissingEvidence,
                command: None,
                exit_code: None,
                message: "No verification command results were collected".to_string(),
            });
            VerificationState::Inconclusive
        } else if has_definitive_failure {
            VerificationState::Failed
        } else if has_incomplete_evidence || !self.report.failures.is_empty() {
            VerificationState::Inconclusive
        } else {
            VerificationState::Passed
        };
        self.set_state(next_state);
        Ok(self.report.clone())
    }

    pub fn report(&self) -> VerificationReport {
        self.report.clone()
    }

    fn set_state(&mut self, state: VerificationState) {
        self.report.state = state;
        self.emit_state_event();
    }

    fn emit_state_event(&mut self) {
        self.report.events.push(VerificationEvent {
            schema_version: 1,
            event_id: uuid::Uuid::new_v4().to_string(),
            task_id: self.report.task_id.clone(),
            verification_id: self.report.verification_id.clone(),
            occurred_at: chrono::Utc::now().to_rfc3339(),
            state: self.report.state,
        });
    }
}

fn failure_kind_for(kind: VerificationCheckKind) -> ExecutionFailureKind {
    match kind {
        VerificationCheckKind::Tests => ExecutionFailureKind::TestFailure,
        VerificationCheckKind::Build => ExecutionFailureKind::BuildFailure,
        VerificationCheckKind::TypeCheck => ExecutionFailureKind::TypeCheckFailure,
        VerificationCheckKind::Other => ExecutionFailureKind::CommandFailure,
    }
}

/// Persists immutable JSON verification reports under UUID-based filenames.
pub struct VerificationStore {
    directory: PathBuf,
}

impl VerificationStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    pub fn persist(&self, report: &VerificationReport) -> Result<PathBuf, String> {
        let report_id = uuid::Uuid::parse_str(&report.verification_id)
            .map_err(|_| "Invalid verification report ID".to_string())?;
        std::fs::create_dir_all(&self.directory).map_err(|error| {
            format!(
                "Failed to create verification directory {}: {}",
                self.directory.display(),
                error
            )
        })?;
        let path = self.directory.join(format!("{}.json", report_id));
        let temporary = self
            .directory
            .join(format!("{}.{}.tmp", report_id, uuid::Uuid::new_v4()));
        let json = serde_json::to_vec_pretty(report)
            .map_err(|error| format!("Failed to serialize verification report: {}", error))?;

        let write_result = (|| -> std::io::Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&json)?;
            file.sync_all()
        })();
        if let Err(error) = write_result {
            let _ = std::fs::remove_file(&temporary);
            return Err(format!(
                "Failed to write verification report {}: {}",
                temporary.display(),
                error
            ));
        }

        if let Err(error) = std::fs::hard_link(&temporary, &path) {
            let _ = std::fs::remove_file(&temporary);
            return Err(format!(
                "Failed to persist verification report {}: {}",
                path.display(),
                error
            ));
        }
        std::fs::remove_file(&temporary).map_err(|error| {
            format!(
                "Verification report was saved to {}, but temporary file cleanup failed: {}",
                path.display(),
                error
            )
        })?;
        Ok(path)
    }

    pub fn load(&self, verification_id: &str) -> Result<Option<VerificationReport>, String> {
        let parsed_id = uuid::Uuid::parse_str(verification_id)
            .map_err(|_| "Invalid verification report ID".to_string())?;
        let path = self.directory.join(format!("{}.json", parsed_id));
        let json = match std::fs::read(&path) {
            Ok(json) => json,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(format!(
                    "Failed to read verification report {}: {}",
                    path.display(),
                    error
                ));
            }
        };
        serde_json::from_slice(&json)
            .map(Some)
            .map_err(|error| format!("Invalid verification report {}: {}", path.display(), error))
    }
}

/// Executes configured checks and returns a report based only on process output.
pub async fn verify_commands(
    task_id: impl Into<String>,
    project_root: &str,
    commands: &[String],
) -> Result<VerificationReport, String> {
    let mut run = VerificationRun::new(task_id);
    run.start()?;
    for command in commands {
        run.record_evidence(execute_verification_command(command, project_root).await?)?;
    }
    run.finish()
}

pub async fn verify_and_persist(
    task_id: impl Into<String>,
    project_root: &str,
    commands: &[String],
    store: &VerificationStore,
) -> Result<VerificationReport, String> {
    let report = verify_commands(task_id, project_root, commands).await?;
    store.persist(&report)?;
    Ok(report)
}

async fn execute_verification_command(
    command: &str,
    project_root: &str,
) -> Result<ExecutionEvidence, String> {
    let started = std::time::Instant::now();
    let started_at = chrono::Utc::now().to_rfc3339();
    let check_kind = VerificationCheckKind::classify(command);
    let parts = parse_command_line(command);
    let unavailable_error = if !Path::new(project_root).is_dir() {
        Some(format!(
            "Project root does not exist or is not a directory: {}",
            project_root
        ))
    } else if parts.is_empty() {
        Some("No verification command specified".to_string())
    } else {
        None
    };

    if let Some(error) = unavailable_error {
        return Ok(ExecutionEvidence {
            command: command.to_string(),
            check_kind,
            exit_code: None,
            timed_out: false,
            execution_error: Some(error),
            stdout: String::new(),
            stderr: String::new(),
            started_at,
            completed_at: chrono::Utc::now().to_rfc3339(),
            duration_ms: started.elapsed().as_millis() as u64,
        });
    }

    let mut cmd = Command::new(&parts[0]);
    cmd.args(&parts[1..]).current_dir(project_root);
    match run_process(
        cmd,
        None,
        VERIFICATION_TIMEOUT_SECS,
        VERIFICATION_MAX_OUTPUT_BYTES,
        None,
        None,
    )
    .await
    {
        Ok(output) => Ok(ExecutionEvidence {
            command: command.to_string(),
            check_kind,
            exit_code: output.exit_code,
            timed_out: output.timed_out,
            execution_error: None,
            stdout: output.stdout,
            stderr: output.stderr,
            started_at,
            completed_at: chrono::Utc::now().to_rfc3339(),
            duration_ms: output.duration_ms,
        }),
        Err(error) => Ok(ExecutionEvidence {
            command: command.to_string(),
            check_kind,
            exit_code: None,
            timed_out: false,
            execution_error: Some(error),
            stdout: String::new(),
            stderr: String::new(),
            started_at,
            completed_at: chrono::Utc::now().to_rfc3339(),
            duration_ms: started.elapsed().as_millis() as u64,
        }),
    }
}

fn redact_known_secrets(text: &str) -> String {
    let mut redacted = text.to_string();
    for name in crate::constants::SECRET_ENV_VARS {
        if let Ok(secret) = std::env::var(name) {
            if !secret.is_empty() {
                redacted = redacted.replace(&secret, "[REDACTED]");
            }
        }
    }
    redacted
}

/// Splits a verification command into program and arguments.
///
/// Supports single and double quotes so arguments and executable paths that
/// contain spaces can be expressed without invoking a shell. No shell
/// metacharacters, expansion, or escaping are interpreted: the first token is
/// still launched directly and the rest are passed verbatim as arguments.
/// Unterminated quotes consume the remainder of the line.
pub(crate) fn parse_command_line(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_token = false;
    let mut quote: Option<char> = None;

    for ch in command.chars() {
        match quote {
            Some(delimiter) => {
                if ch == delimiter {
                    quote = None;
                } else {
                    current.push(ch);
                }
            }
            None => {
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                    in_token = true;
                } else if ch.is_whitespace() {
                    if in_token {
                        tokens.push(std::mem::take(&mut current));
                        in_token = false;
                    }
                } else {
                    current.push(ch);
                    in_token = true;
                }
            }
        }
    }

    if in_token {
        tokens.push(current);
    }

    tokens
}

pub async fn run_verification_command(command_str: &str, project_root: &str) -> VerificationResult {
    run_verification_command_with_timeout(command_str, project_root, VERIFICATION_TIMEOUT_SECS)
        .await
}

async fn run_verification_command_with_timeout(
    command_str: &str,
    project_root: &str,
    timeout_secs: u64,
) -> VerificationResult {
    let root_path = Path::new(project_root);
    if !root_path.exists() || !root_path.is_dir() {
        return VerificationResult {
            command: command_str.to_string(),
            status: VerificationStatus::Unavailable,
            exit_code: None,
            output: format!(
                "Project root does not exist or is not a directory: {}",
                project_root
            ),
        };
    }

    let parts = parse_command_line(command_str);
    if parts.is_empty() {
        return VerificationResult {
            command: command_str.to_string(),
            status: VerificationStatus::Skipped,
            exit_code: Some(0),
            output: "No verification command specified".to_string(),
        };
    }

    let program = &parts[0];
    let args = &parts[1..];

    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(root_path);

    let result = run_process(
        cmd,
        None,
        timeout_secs,
        VERIFICATION_MAX_OUTPUT_BYTES,
        None,
        None,
    )
    .await;

    match result {
        Ok(output) => {
            let stdout_str = redact_known_secrets(&output.stdout);
            let stderr_str = redact_known_secrets(&output.stderr);
            let combined = if stderr_str.trim().is_empty() {
                stdout_str
            } else {
                format!("{}\n{}", stdout_str, stderr_str)
            };

            if output.timed_out {
                return VerificationResult {
                    command: command_str.to_string(),
                    status: VerificationStatus::Failed,
                    exit_code: None,
                    output: format!(
                        "Verification command timed out after {} seconds and its process tree was terminated.\n{}",
                        timeout_secs, combined
                    ),
                };
            }

            let status = if output.exit_code == Some(0) {
                VerificationStatus::Passed
            } else {
                VerificationStatus::Failed
            };

            VerificationResult {
                command: command_str.to_string(),
                status,
                exit_code: output.exit_code,
                output: combined,
            }
        }
        Err(e) => {
            let error = redact_known_secrets(&e.to_string());
            VerificationResult {
                command: command_str.to_string(),
                status: VerificationStatus::Failed,
                exit_code: None,
                output: format!(
                    "Failed to execute verification command '{}': {}",
                    command_str, error
                ),
            }
        }
    }
}

/// A snapshot of a single file at a point in time.
///
/// Contains enough metadata to compare two snapshots and determine whether
/// a file was created, modified, or deleted. Content is fingerprinted using
/// a streaming hash so large files do not need to be read entirely into memory.
#[derive(Debug, Clone)]
pub struct FileSnapshot {
    pub mtime_secs: u64,
    pub size: u64,
    pub hash: Option<String>,
}

/// The result of capturing a project snapshot.
///
/// `entries` maps normalized relative paths to their `FileSnapshot`.
/// `stable` is false if any file changed while it was being inspected,
/// meaning the snapshot may be incomplete or inconsistent.
pub struct SnapshotResult {
    pub entries: HashMap<String, FileSnapshot>,
    pub stable: bool,
}

/// Streams a file's contents through a hasher and returns a content fingerprint.
///
/// Uses bounded 8 KiB reads so large files do not consume unbounded memory.
/// The file path is mixed into the hash so two different paths with identical
/// content still produce distinct fingerprints.
///
/// Returns `None` if the file cannot be opened or read.
fn hash_file(path: &Path) -> Option<String> {
    let mut f = std::fs::File::open(path).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut buffer = [0u8; 8192];
    loop {
        let n = f.read(&mut buffer).ok()?;
        if n == 0 {
            break;
        }
        hasher.write(&buffer[..n]);
    }
    path.hash(&mut hasher);
    Some(format!("{:x}", hasher.finish()))
}

/// Captures a snapshot of a single file, detecting concurrent modification.
///
/// Stats the file before and after reading; if the modification time changed
/// during the read, the file is considered unstable and `stable` is false.
fn capture_file_snapshot(
    path: &Path,
    rel_path: &str,
    snapshots: &mut HashMap<String, FileSnapshot>,
    stable: &mut bool,
) {
    let metadata_before = match path.symlink_metadata() {
        Ok(m) => m,
        Err(_) => return,
    };

    let mtime_before = metadata_before
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let size_before = metadata_before.len();

    let hash = hash_file(path);

    let metadata_after = match path.symlink_metadata() {
        Ok(m) => m,
        Err(_) => return,
    };

    let mtime_after = metadata_after
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    let size_after = metadata_after.len();

    // If mtime or size changed while we were reading, the file was modified
    // during collection; mark the snapshot as unstable.
    if mtime_before != mtime_after || size_before != size_after {
        *stable = false;
    }

    snapshots.insert(
        rel_path.to_string(),
        FileSnapshot {
            mtime_secs: mtime_after,
            size: size_after,
            hash,
        },
    );
}

fn capture_snapshot(
    root: &Path,
    dir: &Path,
    ignored_dirs: &HashSet<String>,
    snapshots: &mut HashMap<String, FileSnapshot>,
    stable: &mut bool,
) {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in read_dir.flatten() {
        let path = entry.path();

        // Use symlink_metadata so we can detect symlinks without following them.
        // Directory symlinks/junctions are not recursed into (conservative policy;
        // full workspace-boundary validation is deferred to Task #10).
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        if metadata.is_dir() {
            if metadata.file_type().is_symlink() {
                // Do not blindly recurse through directory symlinks or junctions.
                continue;
            }
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if ignored_dirs.contains(name_str.as_ref()) {
                continue;
            }
            capture_snapshot(root, &path, ignored_dirs, snapshots, stable);
        } else if metadata.is_file() || metadata.file_type().is_symlink() {
            let Ok(rel) = path.strip_prefix(root) else {
                continue;
            };
            let rel_path = rel.to_string_lossy().replace('\\', "/");

            capture_file_snapshot(&path, &rel_path, snapshots, stable);
        }
    }
}

pub async fn snapshot_project(project_root: &str, ignored_dirs: &[String]) -> SnapshotResult {
    let root = Path::new(project_root);
    if !root.exists() {
        return SnapshotResult {
            entries: HashMap::new(),
            stable: true,
        };
    }

    let ignored: HashSet<String> = ignored_dirs.iter().cloned().collect();
    let mut snapshots = HashMap::new();
    let mut stable = true;
    capture_snapshot(root, root, &ignored, &mut snapshots, &mut stable);

    SnapshotResult {
        entries: snapshots,
        stable,
    }
}

/// Diffs two project snapshots and returns categorized changes.
///
/// Comparison rules:
/// - Present in `after` but absent in `before` → **created**
/// - Present in `before` but absent in `after` → **deleted**
/// - Present in both with identical content hash → unchanged (not reported)
/// - Present in both with different content hash → **modified**
///
/// Metadata-only changes (e.g. mtime) without a content hash difference are
/// not reported as modifications.
pub fn diff_snapshots(
    before: &HashMap<String, FileSnapshot>,
    after: &HashMap<String, FileSnapshot>,
) -> ChangedFiles {
    let mut result = ChangedFiles::default();

    let before_keys: HashSet<&String> = before.keys().collect();
    let after_keys: HashSet<&String> = after.keys().collect();

    // Created: in after but not in before
    for key in after_keys.difference(&before_keys) {
        result.created.push(key.to_string());
    }

    // Deleted: in before but not in after
    for key in before_keys.difference(&after_keys) {
        result.deleted.push(key.to_string());
    }

    // Modified: in both but content hash differs.
    // When both hashes are None (unreadable files), fall back to mtime/size.
    for key in before_keys.intersection(&after_keys) {
        let b = before.get(*key).unwrap();
        let a = after.get(*key).unwrap();

        let modified = match (b.hash.as_deref(), a.hash.as_deref()) {
            (Some(bh), Some(ah)) => bh != ah,
            (None, None) => b.mtime_secs != a.mtime_secs || b.size != a.size,
            // One side unreadable, the other readable: treat as modified.
            _ => true,
        };

        if modified {
            result.modified.push(key.to_string());
        }
    }

    result
}

pub async fn collect_changed_files(
    project_root: &str,
    ignored_dirs: &[String],
) -> (ChangedFiles, Option<String>) {
    let root = Path::new(project_root);
    if !root.exists() {
        return (ChangedFiles::default(), None);
    }

    // Attempt git status --porcelain
    let mut git_status = Command::new("git");
    git_status
        .args(["status", "--porcelain", "-z"])
        .current_dir(root)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let status_output = timeout(Duration::from_secs(5), git_status.output()).await;
    if let Ok(Ok(output)) = status_output {
        if output.status.success() {
            let out_str = String::from_utf8_lossy(&output.stdout);
            let mut created = Vec::new();
            let mut modified = Vec::new();
            let mut deleted = Vec::new();

            let mut records = out_str.split('\0');
            while let Some(record) = records.next() {
                if record.len() < 4 {
                    continue;
                }
                let status = &record[..2];
                let normalized = record[3..].replace('\\', "/");
                let is_rename = status.contains('R') || status.contains('C');
                let rename_source = if is_rename {
                    records.next().map(|path| path.replace('\\', "/"))
                } else {
                    None
                };
                if normalized.is_empty() {
                    continue;
                }

                if is_rename {
                    if !is_in_ignored_git_dir(&normalized, ignored_dirs) {
                        created.push(normalized);
                    }
                    if let Some(original) = rename_source {
                        if !original.is_empty() && !is_in_ignored_git_dir(&original, ignored_dirs) {
                            deleted.push(original);
                        }
                    }
                } else if is_in_ignored_git_dir(&normalized, ignored_dirs) {
                    continue;
                } else if status.contains('D') {
                    deleted.push(normalized);
                } else if status.contains('A') || status == "??" {
                    created.push(normalized);
                } else {
                    modified.push(normalized);
                }
            }

            let mut git_diff = Command::new("git");
            git_diff.args(["diff", "HEAD", "--stat", "--"]);
            for ignored in ignored_dirs {
                let ignored = ignored.replace('\\', "/");
                git_diff.arg(format!(":(exclude,glob)**/{}/**", ignored));
            }
            git_diff
                .current_dir(root)
                .stdout(Stdio::piped())
                .stderr(Stdio::null());

            let diff_summary = if let Ok(Ok(diff_out)) =
                timeout(Duration::from_secs(5), git_diff.output()).await
            {
                if diff_out.status.success() {
                    let s = String::from_utf8_lossy(&diff_out.stdout).trim().to_string();
                    if s.is_empty() { None } else { Some(s) }
                } else {
                    None
                }
            } else {
                None
            };

            return (
                ChangedFiles {
                    created,
                    modified,
                    deleted,
                },
                diff_summary,
            );
        }
    }

    // Fallback if not a git repository: snapshot-based scan.
    // Without a pre-execution baseline, all currently-existing files are
    // treated as "created" relative to an empty before-snapshot. A mtime
    // window is applied to avoid treating an entire long-lived project as
    // changed. This is a heuristic; Task #9 will provide baseline-aware
    // change attribution.
    let snapshot = snapshot_project(project_root, ignored_dirs).await;

    const RECENT_MODIFIED_SECS: u64 = 300;
    let now = UNIX_EPOCH.elapsed().unwrap_or_default().as_secs();

    let empty_before: HashMap<String, FileSnapshot> = HashMap::new();
    let mut filtered = diff_snapshots(&empty_before, &snapshot.entries);

    // Apply mtime window to avoid flagging old, unchanged files.
    filtered.created.retain(|path| {
        snapshot.entries.get(path).map_or(false, |snap| {
            now.saturating_sub(snap.mtime_secs) < RECENT_MODIFIED_SECS
        })
    });

    let summary = if filtered.is_empty() {
        None
    } else {
        Some(format!("{} file(s) detected", filtered.all_paths().len()))
    };

    (filtered, summary)
}

fn is_in_ignored_git_dir(path: &str, ignored_dirs: &[String]) -> bool {
    path.rsplit_once('/')
        .map(|(parent, _)| {
            parent
                .split('/')
                .any(|segment| ignored_dirs.iter().any(|ignored| ignored == segment))
        })
        .unwrap_or(false)
}

const DEFAULT_IGNORED_DIRS: [&str; 14] = [
    "node_modules",
    ".git",
    "target",
    "dist",
    "build",
    "out",
    ".next",
    ".turbo",
    ".cache",
    "__pycache__",
    ".venv",
    "venv",
    ".idea",
    ".vscode",
];

pub fn default_ignored_dirs() -> Vec<String> {
    DEFAULT_IGNORED_DIRS.iter().map(|s| s.to_string()).collect()
}

pub fn is_excluded_dir(name: &str) -> bool {
    DEFAULT_IGNORED_DIRS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ReviewerVerdict, VerificationResult};
    use std::fs;
    use std::process::Command as StdCommand;

    fn temp_dir(label: &str) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("verification_{}_{}", label, uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn git(root: &Path, args: &[&str]) {
        let output = StdCommand::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .expect("git is required for this test");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn completion_assessment_requires_nonempty_criteria_review_and_passed_command() {
        let criteria = vec!["The feature works".to_string()];
        let reviewer = ReviewerResponse {
            verdict: ReviewerVerdict::Complete,
            findings: "Reviewed the feature against the criterion.".to_string(),
            unresolved_issues: Vec::new(),
            suggested_fix: None,
        };
        let verification = VerificationResult {
            command: "cargo test".to_string(),
            status: VerificationStatus::Passed,
            exit_code: Some(0),
            output: "test result: ok".to_string(),
        };

        assert!(
            assess_completion(&criteria, Some(&reviewer), &[verification.clone()], true).passed()
        );
        assert!(assess_completion(&criteria, Some(&reviewer), &[], false).passed());

        let mut malformed = verification;
        malformed.exit_code = None;
        let assessment = assess_completion(&criteria, Some(&reviewer), &[malformed], true);
        assert!(!assessment.passed());
        assert_eq!(assessment.verification_status, EvidenceStatus::Missing);
        assert!(
            assessment
                .missing_evidence
                .iter()
                .any(|item| item.contains("Malformed passing verification"))
        );
    }

    #[test]
    fn completion_assessment_marks_missing_or_failed_criteria_evidence() {
        let criteria = vec!["Criterion A".to_string(), "   ".to_string()];
        let reviewer = ReviewerResponse {
            verdict: ReviewerVerdict::NeedsChanges,
            findings: "Criterion A remains incomplete.".to_string(),
            unresolved_issues: vec!["Criterion A".to_string()],
            suggested_fix: None,
        };
        let assessment = assess_completion(&criteria, Some(&reviewer), &[], false);

        assert!(!assessment.passed());
        assert!(
            assessment
                .criteria
                .iter()
                .any(|criterion| { criterion.status == EvidenceStatus::Failed })
        );
        assert!(
            assessment
                .criteria
                .iter()
                .any(|criterion| { criterion.status == EvidenceStatus::Missing })
        );
        assert!(!assessment.missing_evidence.is_empty());
        assert!(!assessment.failure_evidence.is_empty());
    }

    #[tokio::test]
    async fn git_changes_keep_rename_paths_include_staged_changes_and_honor_ignored_dirs() {
        let root = temp_dir("git_changes");
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "test@example.com"]);
        git(&root, &["config", "user.name", "Test"]);
        fs::write(root.join("original.txt"), "baseline").unwrap();
        fs::write(root.join("staged.txt"), "baseline").unwrap();
        fs::write(root.join("move-into-ignored.txt"), "baseline").unwrap();
        fs::create_dir_all(root.join("generated")).unwrap();
        fs::write(root.join("generated/tracked.txt"), "baseline").unwrap();
        git(
            &root,
            &[
                "add",
                "original.txt",
                "staged.txt",
                "move-into-ignored.txt",
                "generated/tracked.txt",
            ],
        );
        git(&root, &["commit", "-q", "-m", "baseline"]);

        fs::rename(root.join("original.txt"), root.join("renamed.txt")).unwrap();
        fs::rename(
            root.join("move-into-ignored.txt"),
            root.join("generated/moved.txt"),
        )
        .unwrap();
        fs::write(root.join("staged.txt"), "staged change").unwrap();
        fs::write(root.join("generated/tracked.txt"), "ignored change").unwrap();
        git(&root, &["add", "--all"]);
        fs::write(root.join("generated/output.txt"), "ignored").unwrap();

        let ignored_dirs = vec!["generated".to_string()];
        let (changes, summary) = collect_changed_files(root.to_str().unwrap(), &ignored_dirs).await;

        assert_eq!(changes.created, vec!["renamed.txt"]);
        assert_eq!(
            changes.deleted,
            vec!["move-into-ignored.txt", "original.txt"]
        );
        assert_eq!(changes.modified, vec!["staged.txt"]);
        assert!(
            !changes
                .all_paths()
                .iter()
                .any(|path| path.starts_with("generated/"))
        );
        let summary = summary.expect("staged diff must produce a summary");
        assert!(summary.contains("renamed.txt"));
        assert!(summary.contains("staged.txt"));
        assert!(!summary.contains("generated"));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn failed_verification_is_not_reported_as_passed() {
        let root = temp_dir("failed_command");
        let command = if cfg!(windows) {
            "cmd /C exit 9"
        } else {
            "/bin/sh -c 'exit 9'"
        };

        let result =
            run_verification_command_with_timeout(command, root.to_str().unwrap(), 10).await;

        assert_eq!(result.status, VerificationStatus::Failed);
        assert_eq!(result.exit_code, Some(9));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn missing_project_root_is_unavailable_without_running_command() {
        let root = temp_dir("missing_root");
        let missing = root.join("does-not-exist");

        let result = run_verification_command("cmd /C exit 0", missing.to_str().unwrap()).await;

        assert_eq!(result.status, VerificationStatus::Unavailable);
        assert_eq!(result.exit_code, None);
        assert!(result.output.contains("does not exist"));
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn verification_timeout_is_reported_as_failed() {
        let root = temp_dir("timeout");
        let command = if cfg!(windows) {
            "cmd /C ping -n 6 127.0.0.1"
        } else {
            "/bin/sh -c 'sleep 5'"
        };

        let result =
            run_verification_command_with_timeout(command, root.to_str().unwrap(), 1).await;

        assert_eq!(result.status, VerificationStatus::Failed);
        assert_eq!(result.exit_code, None);
        assert!(result.output.contains("timed out"));
        let _ = fs::remove_dir_all(root);
    }

    fn evidence(command: &str, exit_code: Option<i32>) -> ExecutionEvidence {
        ExecutionEvidence {
            command: command.to_string(),
            check_kind: VerificationCheckKind::classify(command),
            exit_code,
            timed_out: false,
            execution_error: None,
            stdout: String::new(),
            stderr: String::new(),
            started_at: chrono::Utc::now().to_rfc3339(),
            completed_at: chrono::Utc::now().to_rfc3339(),
            duration_ms: 1,
        }
    }

    #[tokio::test]
    async fn task_report_uses_actual_failed_process_exit() {
        let root = temp_dir("structured_failed_process");
        let success_command = if cfg!(windows) {
            "cmd /C exit 0"
        } else {
            "/bin/sh -c 'exit 0'"
        };
        let success = verify_commands(
            "task-real-success",
            root.to_str().unwrap(),
            &[success_command.to_string()],
        )
        .await
        .unwrap();
        assert_eq!(success.state, VerificationState::Passed);
        assert_eq!(success.evidence[0].exit_code, Some(0));

        let command = if cfg!(windows) {
            "cmd /C exit 9"
        } else {
            "/bin/sh -c 'exit 9'"
        };
        let report = verify_commands(
            "task-real-failure",
            root.to_str().unwrap(),
            &[command.to_string()],
        )
        .await
        .unwrap();

        assert_eq!(report.state, VerificationState::Failed);
        assert_eq!(report.evidence[0].exit_code, Some(9));
        assert_eq!(
            report.failures[0].kind,
            ExecutionFailureKind::CommandFailure
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn empty_checks_and_missing_project_are_inconclusive() {
        let root = temp_dir("inconclusive");
        let empty = verify_commands("task-empty", root.to_str().unwrap(), &[])
            .await
            .unwrap();
        assert_eq!(empty.state, VerificationState::Inconclusive);

        let missing = root.join("missing");
        let unavailable = verify_commands(
            "task-missing-root",
            missing.to_str().unwrap(),
            &["cargo test".to_string()],
        )
        .await
        .unwrap();
        assert_eq!(unavailable.state, VerificationState::Inconclusive);
        assert_eq!(
            unavailable.failures[0].kind,
            ExecutionFailureKind::IncompleteEvidence
        );
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn missing_executable_is_a_structured_spawn_failure() {
        let root = temp_dir("spawn_failure");
        let report = verify_commands(
            "task-spawn-failure",
            root.to_str().unwrap(),
            &["__verification_command_that_does_not_exist__".to_string()],
        )
        .await
        .unwrap();

        assert_eq!(report.state, VerificationState::Failed);
        assert_eq!(report.failures[0].kind, ExecutionFailureKind::SpawnFailure);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn no_execution_evidence_cannot_be_reported_as_passed() {
        let mut run = VerificationRun::new("task-no-evidence");
        assert_eq!(run.state(), VerificationState::NotVerified);

        run.start().unwrap();
        assert_eq!(run.state(), VerificationState::Running);

        let report = run.finish().unwrap();
        assert_eq!(report.state, VerificationState::Inconclusive);
        assert_eq!(report.evidence.len(), 0);
        assert_eq!(
            report.failures[0].kind,
            ExecutionFailureKind::MissingEvidence
        );
    }

    #[test]
    fn failed_test_build_and_typecheck_commands_are_structured() {
        let cases = [
            ("cargo test --lib", ExecutionFailureKind::TestFailure),
            ("npm run build", ExecutionFailureKind::BuildFailure),
            ("npx tsc --noEmit", ExecutionFailureKind::TypeCheckFailure),
        ];

        for (command, expected_kind) in cases {
            let mut run = VerificationRun::new(format!("task-{}", command));
            run.start().unwrap();
            run.record_evidence(evidence(command, Some(1))).unwrap();

            let report = run.finish().unwrap();
            assert_eq!(report.state, VerificationState::Failed);
            assert_eq!(report.failures.len(), 1);
            assert_eq!(report.failures[0].kind, expected_kind);
            assert_eq!(report.failures[0].exit_code, Some(1));
        }
    }

    #[test]
    fn incomplete_execution_evidence_is_inconclusive() {
        let mut incomplete = evidence("cargo test", None);
        incomplete.execution_error = None;
        let mut run = VerificationRun::new("task-incomplete");
        run.start().unwrap();
        run.record_evidence(incomplete.clone()).unwrap();

        let report = run.finish().unwrap();
        assert_eq!(report.state, VerificationState::Inconclusive);
        assert_eq!(
            report.failures[0].kind,
            ExecutionFailureKind::IncompleteEvidence
        );

        let mut mixed = VerificationRun::new("task-failure-and-incomplete");
        mixed.start().unwrap();
        mixed
            .record_evidence(evidence("npm run build", Some(2)))
            .unwrap();
        mixed.record_evidence(incomplete).unwrap();
        assert_eq!(mixed.finish().unwrap().state, VerificationState::Failed);
    }

    #[test]
    fn timeout_evidence_is_a_definitive_failure() {
        let mut timed_out = evidence("npm test", None);
        timed_out.timed_out = true;
        let mut run = VerificationRun::new("task-timeout");
        run.start().unwrap();
        run.record_evidence(timed_out).unwrap();

        let report = run.finish().unwrap();
        assert_eq!(report.state, VerificationState::Failed);
        assert_eq!(report.failures[0].kind, ExecutionFailureKind::TimedOut);
    }

    #[test]
    fn only_successful_process_evidence_passes_and_emits_state_events() {
        let mut run = VerificationRun::new("task-success");
        run.start().unwrap();
        run.record_evidence(evidence("cargo test --lib", Some(0)))
            .unwrap();
        run.record_evidence(evidence("cargo check", Some(0)))
            .unwrap();

        let report = run.finish().unwrap();
        assert_eq!(report.state, VerificationState::Passed);
        assert_eq!(
            report
                .events
                .iter()
                .map(|event| event.state)
                .collect::<Vec<_>>(),
            vec![
                VerificationState::NotVerified,
                VerificationState::Running,
                VerificationState::Passed
            ]
        );
    }

    #[test]
    fn verification_reports_round_trip_through_store() {
        let root = temp_dir("report_store");
        let mut run = VerificationRun::new("task-persisted");
        run.start().unwrap();
        run.record_evidence(evidence("cargo test", Some(1)))
            .unwrap();
        let report = run.finish().unwrap();

        let store = VerificationStore::new(root.clone());
        store.persist(&report).unwrap();
        let loaded = store.load(&report.verification_id).unwrap().unwrap();

        assert_eq!(loaded, report);
        assert!(store.persist(&report).is_err());
        assert!(store.load("../outside").is_err());
        let _ = fs::remove_dir_all(root);
    }
}
