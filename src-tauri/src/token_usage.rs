//! Token Usage Tracking Module
//!
//! This module provides accurate token usage tracking with support for:
//! - Input and output token counting
//! - Association with providers, models, projects, and tasks
//! - Historical usage records
//! - Persistence across application restarts
//! - Provider-reported token counts (when available)
//! - Unknown token counts (when provider doesn't report)
//! - Avoiding double-counting of retries

use crate::models::{CliTokenUsage, ProviderConfig, TaskRecord};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Token Usage Types
// ---------------------------------------------------------------------------

/// Token usage for a single request/response
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct TokenUsage {
    /// Number of input tokens (prompt tokens)
    #[serde(default)]
    pub input_tokens: Option<u64>,
    /// Number of output tokens (completion tokens)
    #[serde(default)]
    pub output_tokens: Option<u64>,
    /// Total tokens (input + output), if both are known
    #[serde(default)]
    pub total_tokens: Option<u64>,
    /// Whether token counts are unknown (provider didn't report them)
    #[serde(default)]
    pub tokens_unknown: bool,
    /// Timestamp of the usage
    #[serde(default)]
    pub timestamp: String,
    /// Duration of the request in milliseconds
    #[serde(default)]
    pub duration_ms: Option<u64>,
}

impl TokenUsage {
    pub fn new() -> Self {
        Self {
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            tokens_unknown: true,
            timestamp: Utc::now().to_rfc3339(),
            duration_ms: None,
        }
    }

    /// Create with known token counts
    pub fn with_counts(input_tokens: u64, output_tokens: u64) -> Self {
        Self {
            input_tokens: Some(input_tokens),
            output_tokens: Some(output_tokens),
            total_tokens: Some(input_tokens + output_tokens),
            tokens_unknown: false,
            timestamp: Utc::now().to_rfc3339(),
            duration_ms: None,
        }
    }

    /// Create with unknown tokens
    pub fn unknown() -> Self {
        Self {
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            tokens_unknown: true,
            timestamp: Utc::now().to_rfc3339(),
            duration_ms: None,
        }
    }

    /// Create from provider-reported counts
    pub fn from_provider(
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        duration_ms: Option<u64>,
    ) -> Self {
        let total = match (input_tokens, output_tokens) {
            (Some(i), Some(o)) => Some(i + o),
            _ => None,
        };

        Self {
            input_tokens,
            output_tokens,
            total_tokens: total,
            tokens_unknown: input_tokens.is_none() && output_tokens.is_none(),
            timestamp: Utc::now().to_rfc3339(),
            duration_ms,
        }
    }

    /// Create from explicit usage metadata reported by a coding-agent CLI.
    ///
    /// Only reported counts are used; nothing is estimated from output size.
    /// Claude Code reports cache tokens *in addition to* `input_tokens`, while
    /// Codex reports `cached_input_tokens` as a *subset* of `input_tokens`.
    /// Both formats are combined accordingly so reported tokens are not
    /// double-counted.
    pub fn from_cli_usage(cli: &CliTokenUsage, duration_ms: Option<u64>) -> Self {
        let input_tokens = if cli.format == "claude_code_json" {
            sum_reported(&[
                cli.input_tokens,
                cli.cache_creation_input_tokens,
                cli.cache_read_input_tokens,
            ])
        } else {
            cli.input_tokens
        };
        let output_tokens = cli.output_tokens;
        let total_tokens = match (input_tokens, output_tokens) {
            (Some(input), Some(output)) => Some(input + output),
            _ => None,
        };

        Self {
            input_tokens,
            output_tokens,
            total_tokens,
            tokens_unknown: input_tokens.is_none() && output_tokens.is_none(),
            timestamp: Utc::now().to_rfc3339(),
            duration_ms,
        }
    }

    /// Get input tokens or 0 if unknown
    pub fn input_tokens_or_zero(&self) -> u64 {
        self.input_tokens.unwrap_or(0)
    }

    /// Get output tokens or 0 if unknown
    pub fn output_tokens_or_zero(&self) -> u64 {
        self.output_tokens.unwrap_or(0)
    }

    /// Get total tokens or 0 if unknown
    pub fn total_tokens_or_zero(&self) -> u64 {
        self.total_tokens.unwrap_or(0)
    }

    /// Check if this usage has known token counts
    pub fn has_known_counts(&self) -> bool {
        !self.tokens_unknown && self.total_tokens.is_some()
    }

    /// Add another token usage to this one
    pub fn add(&mut self, other: &TokenUsage) {
        if let Some(other_input) = other.input_tokens {
            self.input_tokens = Some(self.input_tokens_or_zero() + other_input);
        }
        if let Some(other_output) = other.output_tokens {
            self.output_tokens = Some(self.output_tokens_or_zero() + other_output);
        }
        self.total_tokens = Some(self.input_tokens_or_zero() + self.output_tokens_or_zero());
        if !other.tokens_unknown {
            self.tokens_unknown = false;
        }
    }

    /// Merge with another token usage
    pub fn merge(&self, other: &TokenUsage) -> TokenUsage {
        let mut merged = self.clone();
        merged.add(other);
        merged
    }
}

/// Sum a set of optional reported counts. Returns `None` only when *none* of
/// the values were reported, so partial metadata stays partial rather than
/// silently becoming zero.
fn sum_reported(values: &[Option<u64>]) -> Option<u64> {
    let present: Vec<u64> = values.iter().filter_map(|value| *value).collect();
    if present.is_empty() {
        None
    } else {
        Some(present.iter().sum())
    }
}

/// Context for token usage association
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct UsageContext {
    /// Provider ID or name
    #[serde(default)]
    pub provider_id: Option<String>,
    /// Provider name
    #[serde(default)]
    pub provider_name: Option<String>,
    /// Model ID
    #[serde(default)]
    pub model_id: Option<String>,
    /// Model name
    #[serde(default)]
    pub model_name: Option<String>,
    /// Project ID
    #[serde(default)]
    pub project_id: Option<String>,
    /// Project name
    #[serde(default)]
    pub project_name: Option<String>,
    /// Task ID
    #[serde(default)]
    pub task_id: Option<String>,
    /// Session ID
    #[serde(default)]
    pub session_id: Option<String>,
    /// Responsibility (planning, coding, review, etc.)
    #[serde(default)]
    pub responsibility: Option<String>,
    /// Additional metadata
    #[serde(default)]
    pub metadata: HashMap<String, String>,
}

impl UsageContext {
    pub fn new() -> Self {
        Self::default()
    }

    /// Create context from a task record
    pub fn from_task(task: &TaskRecord) -> Self {
        Self {
            provider_id: task.provider_profile_id.clone(),
            provider_name: None,
            model_id: task
                .execution_config
                .as_ref()
                .and_then(|c| c.cloud_model.clone()),
            model_name: None,
            project_id: task.project_id.clone(),
            project_name: None,
            task_id: Some(task.id.clone()),
            session_id: task.source_session_id.clone(),
            responsibility: None,
            metadata: HashMap::new(),
        }
    }

    /// Create context from provider config
    pub fn from_provider_config(config: &ProviderConfig) -> Self {
        Self {
            provider_id: None,
            provider_name: None,
            model_id: config.cloud_model.clone(),
            model_name: None,
            project_id: None,
            project_name: None,
            task_id: None,
            session_id: None,
            responsibility: None,
            metadata: HashMap::new(),
        }
    }

    /// Check if context has provider information
    pub fn has_provider(&self) -> bool {
        self.provider_id.is_some() || self.provider_name.is_some()
    }

    /// Check if context has model information
    pub fn has_model(&self) -> bool {
        self.model_id.is_some() || self.model_name.is_some()
    }

    /// Check if context has project information
    pub fn has_project(&self) -> bool {
        self.project_id.is_some() || self.project_name.is_some()
    }

    /// Check if context has task information
    pub fn has_task(&self) -> bool {
        self.task_id.is_some()
    }
}

/// A single token usage record
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TokenUsageRecord {
    /// Unique record ID
    pub id: String,
    /// Token usage data
    pub usage: TokenUsage,
    /// Usage context
    pub context: UsageContext,
    /// Request/operation type (e.g., "chat", "plan", "review", "execute")
    pub operation_type: String,
    /// Whether this is a retry of a previous request
    #[serde(default)]
    pub is_retry: bool,
    /// ID of the original request if this is a retry
    #[serde(default)]
    pub original_request_id: Option<String>,
    /// Whether this usage should be counted (false for retries if we're avoiding double-counting)
    #[serde(default = "default_true")]
    pub count_towards_totals: bool,
    /// Stable key that makes recording idempotent. A replayed or recovered
    /// result carries the same key as the original, so it is counted once.
    /// `None` means the record is not deduplicated (legacy/provider records).
    #[serde(default)]
    pub dedup_key: Option<String>,
    /// Created timestamp
    pub created_at: String,
}

impl TokenUsageRecord {
    pub fn new(usage: TokenUsage, context: UsageContext, operation_type: String) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            usage,
            context,
            operation_type,
            is_retry: false,
            original_request_id: None,
            count_towards_totals: true,
            dedup_key: None,
            created_at: Utc::now().to_rfc3339(),
        }
    }

    /// Create a retry record
    pub fn retry_of(original: &TokenUsageRecord, usage: TokenUsage) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            usage,
            context: original.context.clone(),
            operation_type: original.operation_type.clone(),
            is_retry: true,
            original_request_id: Some(original.id.clone()),
            count_towards_totals: false, // Don't double-count retries
            dedup_key: None,
            created_at: Utc::now().to_rfc3339(),
        }
    }

    /// Mark as a retry that should be counted (e.g., for analytics)
    pub fn with_counting(mut self) -> Self {
        self.count_towards_totals = true;
        self
    }

    /// Get the effective usage (considering count_towards_totals)
    pub fn effective_usage(&self) -> TokenUsage {
        if self.count_towards_totals {
            self.usage.clone()
        } else {
            TokenUsage::default()
        }
    }
}

/// Aggregated token usage statistics
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct TokenUsageStats {
    /// Total input tokens
    #[serde(default)]
    pub total_input_tokens: u64,
    /// Total output tokens
    #[serde(default)]
    pub total_output_tokens: u64,
    /// Total tokens
    #[serde(default)]
    pub total_tokens: u64,
    /// Number of requests
    #[serde(default)]
    pub request_count: u64,
    /// Number of requests with unknown tokens
    #[serde(default)]
    pub unknown_request_count: u64,
    /// Usage by provider
    #[serde(default)]
    pub by_provider: HashMap<String, ProviderUsageStats>,
    /// Usage by model
    #[serde(default)]
    pub by_model: HashMap<String, ModelUsageStats>,
    /// Usage by project
    #[serde(default)]
    pub by_project: HashMap<String, ProjectUsageStats>,
    /// Usage by task
    #[serde(default)]
    pub by_task: HashMap<String, TaskUsageStats>,
    /// Usage by responsibility
    #[serde(default)]
    pub by_responsibility: HashMap<String, ResponsibilityUsageStats>,
    /// Usage over time (by date)
    #[serde(default)]
    pub by_date: HashMap<String, DateUsageStats>,
}

impl TokenUsageStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a token usage record to the stats
    pub fn add_record(&mut self, record: &TokenUsageRecord) {
        let effective_usage = record.effective_usage();
        self.total_input_tokens += effective_usage.input_tokens_or_zero();
        self.total_output_tokens += effective_usage.output_tokens_or_zero();
        self.total_tokens += effective_usage.total_tokens_or_zero();
        self.request_count += 1;
        if record.usage.tokens_unknown {
            self.unknown_request_count += 1;
        }
        // By provider
        if let Some(provider_id) = &record.context.provider_id {
            let entry = self.by_provider.entry(provider_id.clone()).or_default();
            entry.add_usage(&effective_usage);
            entry.request_count += 1;
        }
        // By model
        if let Some(model_id) = &record.context.model_id {
            let entry = self.by_model.entry(model_id.clone()).or_default();
            entry.add_usage(&effective_usage);
            entry.request_count += 1;
        }
        // By project
        if let Some(project_id) = &record.context.project_id {
            let entry = self.by_project.entry(project_id.clone()).or_default();
            entry.add_usage(&effective_usage);
            entry.request_count += 1;
        }
        // By task
        if let Some(task_id) = &record.context.task_id {
            let entry = self.by_task.entry(task_id.clone()).or_default();
            entry.add_usage(&effective_usage);
            entry.request_count += 1;
        }
        // By responsibility
        if let Some(responsibility) = &record.context.responsibility {
            let entry = self
                .by_responsibility
                .entry(responsibility.clone())
                .or_default();
            entry.add_usage(&effective_usage);
            entry.request_count += 1;
        }
        // By date
        let date = record
            .created_at
            .split('T')
            .next()
            .unwrap_or("")
            .to_string();
        if !date.is_empty() {
            let entry = self.by_date.entry(date).or_default();
            entry.add_usage(&effective_usage);
            entry.request_count += 1;
        }
    }

    /// Remove a token usage record from the stats
    pub fn remove_record(&mut self, record: &TokenUsageRecord) {
        let effective_usage = record.effective_usage();
        self.total_input_tokens = self
            .total_input_tokens
            .saturating_sub(effective_usage.input_tokens_or_zero());
        self.total_output_tokens = self
            .total_output_tokens
            .saturating_sub(effective_usage.output_tokens_or_zero());
        self.total_tokens = self
            .total_tokens
            .saturating_sub(effective_usage.total_tokens_or_zero());
        if self.request_count > 0 {
            self.request_count -= 1;
        }
        if record.usage.tokens_unknown && self.unknown_request_count > 0 {
            self.unknown_request_count -= 1;
        }
        // By provider
        if let Some(provider_id) = &record.context.provider_id {
            if let Some(entry) = self.by_provider.get_mut(provider_id) {
                entry.subtract_usage(&effective_usage);
                if entry.request_count > 0 {
                    entry.request_count -= 1;
                }
            }
        }
        // Similar for other dimensions...
    }

    /// Merge another stats object into this one
    pub fn merge(&mut self, other: &TokenUsageStats) {
        self.total_input_tokens += other.total_input_tokens;
        self.total_output_tokens += other.total_output_tokens;
        self.total_tokens += other.total_tokens;
        self.request_count += other.request_count;
        self.unknown_request_count += other.unknown_request_count;
        for (key, value) in &other.by_provider {
            let entry = self.by_provider.entry(key.clone()).or_default();
            entry.merge(value);
        }
        for (key, value) in &other.by_model {
            let entry = self.by_model.entry(key.clone()).or_default();
            entry.merge(value);
        }
        for (key, value) in &other.by_project {
            let entry = self.by_project.entry(key.clone()).or_default();
            entry.merge(value);
        }
        for (key, value) in &other.by_task {
            let entry = self.by_task.entry(key.clone()).or_default();
            entry.merge(value);
        }
        for (key, value) in &other.by_responsibility {
            let entry = self.by_responsibility.entry(key.clone()).or_default();
            entry.merge(value);
        }
        for (key, value) in &other.by_date {
            let entry = self.by_date.entry(key.clone()).or_default();
            entry.merge(value);
        }
    }
}

/// Usage stats for a provider
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderUsageStats {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub request_count: u64,
}

impl ProviderUsageStats {
    pub fn add_usage(&mut self, usage: &TokenUsage) {
        self.input_tokens += usage.input_tokens_or_zero();
        self.output_tokens += usage.output_tokens_or_zero();
        self.total_tokens += usage.total_tokens_or_zero();
    }
    pub fn subtract_usage(&mut self, usage: &TokenUsage) {
        self.input_tokens = self
            .input_tokens
            .saturating_sub(usage.input_tokens_or_zero());
        self.output_tokens = self
            .output_tokens
            .saturating_sub(usage.output_tokens_or_zero());
        self.total_tokens = self
            .total_tokens
            .saturating_sub(usage.total_tokens_or_zero());
    }
    pub fn merge(&mut self, other: &ProviderUsageStats) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.total_tokens += other.total_tokens;
        self.request_count += other.request_count;
    }
}

/// Usage stats for a model
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelUsageStats {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub request_count: u64,
}

impl ModelUsageStats {
    pub fn add_usage(&mut self, usage: &TokenUsage) {
        self.input_tokens += usage.input_tokens_or_zero();
        self.output_tokens += usage.output_tokens_or_zero();
        self.total_tokens += usage.total_tokens_or_zero();
    }
    pub fn subtract_usage(&mut self, usage: &TokenUsage) {
        self.input_tokens = self
            .input_tokens
            .saturating_sub(usage.input_tokens_or_zero());
        self.output_tokens = self
            .output_tokens
            .saturating_sub(usage.output_tokens_or_zero());
        self.total_tokens = self
            .total_tokens
            .saturating_sub(usage.total_tokens_or_zero());
    }
    pub fn merge(&mut self, other: &ModelUsageStats) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.total_tokens += other.total_tokens;
        self.request_count += other.request_count;
    }
}

/// Usage stats for a project
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct ProjectUsageStats {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub request_count: u64,
}

impl ProjectUsageStats {
    pub fn add_usage(&mut self, usage: &TokenUsage) {
        self.input_tokens += usage.input_tokens_or_zero();
        self.output_tokens += usage.output_tokens_or_zero();
        self.total_tokens += usage.total_tokens_or_zero();
    }
    pub fn subtract_usage(&mut self, usage: &TokenUsage) {
        self.input_tokens = self
            .input_tokens
            .saturating_sub(usage.input_tokens_or_zero());
        self.output_tokens = self
            .output_tokens
            .saturating_sub(usage.output_tokens_or_zero());
        self.total_tokens = self
            .total_tokens
            .saturating_sub(usage.total_tokens_or_zero());
    }
    pub fn merge(&mut self, other: &ProjectUsageStats) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.total_tokens += other.total_tokens;
        self.request_count += other.request_count;
    }
}

/// Usage stats for a task
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct TaskUsageStats {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub request_count: u64,
    /// Task ID for reference
    #[serde(default)]
    pub task_id: String,
}

impl TaskUsageStats {
    pub fn new(task_id: String) -> Self {
        Self {
            input_tokens: 0,
            output_tokens: 0,
            total_tokens: 0,
            request_count: 0,
            task_id,
        }
    }
    pub fn add_usage(&mut self, usage: &TokenUsage) {
        self.input_tokens += usage.input_tokens_or_zero();
        self.output_tokens += usage.output_tokens_or_zero();
        self.total_tokens += usage.total_tokens_or_zero();
    }
    pub fn subtract_usage(&mut self, usage: &TokenUsage) {
        self.input_tokens = self
            .input_tokens
            .saturating_sub(usage.input_tokens_or_zero());
        self.output_tokens = self
            .output_tokens
            .saturating_sub(usage.output_tokens_or_zero());
        self.total_tokens = self
            .total_tokens
            .saturating_sub(usage.total_tokens_or_zero());
    }
    pub fn merge(&mut self, other: &TaskUsageStats) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.total_tokens += other.total_tokens;
        self.request_count += other.request_count;
    }
}

/// Usage stats for a responsibility
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct ResponsibilityUsageStats {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub request_count: u64,
}

impl ResponsibilityUsageStats {
    pub fn add_usage(&mut self, usage: &TokenUsage) {
        self.input_tokens += usage.input_tokens_or_zero();
        self.output_tokens += usage.output_tokens_or_zero();
        self.total_tokens += usage.total_tokens_or_zero();
    }
    pub fn subtract_usage(&mut self, usage: &TokenUsage) {
        self.input_tokens = self
            .input_tokens
            .saturating_sub(usage.input_tokens_or_zero());
        self.output_tokens = self
            .output_tokens
            .saturating_sub(usage.output_tokens_or_zero());
        self.total_tokens = self
            .total_tokens
            .saturating_sub(usage.total_tokens_or_zero());
    }
    pub fn merge(&mut self, other: &ResponsibilityUsageStats) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.total_tokens += other.total_tokens;
        self.request_count += other.request_count;
    }
}

/// Usage stats for a date
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct DateUsageStats {
    #[serde(default)]
    pub input_tokens: u64,
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
    #[serde(default)]
    pub request_count: u64,
}

impl DateUsageStats {
    pub fn add_usage(&mut self, usage: &TokenUsage) {
        self.input_tokens += usage.input_tokens_or_zero();
        self.output_tokens += usage.output_tokens_or_zero();
        self.total_tokens += usage.total_tokens_or_zero();
    }
    pub fn subtract_usage(&mut self, usage: &TokenUsage) {
        self.input_tokens = self
            .input_tokens
            .saturating_sub(usage.input_tokens_or_zero());
        self.output_tokens = self
            .output_tokens
            .saturating_sub(usage.output_tokens_or_zero());
        self.total_tokens = self
            .total_tokens
            .saturating_sub(usage.total_tokens_or_zero());
    }
    pub fn merge(&mut self, other: &DateUsageStats) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.total_tokens += other.total_tokens;
        self.request_count += other.request_count;
    }
}

fn default_true() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Token Usage Tracker
// ---------------------------------------------------------------------------

/// Token usage tracker that manages recording and querying token usage
pub struct TokenUsageTracker {
    /// All usage records
    records: Vec<TokenUsageRecord>,
    /// Index by record ID for fast lookup
    records_by_id: HashMap<String, usize>,
    /// Index by task ID
    records_by_task: HashMap<String, Vec<usize>>,
    /// Index by session ID
    records_by_session: HashMap<String, Vec<usize>>,
    /// Index by provider ID
    records_by_provider: HashMap<String, Vec<usize>>,
    /// Index by model ID
    records_by_model: HashMap<String, Vec<usize>>,
    /// Index by project ID
    records_by_project: HashMap<String, Vec<usize>>,
    /// Storage path for persistence
    storage_path: PathBuf,
}

impl TokenUsageTracker {
    pub fn new(storage_path: impl AsRef<Path>) -> Self {
        Self {
            records: Vec::new(),
            records_by_id: HashMap::new(),
            records_by_task: HashMap::new(),
            records_by_session: HashMap::new(),
            records_by_provider: HashMap::new(),
            records_by_model: HashMap::new(),
            records_by_project: HashMap::new(),
            storage_path: storage_path.as_ref().to_path_buf(),
        }
    }

    /// Record a token usage
    pub fn record_usage(
        &mut self,
        usage: TokenUsage,
        context: UsageContext,
        operation_type: String,
    ) -> TokenUsageRecord {
        let record = TokenUsageRecord::new(usage, context, operation_type);
        self.add_record_internal(&record);
        record
    }

    /// Record usage at most once per `dedup_key`.
    ///
    /// Returns `None` when a record with the same key already exists. This makes
    /// replay, retry, and recovery paths idempotent: the same reported result is
    /// never counted twice, while a genuinely new execution gets a new key.
    pub fn record_usage_deduplicated(
        &mut self,
        usage: TokenUsage,
        context: UsageContext,
        operation_type: String,
        dedup_key: Option<String>,
    ) -> Option<TokenUsageRecord> {
        if let Some(key) = dedup_key.as_deref() {
            if self.has_dedup_key(key) {
                return None;
            }
        }
        let mut record = TokenUsageRecord::new(usage, context, operation_type);
        record.dedup_key = dedup_key;
        self.add_record_internal(&record);
        Some(record)
    }

    /// Whether a record with this deduplication key already exists.
    pub fn has_dedup_key(&self, key: &str) -> bool {
        self.records
            .iter()
            .any(|record| record.dedup_key.as_deref() == Some(key))
    }

    /// Record a retry usage
    pub fn record_retry(
        &mut self,
        original_record_id: &str,
        usage: TokenUsage,
        count_towards_totals: bool,
    ) -> Option<TokenUsageRecord> {
        // Find original record
        let original_index = self.records_by_id.get(original_record_id)?;
        let original = &self.records[*original_index];
        let mut retry = TokenUsageRecord::retry_of(original, usage);
        retry.count_towards_totals = count_towards_totals;
        self.add_record_internal(&retry);
        Some(retry)
    }

    /// Add a record without locking (internal use)
    fn add_record_internal(&mut self, record: &TokenUsageRecord) {
        let index = self.records.len();
        self.records.push(record.clone());
        // Update indexes
        self.records_by_id.insert(record.id.clone(), index);
        if let Some(task_id) = &record.context.task_id {
            self.records_by_task
                .entry(task_id.clone())
                .or_default()
                .push(index);
        }
        if let Some(session_id) = &record.context.session_id {
            self.records_by_session
                .entry(session_id.clone())
                .or_default()
                .push(index);
        }
        if let Some(provider_id) = &record.context.provider_id {
            self.records_by_provider
                .entry(provider_id.clone())
                .or_default()
                .push(index);
        }
        if let Some(model_id) = &record.context.model_id {
            self.records_by_model
                .entry(model_id.clone())
                .or_default()
                .push(index);
        }
        if let Some(project_id) = &record.context.project_id {
            self.records_by_project
                .entry(project_id.clone())
                .or_default()
                .push(index);
        }
    }

    /// Get a record by ID
    pub fn get_record(&self, record_id: &str) -> Option<TokenUsageRecord> {
        self.records_by_id
            .get(record_id)
            .map(|&index| self.records[index].clone())
    }

    /// Get all records
    pub fn get_all_records(&self) -> Vec<TokenUsageRecord> {
        self.records.clone()
    }

    /// Get records for a specific task
    pub fn get_records_by_task(&self, task_id: &str) -> Vec<TokenUsageRecord> {
        self.records_by_task
            .get(task_id)
            .map_or_else(Vec::new, |indices| {
                indices.iter().map(|&i| self.records[i].clone()).collect()
            })
    }

    /// Get records for a specific session
    pub fn get_records_by_session(&self, session_id: &str) -> Vec<TokenUsageRecord> {
        self.records_by_session
            .get(session_id)
            .map_or_else(Vec::new, |indices| {
                indices.iter().map(|&i| self.records[i].clone()).collect()
            })
    }

    /// Get records for a specific provider
    pub fn get_records_by_provider(&self, provider_id: &str) -> Vec<TokenUsageRecord> {
        self.records_by_provider
            .get(provider_id)
            .map_or_else(Vec::new, |indices| {
                indices.iter().map(|&i| self.records[i].clone()).collect()
            })
    }

    /// Get records for a specific model
    pub fn get_records_by_model(&self, model_id: &str) -> Vec<TokenUsageRecord> {
        self.records_by_model
            .get(model_id)
            .map_or_else(Vec::new, |indices| {
                indices.iter().map(|&i| self.records[i].clone()).collect()
            })
    }

    /// Get records for a specific project
    pub fn get_records_by_project(&self, project_id: &str) -> Vec<TokenUsageRecord> {
        self.records_by_project
            .get(project_id)
            .map_or_else(Vec::new, |indices| {
                indices.iter().map(|&i| self.records[i].clone()).collect()
            })
    }

    /// Get records within a time range
    pub fn get_records_in_range(
        &self,
        start_time: Option<&str>,
        end_time: Option<&str>,
    ) -> Vec<TokenUsageRecord> {
        self.records
            .iter()
            .filter(|record| {
                let record_time = record.created_at.clone();
                let after_start = start_time.map_or(true, |s| record_time.as_str() >= s);
                let before_end = end_time.map_or(true, |e| record_time.as_str() <= e);
                after_start && before_end
            })
            .cloned()
            .collect()
    }

    /// Get statistics for all usage
    pub fn get_stats(&self) -> TokenUsageStats {
        let mut stats = TokenUsageStats::new();
        for record in &self.records {
            stats.add_record(record);
        }
        stats
    }

    /// Get statistics for a specific task
    pub fn get_stats_by_task(&self, task_id: &str) -> TokenUsageStats {
        let mut stats = TokenUsageStats::new();
        if let Some(indices) = self.records_by_task.get(task_id) {
            for &index in indices {
                stats.add_record(&self.records[index]);
            }
        }
        stats
    }

    /// Get statistics for a specific provider
    pub fn get_stats_by_provider(&self, provider_id: &str) -> TokenUsageStats {
        let mut stats = TokenUsageStats::new();
        if let Some(indices) = self.records_by_provider.get(provider_id) {
            for &index in indices {
                stats.add_record(&self.records[index]);
            }
        }
        stats
    }

    /// Get statistics for a specific model
    pub fn get_stats_by_model(&self, model_id: &str) -> TokenUsageStats {
        let mut stats = TokenUsageStats::new();
        if let Some(indices) = self.records_by_model.get(model_id) {
            for &index in indices {
                stats.add_record(&self.records[index]);
            }
        }
        stats
    }

    /// Get statistics for a specific project
    pub fn get_stats_by_project(&self, project_id: &str) -> TokenUsageStats {
        let mut stats = TokenUsageStats::new();
        if let Some(indices) = self.records_by_project.get(project_id) {
            for &index in indices {
                stats.add_record(&self.records[index]);
            }
        }
        stats
    }

    /// Get statistics for a specific responsibility
    pub fn get_stats_by_responsibility(&self, responsibility: &str) -> TokenUsageStats {
        let mut stats = TokenUsageStats::new();
        for record in &self.records {
            if let Some(resp) = &record.context.responsibility {
                if resp == responsibility {
                    stats.add_record(record);
                }
            }
        }
        stats
    }

    /// Delete a record by ID
    pub fn delete_record(&mut self, record_id: &str) -> bool {
        if let Some(&index) = self.records_by_id.get(record_id) {
            self.remove_record_internal(index);
            true
        } else {
            false
        }
    }

    /// Remove a record by index (internal use)
    fn remove_record_internal(&mut self, index: usize) {
        let record = &self.records[index];
        // Remove from indexes
        self.records_by_id.remove(&record.id);
        if let Some(task_id) = &record.context.task_id {
            if let Some(indices) = self.records_by_task.get_mut(task_id) {
                indices.retain(|&i| i != index);
            }
        }
        if let Some(session_id) = &record.context.session_id {
            if let Some(indices) = self.records_by_session.get_mut(session_id) {
                indices.retain(|&i| i != index);
            }
        }
        if let Some(provider_id) = &record.context.provider_id {
            if let Some(indices) = self.records_by_provider.get_mut(provider_id) {
                indices.retain(|&i| i != index);
            }
        }
        if let Some(model_id) = &record.context.model_id {
            if let Some(indices) = self.records_by_model.get_mut(model_id) {
                indices.retain(|&i| i != index);
            }
        }
        if let Some(project_id) = &record.context.project_id {
            if let Some(indices) = self.records_by_project.get_mut(project_id) {
                indices.retain(|&i| i != index);
            }
        }
        // Remove from records
        self.records.remove(index);
        // Update indexes for remaining records
        self.rebuild_indexes();
    }

    /// Rebuild all indexes after removal
    fn rebuild_indexes(&mut self) {
        self.records_by_id.clear();
        self.records_by_task.clear();
        self.records_by_session.clear();
        self.records_by_provider.clear();
        self.records_by_model.clear();
        self.records_by_project.clear();
        for (index, record) in self.records.iter().enumerate() {
            self.records_by_id.insert(record.id.clone(), index);

            if let Some(task_id) = &record.context.task_id {
                self.records_by_task
                    .entry(task_id.clone())
                    .or_default()
                    .push(index);
            }

            if let Some(session_id) = &record.context.session_id {
                self.records_by_session
                    .entry(session_id.clone())
                    .or_default()
                    .push(index);
            }

            if let Some(provider_id) = &record.context.provider_id {
                self.records_by_provider
                    .entry(provider_id.clone())
                    .or_default()
                    .push(index);
            }

            if let Some(model_id) = &record.context.model_id {
                self.records_by_model
                    .entry(model_id.clone())
                    .or_default()
                    .push(index);
            }

            if let Some(project_id) = &record.context.project_id {
                self.records_by_project
                    .entry(project_id.clone())
                    .or_default()
                    .push(index);
            }
        }
    }

    /// Clear all records
    pub fn clear_all(&mut self) {
        self.records.clear();
        self.records_by_id.clear();
        self.records_by_task.clear();
        self.records_by_session.clear();
        self.records_by_provider.clear();
        self.records_by_model.clear();
        self.records_by_project.clear();
    }

    /// Count total records
    pub fn count(&self) -> usize {
        self.records.len()
    }

    /// Save all records to disk
    pub fn save(&self) -> Result<(), String> {
        self.save_internal()
    }

    /// Save without locking (internal use)
    fn save_internal(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.records)
            .map_err(|e| format!("Failed to serialize token usage records: {}", e))?;
        // Ensure directory exists
        if let Some(parent) = self.storage_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create directory {}: {}", parent.display(), e))?;
        }
        // Write to temp file first
        let temp_path = self.storage_path.with_extension("tmp");
        std::fs::write(&temp_path, &json).map_err(|e| {
            format!(
                "Failed to write token usage to {}: {}",
                temp_path.display(),
                e
            )
        })?;
        // Rename to final path
        std::fs::rename(&temp_path, &self.storage_path)
            .map_err(|e| format!("Failed to rename token usage file: {}", e))?;
        Ok(())
    }

    /// Load records from disk
    pub fn load(&mut self) -> Result<(), String> {
        self.load_internal()
    }

    /// Load without locking (internal use)
    fn load_internal(&mut self) -> Result<(), String> {
        if !self.storage_path.exists() {
            return Ok(());
        }
        let content = std::fs::read_to_string(&self.storage_path).map_err(|e| {
            format!(
                "Failed to read token usage from {}: {}",
                self.storage_path.display(),
                e
            )
        })?;
        let records: Vec<TokenUsageRecord> = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to deserialize token usage records: {}", e))?;
        self.records = records;
        self.rebuild_indexes();
        Ok(())
    }

    /// Set storage path
    pub fn set_storage_path(&mut self, path: impl AsRef<Path>) {
        self.storage_path = path.as_ref().to_path_buf();
    }

    /// Get storage path
    pub fn storage_path(&self) -> &Path {
        &self.storage_path
    }
}

// ---------------------------------------------------------------------------
// Token Usage Reporter (for provider integration)
// ---------------------------------------------------------------------------

/// Trait for types that can report token usage
pub trait TokenUsageReporter {
    /// Report token usage from a response
    /// Returns (input_tokens, output_tokens) if available
    fn report_usage(&self) -> (Option<u64>, Option<u64>);
}

/// Helper struct for extracting token usage from provider responses
pub struct TokenUsageExtractor;

impl TokenUsageExtractor {
    /// Extract token usage from a JSON response
    /// Supports various provider response formats:
    /// - OpenAI: { "usage": { "prompt_tokens": X, "completion_tokens": Y, "total_tokens": Z } }
    /// - Anthropic: { "usage": { "input_tokens": X, "output_tokens": Y } }
    /// - Google: { "usageMetadata": { "promptTokenCount": X, "candidatesTokenCount": Y, "totalTokenCount": Z } }
    pub fn extract_from_json(response_json: &str) -> (Option<u64>, Option<u64>, Option<u64>) {
        let mut input_tokens: Option<u64> = None;
        let mut output_tokens: Option<u64> = None;
        let mut total_tokens: Option<u64> = None;
        // Try to parse as serde_json::Value
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(response_json) {
            // Try OpenAI format
            if let Some(usage) = value.get("usage").and_then(|u| u.as_object()) {
                if let Some(prompt) = usage.get("prompt_tokens").and_then(|t| t.as_u64()) {
                    input_tokens = Some(prompt);
                }
                if let Some(completion) = usage.get("completion_tokens").and_then(|t| t.as_u64()) {
                    output_tokens = Some(completion);
                }
                if let Some(total) = usage.get("total_tokens").and_then(|t| t.as_u64()) {
                    total_tokens = Some(total);
                }
            }

            // Try Anthropic format (also uses "usage" but with different field names)
            if input_tokens.is_none() || output_tokens.is_none() {
                if let Some(usage) = value.get("usage").and_then(|u| u.as_object()) {
                    if let Some(inp) = usage.get("input_tokens").and_then(|t| t.as_u64()) {
                        input_tokens = Some(inp);
                    }
                    if let Some(out) = usage.get("output_tokens").and_then(|t| t.as_u64()) {
                        output_tokens = Some(out);
                    }
                }
            }

            // Try Google format
            if let Some(usage_meta) = value.get("usageMetadata").and_then(|u| u.as_object()) {
                if let Some(prompt) = usage_meta.get("promptTokenCount").and_then(|t| t.as_u64()) {
                    input_tokens = Some(prompt);
                }
                if let Some(candidates) = usage_meta
                    .get("candidatesTokenCount")
                    .and_then(|t| t.as_u64())
                {
                    output_tokens = Some(candidates);
                }
                if let Some(total) = usage_meta.get("totalTokenCount").and_then(|t| t.as_u64()) {
                    total_tokens = Some(total);
                }
            }
        }
        (input_tokens, output_tokens, total_tokens)
    }

    /// Create a TokenUsage from extracted values
    pub fn create_usage_from_extracted(
        input_tokens: Option<u64>,
        output_tokens: Option<u64>,
        total_tokens: Option<u64>,
    ) -> TokenUsage {
        let tokens_unknown = input_tokens.is_none() && output_tokens.is_none();
        TokenUsage {
            input_tokens,
            output_tokens,
            total_tokens: total_tokens
                .or_else(|| input_tokens.and_then(|i| output_tokens.map(|o| i + o))),
            tokens_unknown,
            timestamp: Utc::now().to_rfc3339(),
            duration_ms: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Global Token Usage Tracker
// ---------------------------------------------------------------------------

/// Global token usage tracker instance
/// Uses lazy initialization to avoid requiring app handle at startup
pub struct GlobalTokenUsageTracker {
    inner: Option<Arc<Mutex<TokenUsageTracker>>>,
}

impl GlobalTokenUsageTracker {
    pub fn new() -> Self {
        Self { inner: None }
    }

    /// Initialize the tracker with a storage path
    pub fn initialize(&mut self, storage_path: impl AsRef<Path>) {
        let tracker = TokenUsageTracker::new(storage_path);
        self.inner = Some(Arc::new(Mutex::new(tracker)));
    }

    /// Get a reference to the inner tracker
    pub fn get(&self) -> Option<Arc<Mutex<TokenUsageTracker>>> {
        self.inner.clone()
    }

    /// Record usage through the global tracker
    pub fn record_usage(
        &self,
        usage: TokenUsage,
        context: UsageContext,
        operation_type: String,
    ) -> Option<TokenUsageRecord> {
        let tracker = self.get()?;
        let mut guard = tracker.lock().unwrap();
        Some(guard.record_usage(usage, context, operation_type))
    }

    /// Get stats through the global tracker
    pub fn get_stats(&self) -> Option<TokenUsageStats> {
        let tracker = self.get()?;
        let guard = tracker.lock().unwrap();
        Some(guard.get_stats())
    }

    /// Save through the global tracker
    pub fn save(&self) -> Option<Result<(), String>> {
        let tracker = self.get()?;
        let guard = tracker.lock().unwrap();
        Some(guard.save())
    }

    /// Load through the global tracker
    pub fn load(&self) -> Option<Result<(), String>> {
        let tracker = self.get()?;
        let mut guard = tracker.lock().unwrap();
        Some(guard.load())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_token_usage_new() {
        let usage = TokenUsage::new();
        assert!(usage.tokens_unknown);
        assert!(usage.input_tokens.is_none());
        assert!(usage.output_tokens.is_none());
    }

    #[tokio::test]
    async fn test_token_usage_with_counts() {
        let usage = TokenUsage::with_counts(100, 50);
        assert!(!usage.tokens_unknown);
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, Some(50));
        assert_eq!(usage.total_tokens, Some(150));
    }

    #[tokio::test]
    async fn test_token_usage_unknown() {
        let usage = TokenUsage::unknown();
        assert!(usage.tokens_unknown);
        assert!(usage.input_tokens.is_none());
        assert!(usage.output_tokens.is_none());
    }

    #[tokio::test]
    async fn test_token_usage_from_provider() {
        let usage = TokenUsage::from_provider(Some(100), Some(50), Some(1000));
        assert!(!usage.tokens_unknown);
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, Some(50));
        assert_eq!(usage.total_tokens, Some(150));
        assert_eq!(usage.duration_ms, Some(1000));
    }

    #[tokio::test]
    async fn test_token_usage_or_zero() {
        let usage = TokenUsage::new();
        assert_eq!(usage.input_tokens_or_zero(), 0);
        assert_eq!(usage.output_tokens_or_zero(), 0);
        assert_eq!(usage.total_tokens_or_zero(), 0);
        let usage = TokenUsage::with_counts(100, 50);
        assert_eq!(usage.input_tokens_or_zero(), 100);
        assert_eq!(usage.output_tokens_or_zero(), 50);
        assert_eq!(usage.total_tokens_or_zero(), 150);
    }

    #[tokio::test]
    async fn test_token_usage_add() {
        let mut usage1 = TokenUsage::with_counts(100, 50);
        let usage2 = TokenUsage::with_counts(50, 25);
        usage1.add(&usage2);
        assert_eq!(usage1.input_tokens, Some(150));
        assert_eq!(usage1.output_tokens, Some(75));
        assert_eq!(usage1.total_tokens, Some(225));
    }

    #[tokio::test]
    async fn test_usage_context_new() {
        let context = UsageContext::new();
        assert!(context.provider_id.is_none());
        assert!(context.model_id.is_none());
        assert!(context.project_id.is_none());
        assert!(context.task_id.is_none());
    }

    #[tokio::test]
    async fn test_usage_context_has_methods() {
        let mut context = UsageContext::new();
        assert!(!context.has_provider());
        assert!(!context.has_model());
        assert!(!context.has_project());
        assert!(!context.has_task());
        context.provider_id = Some("openai".to_string());
        assert!(context.has_provider());
        context.model_id = Some("gpt-4".to_string());
        assert!(context.has_model());
        context.project_id = Some("proj-1".to_string());
        assert!(context.has_project());
        context.task_id = Some("task-1".to_string());
        assert!(context.has_task());
    }

    #[tokio::test]
    async fn test_token_usage_record_new() {
        let usage = TokenUsage::with_counts(100, 50);
        let context = UsageContext::new();
        let record = TokenUsageRecord::new(usage, context, "chat".to_string());
        assert!(!record.id.is_empty());
        assert_eq!(record.operation_type, "chat");
        assert!(!record.is_retry);
        assert!(record.count_towards_totals);
    }

    #[tokio::test]
    async fn test_token_usage_record_retry() {
        let usage = TokenUsage::with_counts(100, 50);
        let context = UsageContext::new();
        let original = TokenUsageRecord::new(usage, context, "chat".to_string());
        let retry_usage = TokenUsage::with_counts(100, 50);
        let retry = TokenUsageRecord::retry_of(&original, retry_usage);
        assert!(retry.is_retry);
        assert_eq!(retry.original_request_id, Some(original.id));
        assert!(!retry.count_towards_totals);
    }

    #[tokio::test]
    async fn test_token_usage_tracker() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("token_usage.json");
        let mut tracker = TokenUsageTracker::new(&storage_path);
        // Record some usage
        let usage1 = TokenUsage::with_counts(100, 50);
        let context1 = UsageContext {
            provider_id: Some("openai".to_string()),
            provider_name: Some("OpenAI".to_string()),
            model_id: Some("gpt-4".to_string()),
            model_name: Some("GPT-4".to_string()),
            project_id: Some("proj-1".to_string()),
            project_name: Some("Project 1".to_string()),
            task_id: Some("task-1".to_string()),
            session_id: None,
            responsibility: Some("chat".to_string()),
            metadata: HashMap::new(),
        };
        let _record1 = tracker.record_usage(usage1, context1, "chat".to_string());
        // Get stats
        let stats = tracker.get_stats();
        assert_eq!(stats.total_input_tokens, 100);
        assert_eq!(stats.total_output_tokens, 50);
        assert_eq!(stats.total_tokens, 150);
        assert_eq!(stats.request_count, 1);
        // Check by provider
        let provider_stats = tracker.get_stats_by_provider("openai");
        assert_eq!(provider_stats.total_input_tokens, 100);
        assert_eq!(provider_stats.total_output_tokens, 50);
        // Check by model
        let model_stats = tracker.get_stats_by_model("gpt-4");
        assert_eq!(model_stats.total_input_tokens, 100);
        assert_eq!(model_stats.total_output_tokens, 50);
        // Check by task
        let task_stats = tracker.get_stats_by_task("task-1");
        assert_eq!(task_stats.total_input_tokens, 100);
        assert_eq!(task_stats.total_output_tokens, 50);
        // Check by project
        let project_stats = tracker.get_stats_by_project("proj-1");
        assert_eq!(project_stats.total_input_tokens, 100);
        assert_eq!(project_stats.total_output_tokens, 50);
        // Check by responsibility
        let resp_stats = tracker.get_stats_by_responsibility("chat");
        assert_eq!(resp_stats.total_input_tokens, 100);
        assert_eq!(resp_stats.total_output_tokens, 50);
    }

    #[tokio::test]
    async fn test_token_usage_tracker_persistence() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("token_usage.json");
        // Create and save
        let mut tracker = TokenUsageTracker::new(&storage_path);
        let usage = TokenUsage::with_counts(100, 50);
        let context = UsageContext::new();
        let _record = tracker.record_usage(usage, context, "chat".to_string());
        tracker.save().unwrap();
        // Load in a new tracker
        let mut tracker2 = TokenUsageTracker::new(&storage_path);
        tracker2.load().unwrap();
        let stats = tracker2.get_stats();
        assert_eq!(stats.total_input_tokens, 100);
        assert_eq!(stats.total_output_tokens, 50);
    }

    #[tokio::test]
    async fn test_token_usage_tracker_clear() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("token_usage.json");
        let mut tracker = TokenUsageTracker::new(&storage_path);
        let usage = TokenUsage::with_counts(100, 50);
        let context = UsageContext::new();
        let _record = tracker.record_usage(usage, context, "chat".to_string());
        assert_eq!(tracker.count(), 1);
        tracker.clear_all();
        assert_eq!(tracker.count(), 0);
    }

    #[test]
    fn test_token_usage_extractor_openai_format() {
        let json = r#"{
            "usage": {
                "prompt_tokens": 100,
                "completion_tokens": 50,
                "total_tokens": 150
            }
        }"#;
        let (input, output, total) = TokenUsageExtractor::extract_from_json(json);
        assert_eq!(input, Some(100));
        assert_eq!(output, Some(50));
        assert_eq!(total, Some(150));
    }

    #[test]
    fn test_token_usage_extractor_anthropic_format() {
        let json = r#"{
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50
            }
        }"#;
        let (input, output, total) = TokenUsageExtractor::extract_from_json(json);
        assert_eq!(input, Some(100));
        assert_eq!(output, Some(50));
        assert_eq!(total, None); // Total not provided
    }

    #[test]
    fn test_token_usage_extractor_google_format() {
        let json = r#"{
            "usageMetadata": {
                "promptTokenCount": 100,
                "candidatesTokenCount": 50,
                "totalTokenCount": 150
            }
        }"#;
        let (input, output, total) = TokenUsageExtractor::extract_from_json(json);
        assert_eq!(input, Some(100));
        assert_eq!(output, Some(50));
        assert_eq!(total, Some(150));
    }

    #[test]
    fn test_token_usage_extractor_no_usage() {
        let json = r#"{"content": "hello"}"#;
        let (input, output, total) = TokenUsageExtractor::extract_from_json(json);
        assert!(input.is_none());
        assert!(output.is_none());
        assert!(total.is_none());
    }

    #[tokio::test]
    async fn test_token_usage_record_effective_usage() {
        let usage = TokenUsage::with_counts(100, 50);
        let context = UsageContext::new();
        let record = TokenUsageRecord::new(usage, context, "chat".to_string());
        assert_eq!(record.effective_usage().input_tokens_or_zero(), 100);
        assert_eq!(record.effective_usage().output_tokens_or_zero(), 50);
        // Retry with count_towards_totals = false
        let retry = TokenUsageRecord::retry_of(&record, TokenUsage::with_counts(100, 50));
        assert_eq!(retry.effective_usage().input_tokens_or_zero(), 0);
        assert_eq!(retry.effective_usage().output_tokens_or_zero(), 0);
        // Retry with count_towards_totals = true
        let retry_with_count = retry.with_counting();
        assert_eq!(
            retry_with_count.effective_usage().input_tokens_or_zero(),
            100
        );
        assert_eq!(
            retry_with_count.effective_usage().output_tokens_or_zero(),
            50
        );
    }

    #[tokio::test]
    async fn test_token_usage_stats() {
        let mut stats = TokenUsageStats::new();
        let usage1 = TokenUsage::with_counts(100, 50);
        let context1 = UsageContext {
            provider_id: Some("openai".to_string()),
            model_id: Some("gpt-4".to_string()),
            project_id: Some("proj-1".to_string()),
            task_id: Some("task-1".to_string()),
            responsibility: Some("chat".to_string()),
            ..Default::default()
        };
        let record1 = TokenUsageRecord::new(usage1, context1, "chat".to_string());
        let usage2 = TokenUsage::with_counts(200, 100);
        let context2 = UsageContext {
            provider_id: Some("anthropic".to_string()),
            model_id: Some("claude-3".to_string()),
            project_id: Some("proj-1".to_string()),
            task_id: Some("task-1".to_string()),
            responsibility: Some("planning".to_string()),
            ..Default::default()
        };
        let record2 = TokenUsageRecord::new(usage2, context2, "plan".to_string());
        stats.add_record(&record1);
        stats.add_record(&record2);
        assert_eq!(stats.total_input_tokens, 300);
        assert_eq!(stats.total_output_tokens, 150);
        assert_eq!(stats.total_tokens, 450);
        assert_eq!(stats.request_count, 2);
        assert_eq!(stats.by_provider.get("openai").unwrap().input_tokens, 100);
        assert_eq!(stats.by_provider.get("openai").unwrap().output_tokens, 50);
        assert_eq!(
            stats.by_provider.get("anthropic").unwrap().input_tokens,
            200
        );
        assert_eq!(
            stats.by_provider.get("anthropic").unwrap().output_tokens,
            100
        );
        assert_eq!(stats.by_model.get("gpt-4").unwrap().input_tokens, 100);
        assert_eq!(stats.by_model.get("claude-3").unwrap().input_tokens, 200);
        assert_eq!(stats.by_project.get("proj-1").unwrap().input_tokens, 300);
        assert_eq!(stats.by_project.get("proj-1").unwrap().output_tokens, 150);
        assert_eq!(stats.by_task.get("task-1").unwrap().input_tokens, 300);
        assert_eq!(stats.by_task.get("task-1").unwrap().output_tokens, 150);
    }

    #[tokio::test]
    async fn test_token_usage_tracker_get_records() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("token_usage.json");
        let mut tracker = TokenUsageTracker::new(&storage_path);
        let usage1 = TokenUsage::with_counts(100, 50);
        let context1 = UsageContext {
            task_id: Some("task-1".to_string()),
            ..Default::default()
        };
        let _record1 = tracker.record_usage(usage1, context1, "chat".to_string());
        let usage2 = TokenUsage::with_counts(200, 100);
        let context2 = UsageContext {
            task_id: Some("task-1".to_string()),
            ..Default::default()
        };
        let _record2 = tracker.record_usage(usage2, context2, "chat".to_string());
        let usage3 = TokenUsage::with_counts(300, 150);
        let context3 = UsageContext {
            task_id: Some("task-2".to_string()),
            ..Default::default()
        };
        let _record3 = tracker.record_usage(usage3, context3, "plan".to_string());
        // Get all records
        let all_records = tracker.get_all_records();
        assert_eq!(all_records.len(), 3);
        // Get records by task
        let task1_records = tracker.get_records_by_task("task-1");
        assert_eq!(task1_records.len(), 2);
        let task2_records = tracker.get_records_by_task("task-2");
        assert_eq!(task2_records.len(), 1);
        // Get records by operation type
        let chat_records: Vec<_> = all_records
            .iter()
            .filter(|r| r.operation_type == "chat")
            .collect();
        assert_eq!(chat_records.len(), 2);
        let plan_records: Vec<_> = all_records
            .iter()
            .filter(|r| r.operation_type == "plan")
            .collect();
        assert_eq!(plan_records.len(), 1);
    }

    #[tokio::test]
    async fn test_token_usage_tracker_delete() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("token_usage.json");
        let mut tracker = TokenUsageTracker::new(&storage_path);
        let usage = TokenUsage::with_counts(100, 50);
        let context = UsageContext::new();
        let record = tracker.record_usage(usage, context, "chat".to_string());
        assert_eq!(tracker.count(), 1);
        let deleted = tracker.delete_record(&record.id);
        assert!(deleted);
        assert_eq!(tracker.count(), 0);
    }

    #[tokio::test]
    async fn test_token_usage_tracker_retry_not_double_counted() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("token_usage.json");
        let mut tracker = TokenUsageTracker::new(&storage_path);
        let usage = TokenUsage::with_counts(100, 50);
        let context = UsageContext {
            task_id: Some("task-1".to_string()),
            ..Default::default()
        };
        let original = tracker.record_usage(usage, context, "chat".to_string());
        let retry_usage = TokenUsage::with_counts(100, 50);
        let retry = tracker.record_retry(&original.id, retry_usage, false);
        assert!(retry.is_some());
        assert_eq!(tracker.count(), 2);
        let stats = tracker.get_stats();
        // Retry should not be counted
        assert_eq!(stats.total_input_tokens, 100);
        assert_eq!(stats.total_output_tokens, 50);
        assert_eq!(stats.request_count, 2); // Both records exist
    }

    #[test]
    fn test_from_cli_usage_claude_combines_cache_tokens() {
        let cli = CliTokenUsage {
            input_tokens: Some(100),
            output_tokens: Some(20),
            cache_creation_input_tokens: Some(30),
            cache_read_input_tokens: Some(40),
            model: Some("claude-sonnet-4".to_string()),
            format: "claude_code_json".to_string(),
            ..Default::default()
        };
        let usage = TokenUsage::from_cli_usage(&cli, Some(1234));
        // Claude reports cache tokens in addition to input_tokens.
        assert_eq!(usage.input_tokens, Some(170));
        assert_eq!(usage.output_tokens, Some(20));
        assert_eq!(usage.total_tokens, Some(190));
        assert!(!usage.tokens_unknown);
        assert_eq!(usage.duration_ms, Some(1234));
    }

    #[test]
    fn test_from_cli_usage_codex_does_not_add_cached_input_twice() {
        let cli = CliTokenUsage {
            input_tokens: Some(100),
            output_tokens: Some(20),
            cache_read_input_tokens: Some(60),
            format: "codex_jsonl".to_string(),
            ..Default::default()
        };
        let usage = TokenUsage::from_cli_usage(&cli, None);
        // Codex cached input is a subset of input_tokens, so it must not be added.
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, Some(20));
        assert_eq!(usage.total_tokens, Some(120));
        assert!(!usage.tokens_unknown);
    }

    #[test]
    fn test_from_cli_usage_partial_stays_partial() {
        let cli = CliTokenUsage {
            input_tokens: Some(100),
            format: "claude_code_json".to_string(),
            ..Default::default()
        };
        let usage = TokenUsage::from_cli_usage(&cli, None);
        assert_eq!(usage.input_tokens, Some(100));
        assert_eq!(usage.output_tokens, None);
        // A missing half is never guessed.
        assert_eq!(usage.total_tokens, None);
        assert!(!usage.tokens_unknown);
    }

    #[test]
    fn test_from_cli_usage_missing_counts_is_unknown_not_estimated() {
        let cli = CliTokenUsage {
            format: "claude_code_json".to_string(),
            ..Default::default()
        };
        let usage = TokenUsage::from_cli_usage(&cli, Some(10));
        assert!(usage.tokens_unknown);
        assert_eq!(usage.input_tokens, None);
        assert_eq!(usage.output_tokens, None);
        assert_eq!(usage.total_tokens, None);
    }

    #[test]
    fn test_record_usage_deduplicated_is_idempotent_across_reload() {
        let temp_dir = tempdir().unwrap();
        let storage_path = temp_dir.path().join("token_usage.json");
        let mut tracker = TokenUsageTracker::new(&storage_path);
        let key = "cli:task-1:task-iteration:1:2026-01-01T00:00:00Z".to_string();

        let first = tracker.record_usage_deduplicated(
            TokenUsage::with_counts(100, 50),
            UsageContext {
                task_id: Some("task-1".to_string()),
                ..Default::default()
            },
            "execute".to_string(),
            Some(key.clone()),
        );
        assert!(first.is_some());

        let duplicate = tracker.record_usage_deduplicated(
            TokenUsage::with_counts(999, 999),
            UsageContext {
                task_id: Some("task-1".to_string()),
                ..Default::default()
            },
            "execute".to_string(),
            Some(key.clone()),
        );
        assert!(duplicate.is_none());
        assert_eq!(tracker.count(), 1);
        tracker.save().unwrap();

        let mut reloaded = TokenUsageTracker::new(&storage_path);
        reloaded.load().unwrap();
        assert_eq!(reloaded.count(), 1);
        assert!(reloaded.has_dedup_key(&key));
        let stats = reloaded.get_stats();
        assert_eq!(stats.total_input_tokens, 100);
        assert_eq!(stats.total_output_tokens, 50);
        assert_eq!(stats.request_count, 1);

        // A replayed result after a reload is still not counted twice.
        let replay = reloaded.record_usage_deduplicated(
            TokenUsage::with_counts(100, 50),
            UsageContext::default(),
            "execute".to_string(),
            Some(key),
        );
        assert!(replay.is_none());
        assert_eq!(reloaded.count(), 1);
    }
}
