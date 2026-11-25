//! Recovery policy module — pure, standalone decision-making for execution recovery.
//!
//! This module is intentionally decoupled from the orchestrator and all Tauri
//! infrastructure. It has no async, no I/O, and no Tauri dependencies.
//! It evaluates an [`ExecutionReport`] and recommends a [`PolicyDecision`].

use crate::models::{
    ExecutorResult, ReviewerResponse, ReviewerVerdict, VerificationResult, VerificationStatus,
};
use crate::verification::{EvidenceStatus, assess_completion};

// ---------------------------------------------------------------------------
// Public decision types
// ---------------------------------------------------------------------------

/// The class of failure that caused an iteration to not succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailureClass {
    /// Build/compile failure detected in output.
    CompilerError,
    /// Test suite failure detected.
    TestFailure,
    /// Process died unexpectedly (non-zero exit, no timeout).
    ExecutorCrash,
    /// Execution timed out.
    Timeout,
    /// Task description not addressed.
    IncompleteWork,
    /// Claims success but evidence is missing.
    FalseSuccess,
    /// Configuration is permanently invalid.
    InvalidConfig,
    /// The process completed with a runtime-level error.
    RuntimeError,
    /// The reviewer found that the requested work is blocked or incomplete.
    ReviewFailure,
    /// A required verification command failed.
    VerificationFailure,
    /// Required acceptance, review, or verification evidence is absent.
    MissingEvidence,
    /// The execution or verification report is internally inconsistent.
    MalformedReport,
}

impl FailureClass {
    /// Human-readable label used in corrective prompt hints.
    pub fn label(&self) -> &'static str {
        match self {
            FailureClass::CompilerError => "compiler/build error",
            FailureClass::TestFailure => "test failure",
            FailureClass::ExecutorCrash => "executor crash",
            FailureClass::Timeout => "execution timeout",
            FailureClass::IncompleteWork => "incomplete work",
            FailureClass::FalseSuccess => "false success (no changes detected)",
            FailureClass::InvalidConfig => "invalid configuration",
            FailureClass::RuntimeError => "runtime error",
            FailureClass::ReviewFailure => "review failure",
            FailureClass::VerificationFailure => "verification failure",
            FailureClass::MissingEvidence => "missing verification evidence",
            FailureClass::MalformedReport => "malformed execution report",
        }
    }
}

/// Reason for a [`PolicyDecision::Stop`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// Retry budget exhausted.
    RetryBudgetExhausted { attempts: u32, max: u32 },
    /// A permanent, unrecoverable failure.
    PermanentFailure { failure_class: FailureClass },
    /// Task was cancelled by the user.
    Cancelled,
    /// The configuration is invalid and cannot be retried.
    InvalidConfiguration { detail: String },
    /// The same unresolved issue recurred across iterations.
    RepeatedIdenticalFailure { issue: String },
}

/// The recommended action after evaluating an [`ExecutionReport`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    /// Execution succeeded and all acceptance criteria are met. Mark Complete.
    Complete,
    /// Execution succeeded structurally but criteria are not met.
    /// Queue another iteration with an improved prompt.
    Continue { corrective_prompt_hint: String },
    /// A specific failure was identified. The corrective prompt should target the root cause.
    Repair {
        failure_class: FailureClass,
        corrective_prompt_hint: String,
    },
    /// Verification passed but needs further evidence collection.
    Verify,
    /// Escalate to user: requires clarification or approval.
    AskUser { reason: String },
    /// Cannot continue: retry budget exhausted, permanent failure, or cancellation.
    Stop { reason: StopReason },
}

// ---------------------------------------------------------------------------
// Core input type
// ---------------------------------------------------------------------------

/// A self-contained snapshot of everything the policy needs to make a decision.
/// Built by the orchestrator; never read from disk by this module.
#[derive(Debug, Clone)]
pub struct ExecutionReport {
    /// The user-supplied task description.
    pub task_description: String,
    /// Acceptance criteria from the plan (may be empty).
    pub acceptance_criteria: Vec<String>,
    /// Current iteration number (1-based).
    pub iteration: u32,
    /// Maximum allowed iterations.
    pub max_iterations: u32,
    /// Result of the executor run, or `None` if the executor failed to launch.
    pub executor_result: Option<ExecutorResult>,
    /// Verification results (may be empty).
    pub verification_results: Vec<VerificationResult>,
    /// Reviewer response, if one was obtained.
    pub reviewer_response: Option<ReviewerResponse>,
    /// Unresolved issues reported by the reviewer in a *previous* iteration.
    pub previous_unresolved_issues: Vec<String>,
    /// Whether the task was cancelled.
    pub cancelled: bool,
    /// If set, the config is permanently invalid; value is a human-readable detail.
    pub invalid_config_detail: Option<String>,
    /// Number of files changed in this iteration.
    pub changed_files_count: usize,
    /// Whether verification is configured.
    pub verification_configured: bool,
}

// ---------------------------------------------------------------------------
// Pattern helpers (pure string matching, no regex dependency)
// ---------------------------------------------------------------------------

fn contains_compiler_error(text: &str) -> bool {
    // Rust: "error[E…", general: "Build failed", "compilation error",
    // TypeScript: "error TS", Python/JS: "SyntaxError"
    text.contains("error[E")
        || text.contains("Build failed")
        || text.contains("build failed")
        || text.contains("SyntaxError")
        || text.contains("compilation error")
        || text.contains("error TS")
        || text.contains("Compilation failed")
        || text.contains("compilation failed")
}

fn contains_test_failure(text: &str) -> bool {
    text.contains("test failed")
        || text.contains("tests failed")
        || text.contains("FAILED")
        || text.contains("assertion failed")
        || text.contains("AssertionError")
        || text.contains("test result: FAIL")
}

fn contains_runtime_error(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "panic",
        "exception",
        "runtime error",
        "segmentation fault",
        "unhandled exception",
        "fatal error",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

fn evidence_for_prompt(evidence: &[String]) -> String {
    const MAX_EVIDENCE_CHARS: usize = 3000;
    let joined = evidence.join("\n");
    if joined.chars().count() <= MAX_EVIDENCE_CHARS {
        joined
    } else {
        let mut bounded: String = joined.chars().take(MAX_EVIDENCE_CHARS).collect();
        bounded.push_str("\n[additional evidence omitted]");
        bounded
    }
}

fn retry_or_stop(
    report: &ExecutionReport,
    failure_class: FailureClass,
    corrective_prompt_hint: String,
) -> PolicyDecision {
    if report.iteration >= report.max_iterations {
        PolicyDecision::Stop {
            reason: StopReason::RetryBudgetExhausted {
                attempts: report.iteration,
                max: report.max_iterations,
            },
        }
    } else {
        PolicyDecision::Repair {
            failure_class,
            corrective_prompt_hint,
        }
    }
}

fn continue_or_stop(report: &ExecutionReport, hint: String) -> PolicyDecision {
    if report.iteration >= report.max_iterations {
        PolicyDecision::Stop {
            reason: StopReason::RetryBudgetExhausted {
                attempts: report.iteration,
                max: report.max_iterations,
            },
        }
    } else {
        PolicyDecision::Continue {
            corrective_prompt_hint: hint,
        }
    }
}

fn verify_or_stop(report: &ExecutionReport) -> PolicyDecision {
    if report.iteration >= report.max_iterations {
        PolicyDecision::Stop {
            reason: StopReason::RetryBudgetExhausted {
                attempts: report.iteration,
                max: report.max_iterations,
            },
        }
    } else {
        PolicyDecision::Verify
    }
}

// ---------------------------------------------------------------------------
// Core evaluation function
// ---------------------------------------------------------------------------

/// Evaluate an [`ExecutionReport`] and return the recommended [`PolicyDecision`].
///
/// Priority order is enforced by the sequential checks below.
pub fn evaluate(report: &ExecutionReport) -> PolicyDecision {
    if report.cancelled {
        return PolicyDecision::Stop {
            reason: StopReason::Cancelled,
        };
    }

    if let Some(ref detail) = report.invalid_config_detail {
        return PolicyDecision::Stop {
            reason: StopReason::InvalidConfiguration {
                detail: detail.clone(),
            },
        };
    }

    let Some(exec) = report.executor_result.as_ref() else {
        return retry_or_stop(
            report,
            FailureClass::InvalidConfig,
            format!(
                "The executor failed to launch (no result returned). \
                 This is a {}. Check the CLI configuration, executable path, \
                 and working directory before retrying.",
                FailureClass::InvalidConfig.label()
            ),
        );
    };

    if exec.timed_out {
        return retry_or_stop(
            report,
            FailureClass::Timeout,
            format!(
                "The previous execution timed out ({}). \
                 Simplify the task scope or break it into smaller steps \
                 to avoid a {}.",
                exec.duration_ms,
                FailureClass::Timeout.label()
            ),
        );
    }

    let combined = format!("{}\n{}", exec.stdout, exec.stderr);
    if contains_compiler_error(&combined) {
        return retry_or_stop(
            report,
            FailureClass::CompilerError,
            format!(
                "A {} was detected in the previous run. \
                 Review and fix the compilation errors reported in the output \
                 before continuing.\n\nEvidence:\n{}",
                FailureClass::CompilerError.label(),
                evidence_for_prompt(&[combined.clone()])
            ),
        );
    }

    if contains_test_failure(&combined) {
        return retry_or_stop(
            report,
            FailureClass::TestFailure,
            format!(
                "A {} was detected in the previous run. \
                 Examine the failing tests and fix the underlying logic or \
                 test expectations.\n\nEvidence:\n{}",
                FailureClass::TestFailure.label(),
                evidence_for_prompt(&[combined.clone()])
            ),
        );
    }

    let failed_verifications: Vec<&VerificationResult> = report
        .verification_results
        .iter()
        .filter(|v| v.status == VerificationStatus::Failed)
        .collect();

    if !failed_verifications.is_empty() {
        let mut details = Vec::new();
        for result in &failed_verifications {
            details.push(format!(
                "{} (exit {:?}): {}",
                result.command, result.exit_code, result.output
            ));
        }
        let evidence = evidence_for_prompt(&details);
        let class = if details.iter().any(|text| contains_compiler_error(text)) {
            FailureClass::CompilerError
        } else if details.iter().any(|text| contains_test_failure(text)) {
            FailureClass::TestFailure
        } else if details
            .iter()
            .any(|text| text.to_ascii_lowercase().contains("timed out"))
        {
            FailureClass::Timeout
        } else {
            FailureClass::VerificationFailure
        };
        return retry_or_stop(
            report,
            class.clone(),
            format!(
                "{} for command(s):\n{}\n\nFix the reported issue and preserve this evidence in the next review.",
                class.label(),
                evidence
            ),
        );
    }

    let exit_ok = exec.exit_code == Some(0);
    let reviewer_complete = matches!(
        report.reviewer_response.as_ref().map(|r| &r.verdict),
        Some(ReviewerVerdict::Complete)
    );
    if exit_ok
        && !report.acceptance_criteria.is_empty()
        && report.changed_files_count == 0
        && reviewer_complete
    {
        return retry_or_stop(
            report,
            FailureClass::FalseSuccess,
            format!(
                "The previous run claimed success (exit 0, reviewer Complete) \
                 but no files were changed despite non-empty acceptance criteria. \
                 This looks like a {}. Explicitly implement the required changes \
                 and verify the acceptance criteria are actually met.",
                FailureClass::FalseSuccess.label()
            ),
        );
    }

    if let Some(ref reviewer) = report.reviewer_response {
        if reviewer.verdict == ReviewerVerdict::Blocked {
            return PolicyDecision::Stop {
                reason: StopReason::PermanentFailure {
                    failure_class: FailureClass::ReviewFailure,
                },
            };
        }
    }

    if !report.previous_unresolved_issues.is_empty() {
        if let Some(ref reviewer) = report.reviewer_response {
            if !reviewer.unresolved_issues.is_empty()
                && reviewer.unresolved_issues == report.previous_unresolved_issues
            {
                let issue = reviewer.unresolved_issues.join("; ");
                return PolicyDecision::Stop {
                    reason: StopReason::RepeatedIdenticalFailure { issue },
                };
            }
        }
    }

    if let Some(exit_code) = exec.exit_code {
        if exit_code != 0 {
            let (failure_class, description) = if contains_runtime_error(&combined) {
                (FailureClass::RuntimeError, "runtime error")
            } else {
                (FailureClass::ExecutorCrash, "non-zero executor exit")
            };
            return retry_or_stop(
                report,
                failure_class.clone(),
                format!(
                    "The executor ended with {} (exit code {}). Review the captured process output and resolve the underlying failure.\n\nEvidence:\n{}",
                    description,
                    exit_code,
                    evidence_for_prompt(&[combined])
                ),
            );
        }
    } else {
        return retry_or_stop(
            report,
            FailureClass::ExecutorCrash,
            format!(
                "The executor returned no exit code and did not report a timeout. Treat this as a {} and inspect the process output before retrying.\n\nEvidence:\n{}",
                FailureClass::ExecutorCrash.label(),
                evidence_for_prompt(&[combined])
            ),
        );
    }

    if let Some(ref reviewer) = report.reviewer_response {
        if reviewer.verdict == ReviewerVerdict::NeedsChanges {
            let hint = match &reviewer.suggested_fix {
                Some(fix) if !fix.is_empty() => fix.clone(),
                _ => format!(
                    "The reviewer indicated changes are needed: {}. \
                     Address the findings and unresolved issues.",
                    reviewer.findings
                ),
            };
            return retry_or_stop(report, FailureClass::IncompleteWork, hint);
        }
    }

    if reviewer_complete {
        let assessment = assess_completion(
            &report.acceptance_criteria,
            report.reviewer_response.as_ref(),
            &report.verification_results,
            report.verification_configured,
        );
        if assessment.passed() {
            return PolicyDecision::Complete;
        }

        let mut evidence = assessment.missing_evidence;
        evidence.extend(assessment.failure_evidence);
        let class = if assessment.verification_status == EvidenceStatus::Failed {
            FailureClass::VerificationFailure
        } else if evidence.iter().any(|item| item.starts_with("Malformed")) {
            FailureClass::MalformedReport
        } else {
            FailureClass::MissingEvidence
        };
        return retry_or_stop(
            report,
            class.clone(),
            format!(
                "Verification is configured but no verification command results were collected. \
                 Collect the missing evidence, satisfy every acceptance criterion, and obtain a \
                 passing verification result before reporting completion. Evidence:\n{}",
                evidence_for_prompt(&evidence)
            ),
        );
    }

    if report.reviewer_response.is_none() {
        return verify_or_stop(report);
    }

    continue_or_stop(
        report,
        "Continue working on the task. Review the previous output and ensure all acceptance criteria are met.".to_string(),
    )
}

// ---------------------------------------------------------------------------
// Escalation helper
// ---------------------------------------------------------------------------

/// Returns `Some(reason)` if the task should be escalated to the user for
/// input before the orchestrator continues.
///
/// Escalation triggers:
/// - `invalid_config_detail` is set.
/// - Task description contains escalation keywords.
/// - Reviewer findings contain escalation keywords.
pub fn should_escalate_to_user(report: &ExecutionReport) -> Option<String> {
    // Always escalate on invalid config detail.
    if let Some(ref detail) = report.invalid_config_detail {
        return Some(format!(
            "Invalid configuration requires user attention: {detail}"
        ));
    }

    let escalation_keywords = ["CONFIRM:", "requires approval", "destructive"];

    // Check task description.
    for kw in &escalation_keywords {
        if report.task_description.contains(kw) {
            return Some(format!(
                "Task description contains escalation keyword '{kw}' and requires user confirmation."
            ));
        }
    }

    // Check reviewer findings.
    if let Some(ref reviewer) = report.reviewer_response {
        for kw in &escalation_keywords {
            if reviewer.findings.contains(kw) {
                return Some(format!(
                    "Reviewer findings contain '{kw}' and require user confirmation."
                ));
            }
        }
        for issue in &reviewer.unresolved_issues {
            for kw in &escalation_keywords {
                if issue.contains(kw) {
                    return Some(format!(
                        "Unresolved reviewer issue contains '{kw}' and requires user confirmation."
                    ));
                }
            }
        }
    }

    None
}

// ---------------------------------------------------------------------------
// Prompt improvement helper
// ---------------------------------------------------------------------------

/// Returns an improved corrective prompt that incorporates the failure class
/// and any evidence from previous failures.
pub fn improve_corrective_prompt(
    base_prompt: &str,
    failure_class: &FailureClass,
    previous_failures: &[String],
) -> String {
    let mut parts = Vec::new();

    // Always start with the base prompt.
    parts.push(base_prompt.to_string());

    // Append failure class context.
    parts.push(format!(
        "\n\n[Recovery context: failure class = {}]",
        failure_class.label()
    ));

    // Append previous failure evidence.
    if !previous_failures.is_empty() {
        parts.push("\n\nPrevious unresolved issues that must be addressed:".to_string());
        for (i, failure) in previous_failures.iter().enumerate() {
            parts.push(format!("  {}. {}", i + 1, failure));
        }
    }

    parts.join("")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        ExecutorResult, ReviewerResponse, ReviewerVerdict, VerificationResult, VerificationStatus,
    };

    // --- Helpers ---

    fn ok_executor() -> ExecutorResult {
        ExecutorResult {
            exit_code: Some(0),
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
            duration_ms: 1000,
            token_usage: None,
        }
    }

    fn fail_executor() -> ExecutorResult {
        ExecutorResult {
            exit_code: Some(1),
            stdout: String::new(),
            stderr: String::new(),
            timed_out: false,
            duration_ms: 500,
            token_usage: None,
        }
    }

    fn reviewer_complete() -> ReviewerResponse {
        ReviewerResponse {
            verdict: ReviewerVerdict::Complete,
            findings: "All criteria met.".to_string(),
            unresolved_issues: vec![],
            suggested_fix: None,
        }
    }

    fn reviewer_needs_changes(hint: &str) -> ReviewerResponse {
        ReviewerResponse {
            verdict: ReviewerVerdict::NeedsChanges,
            findings: "Some issues remain.".to_string(),
            unresolved_issues: vec!["issue-1".to_string()],
            suggested_fix: Some(hint.to_string()),
        }
    }

    fn reviewer_blocked() -> ReviewerResponse {
        ReviewerResponse {
            verdict: ReviewerVerdict::Blocked,
            findings: "Cannot proceed due to external constraint.".to_string(),
            unresolved_issues: vec![],
            suggested_fix: None,
        }
    }

    fn passed_verification() -> VerificationResult {
        VerificationResult {
            command: "cargo test".to_string(),
            status: VerificationStatus::Passed,
            exit_code: Some(0),
            output: "All tests passed.".to_string(),
        }
    }

    fn failed_verification() -> VerificationResult {
        VerificationResult {
            command: "cargo test".to_string(),
            status: VerificationStatus::Failed,
            exit_code: Some(1),
            output: "2 tests failed.".to_string(),
        }
    }

    fn base_report() -> ExecutionReport {
        ExecutionReport {
            task_description: "Implement feature X".to_string(),
            acceptance_criteria: vec!["Feature X works".to_string()],
            iteration: 1,
            max_iterations: 5,
            executor_result: Some(ok_executor()),
            verification_results: vec![],
            reviewer_response: None,
            previous_unresolved_issues: vec![],
            cancelled: false,
            invalid_config_detail: None,
            changed_files_count: 2,
            verification_configured: true,
        }
    }

    // --- Test cases ---

    #[test]
    fn test_complete_on_successful_execution_with_full_evidence() {
        let report = ExecutionReport {
            executor_result: Some(ok_executor()),
            verification_results: vec![passed_verification()],
            reviewer_response: Some(reviewer_complete()),
            changed_files_count: 3,
            ..base_report()
        };
        assert_eq!(evaluate(&report), PolicyDecision::Complete);
    }

    #[test]
    fn test_repair_on_compiler_error_in_stderr() {
        let mut exec = fail_executor();
        exec.stderr = "error[E0001]: cannot find value `x` in this scope".to_string();
        let report = ExecutionReport {
            executor_result: Some(exec),
            ..base_report()
        };
        match evaluate(&report) {
            PolicyDecision::Repair { failure_class, .. } => {
                assert_eq!(failure_class, FailureClass::CompilerError);
            }
            other => panic!("Expected Repair(CompilerError), got {:?}", other),
        }
    }

    #[test]
    fn test_repair_on_build_failure_in_stdout() {
        let mut exec = fail_executor();
        exec.stdout = "Build failed with 3 errors".to_string();
        let report = ExecutionReport {
            executor_result: Some(exec),
            ..base_report()
        };
        match evaluate(&report) {
            PolicyDecision::Repair { failure_class, .. } => {
                assert_eq!(failure_class, FailureClass::CompilerError);
            }
            other => panic!("Expected Repair(CompilerError), got {:?}", other),
        }
    }

    #[test]
    fn test_repair_on_test_failure() {
        let mut exec = fail_executor();
        exec.stderr = "test failed: 3 tests failed".to_string();
        let report = ExecutionReport {
            executor_result: Some(exec),
            ..base_report()
        };
        match evaluate(&report) {
            PolicyDecision::Repair { failure_class, .. } => {
                assert_eq!(failure_class, FailureClass::TestFailure);
            }
            other => panic!("Expected Repair(TestFailure), got {:?}", other),
        }
    }

    #[test]
    fn test_repair_on_timeout() {
        let mut exec = ok_executor();
        exec.timed_out = true;
        exec.exit_code = None;
        let report = ExecutionReport {
            executor_result: Some(exec),
            ..base_report()
        };
        match evaluate(&report) {
            PolicyDecision::Repair { failure_class, .. } => {
                assert_eq!(failure_class, FailureClass::Timeout);
            }
            other => panic!("Expected Repair(Timeout), got {:?}", other),
        }
    }

    #[test]
    fn test_stop_on_retry_exhaustion() {
        let report = ExecutionReport {
            iteration: 5,
            max_iterations: 5,
            reviewer_response: Some(reviewer_needs_changes("fix it")),
            ..base_report()
        };
        match evaluate(&report) {
            PolicyDecision::Stop {
                reason: StopReason::RetryBudgetExhausted { attempts, max },
            } => {
                assert_eq!(attempts, 5);
                assert_eq!(max, 5);
            }
            other => panic!("Expected Stop(RetryBudgetExhausted), got {:?}", other),
        }
    }

    #[test]
    fn test_stop_on_repeated_identical_failure() {
        let issues = vec!["Unresolved issue A".to_string()];
        let report = ExecutionReport {
            previous_unresolved_issues: issues.clone(),
            reviewer_response: Some(ReviewerResponse {
                verdict: ReviewerVerdict::NeedsChanges,
                findings: "Same issue again.".to_string(),
                unresolved_issues: issues.clone(),
                suggested_fix: None,
            }),
            ..base_report()
        };
        match evaluate(&report) {
            PolicyDecision::Stop {
                reason: StopReason::RepeatedIdenticalFailure { issue },
            } => {
                assert!(issue.contains("Unresolved issue A"));
            }
            other => panic!("Expected Stop(RepeatedIdenticalFailure), got {:?}", other),
        }
    }

    #[test]
    fn test_stop_on_invalid_configuration() {
        let report = ExecutionReport {
            invalid_config_detail: Some("Missing required API key".to_string()),
            ..base_report()
        };
        match evaluate(&report) {
            PolicyDecision::Stop {
                reason: StopReason::InvalidConfiguration { detail },
            } => {
                assert!(detail.contains("API key"));
            }
            other => panic!("Expected Stop(InvalidConfiguration), got {:?}", other),
        }
    }

    #[test]
    fn test_stop_on_cancellation() {
        let report = ExecutionReport {
            cancelled: true,
            ..base_report()
        };
        assert_eq!(
            evaluate(&report),
            PolicyDecision::Stop {
                reason: StopReason::Cancelled
            }
        );
    }

    #[test]
    fn test_stop_on_reviewer_blocked() {
        let report = ExecutionReport {
            reviewer_response: Some(reviewer_blocked()),
            ..base_report()
        };
        match evaluate(&report) {
            PolicyDecision::Stop {
                reason: StopReason::PermanentFailure { failure_class },
            } => {
                assert_eq!(failure_class, FailureClass::ReviewFailure);
            }
            other => panic!("Expected Stop(PermanentFailure), got {:?}", other),
        }
    }

    #[test]
    fn test_zero_exit_with_reviewer_complete_and_no_verification_command_completes() {
        // No verification command is configured: there is nothing to fail, so
        // a clean executor exit plus a Complete reviewer verdict completes.
        let report = ExecutionReport {
            reviewer_response: Some(reviewer_complete()),
            changed_files_count: 2,
            verification_configured: false,
            ..base_report()
        };
        assert_eq!(evaluate(&report), PolicyDecision::Complete);
    }

    #[test]
    fn test_configured_verification_without_results_is_missing_evidence() {
        // A verification command is configured but no results were collected:
        // this must be reported as missing evidence and cannot complete.
        let report = ExecutionReport {
            reviewer_response: Some(reviewer_complete()),
            verification_results: vec![],
            verification_configured: true,
            ..base_report()
        };
        match evaluate(&report) {
            PolicyDecision::Repair {
                failure_class,
                corrective_prompt_hint,
            } => {
                assert_eq!(failure_class, FailureClass::MissingEvidence);
                assert!(corrective_prompt_hint.contains(
                    "Verification is configured but no verification command results were collected"
                ));
            }
            other => panic!("Expected Repair(MissingEvidence), got {:?}", other),
        }
    }

    #[test]
    fn test_configured_verification_with_failed_results_is_reported_and_blocked() {
        // A verification command is configured and produced a failed result:
        // this must be reported as a failure and cannot complete.
        let report = ExecutionReport {
            reviewer_response: Some(reviewer_complete()),
            verification_results: vec![failed_verification()],
            verification_configured: true,
            ..base_report()
        };
        match evaluate(&report) {
            PolicyDecision::Repair {
                failure_class,
                corrective_prompt_hint,
            } => {
                // Verification output contains test-failure language, so the
                // more specific classification applies.
                assert_eq!(failure_class, FailureClass::TestFailure);
                assert!(corrective_prompt_hint.contains("cargo test"));
            }
            other => panic!("Expected Repair(TestFailure), got {:?}", other),
        }
    }

    #[test]
    fn test_nonzero_exit_cannot_complete_even_with_positive_review_and_verification() {
        let report = ExecutionReport {
            executor_result: Some(fail_executor()),
            verification_results: vec![passed_verification()],
            reviewer_response: Some(reviewer_complete()),
            changed_files_count: 2,
            ..base_report()
        };
        match evaluate(&report) {
            PolicyDecision::Repair { failure_class, .. } => {
                assert_eq!(failure_class, FailureClass::ExecutorCrash);
            }
            other => panic!("Expected Repair(ExecutorCrash), got {:?}", other),
        }
    }

    #[test]
    fn test_runtime_failure_is_distinguished_from_generic_executor_failure() {
        let mut executor = fail_executor();
        executor.stderr = "thread panicked: runtime error".to_string();
        let report = ExecutionReport {
            executor_result: Some(executor),
            ..base_report()
        };
        match evaluate(&report) {
            PolicyDecision::Repair { failure_class, .. } => {
                assert_eq!(failure_class, FailureClass::RuntimeError);
            }
            other => panic!("Expected Repair(RuntimeError), got {:?}", other),
        }
    }

    #[test]
    fn test_verification_failure_is_classified_separately() {
        let mut verification = failed_verification();
        verification.output = "command returned an unexpected result".to_string();
        let report = ExecutionReport {
            verification_results: vec![verification],
            ..base_report()
        };
        match evaluate(&report) {
            PolicyDecision::Repair {
                failure_class,
                corrective_prompt_hint,
            } => {
                assert_eq!(failure_class, FailureClass::VerificationFailure);
                assert!(corrective_prompt_hint.contains("unexpected result"));
            }
            other => panic!("Expected Repair(VerificationFailure), got {:?}", other),
        }
    }

    #[test]
    fn test_verification_timeout_is_classified_as_timeout() {
        let mut verification = failed_verification();
        verification.output = "Verification command timed out after 300 seconds".to_string();
        let report = ExecutionReport {
            verification_results: vec![verification],
            ..base_report()
        };
        match evaluate(&report) {
            PolicyDecision::Repair { failure_class, .. } => {
                assert_eq!(failure_class, FailureClass::Timeout);
            }
            other => panic!("Expected Repair(Timeout), got {:?}", other),
        }
    }

    #[test]
    fn test_malformed_passing_verification_report_cannot_complete() {
        let mut verification = passed_verification();
        verification.exit_code = Some(2);
        let report = ExecutionReport {
            verification_results: vec![verification],
            reviewer_response: Some(reviewer_complete()),
            changed_files_count: 2,
            ..base_report()
        };
        match evaluate(&report) {
            PolicyDecision::Repair { failure_class, .. } => {
                assert_eq!(failure_class, FailureClass::MalformedReport);
            }
            other => panic!("Expected Repair(MalformedReport), got {:?}", other),
        }
    }

    #[test]
    fn test_repair_decisions_stop_at_iteration_limit() {
        let mut executor = fail_executor();
        executor.stderr = "error[E0001]: type mismatch".to_string();
        let report = ExecutionReport {
            executor_result: Some(executor),
            iteration: 5,
            max_iterations: 5,
            ..base_report()
        };
        assert_eq!(
            evaluate(&report),
            PolicyDecision::Stop {
                reason: StopReason::RetryBudgetExhausted {
                    attempts: 5,
                    max: 5
                }
            }
        );
    }

    #[test]
    fn test_repair_false_success() {
        // exit 0, reviewer Complete, 0 changed files, non-empty acceptance criteria
        let report = ExecutionReport {
            executor_result: Some(ok_executor()),
            reviewer_response: Some(reviewer_complete()),
            changed_files_count: 0,
            acceptance_criteria: vec!["Feature X works".to_string()],
            ..base_report()
        };
        match evaluate(&report) {
            PolicyDecision::Repair { failure_class, .. } => {
                assert_eq!(failure_class, FailureClass::FalseSuccess);
            }
            other => panic!("Expected Repair(FalseSuccess), got {:?}", other),
        }
    }

    #[test]
    fn test_verify_when_no_reviewer_response() {
        let report = ExecutionReport {
            executor_result: Some(ok_executor()),
            reviewer_response: None,
            changed_files_count: 2,
            ..base_report()
        };
        assert_eq!(evaluate(&report), PolicyDecision::Verify);
    }

    #[test]
    fn test_improve_corrective_prompt_incorporates_past_failures() {
        let base = "Fix the bug in module A.";
        let previous = vec![
            "Module A still crashes on empty input.".to_string(),
            "Missing null check in function foo.".to_string(),
        ];
        let improved = improve_corrective_prompt(base, &FailureClass::ExecutorCrash, &previous);

        // Must contain the base prompt.
        assert!(
            improved.contains(base),
            "Improved prompt must contain the base prompt"
        );
        // Must reference the failure class.
        assert!(
            improved.contains(FailureClass::ExecutorCrash.label()),
            "Improved prompt must reference the failure class label"
        );
        // Must reference both past failure items.
        assert!(
            improved.contains("Module A still crashes on empty input."),
            "Improved prompt must include first previous failure"
        );
        assert!(
            improved.contains("Missing null check in function foo."),
            "Improved prompt must include second previous failure"
        );
    }

    #[test]
    fn test_should_escalate_on_destructive_action() {
        let report = ExecutionReport {
            task_description: "Perform destructive cleanup of old data".to_string(),
            ..base_report()
        };
        let result = should_escalate_to_user(&report);
        assert!(result.is_some(), "Should escalate on destructive task");
        assert!(
            result.unwrap().contains("destructive"),
            "Escalation reason must mention 'destructive'"
        );
    }

    #[test]
    fn test_cancellation_wins_over_all_other_conditions() {
        // cancelled=true even with invalid config and retry exhaustion → Stop { Cancelled }
        let report = ExecutionReport {
            cancelled: true,
            invalid_config_detail: Some("broken config".to_string()),
            iteration: 5,
            max_iterations: 5,
            ..base_report()
        };
        assert_eq!(
            evaluate(&report),
            PolicyDecision::Stop {
                reason: StopReason::Cancelled
            }
        );
    }
}
