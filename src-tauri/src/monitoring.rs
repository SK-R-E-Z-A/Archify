//! Batch 3 structured execution monitoring.
//!
//! This module owns event **delivery, ordering, deduplication and history**.
//! It deliberately does *not* define the shared execution-event schema:
//! `models::ExecutionEvent` is owned by Claude. The hub accepts any
//! [`Serialize`] payload, so once the shared type exists callers pass it
//! directly:
//!
//! ```ignore
//! let outcome = hub.emit(&context, &ExecutionEvent { /* ... */ });
//! ```
//!
//! Design rules enforced here:
//!
//! * Events are accepted only with a non-empty task identifier. Iteration
//!   numbers, when supplied, are 1-based.
//! * Sequence numbers are assigned and the payload is handed to the consumer
//!   while holding one lock, so delivery order always matches sequence order.
//! * Identical payloads inside the deduplication window are suppressed and are
//!   not written to history. A retryable delivery failure is *not* remembered
//!   so the caller can retry without being treated as a duplicate.
//! * A missing consumer is a normal state, never an error and never a panic.
//!   The event is still recorded in history with `NoConsumer` status.
//! * The module never synthesises lifecycle events. Every event is supplied by
//!   the orchestrator from an observed transition; nothing here fabricates
//!   progress.
//!
//! Integration into `orchestrator.rs` is intentionally left to Claude; see the
//! hand-off notes in the Batch 3 report.

use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};
use tauri::Emitter;

/// Tauri event name subscribed to by the frontend.
pub const ORCHESTRATION_EVENT: &str = "orchestration_event";

/// Default number of recent events retained for history queries.
pub const DEFAULT_HISTORY_CAPACITY: usize = 512;

/// Default number of recent payload fingerprints checked for duplicates.
pub const DEFAULT_DEDUP_WINDOW: usize = 32;

/// Errors that prevent an event from being accepted.
///
/// Delivery failures are reported separately through [`EmitOutcome::Failed`];
/// they are not monitoring errors because the event itself is valid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MonitoringError {
    /// The event carried no usable task identifier.
    MissingTaskId,
    /// Iteration numbers are 1-based; zero is not a valid iteration.
    InvalidIteration(u32),
    /// A task monitor only accepts strictly increasing iteration numbers.
    NonMonotonicIteration { current: u32, requested: u32 },
    /// The monitored task has already been marked finished.
    TaskFinished,
    /// The event payload could not be serialised.
    Serialization(String),
    /// The internal monitoring lock was poisoned by a panicking caller.
    LockPoisoned,
}

impl fmt::Display for MonitoringError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MonitoringError::MissingTaskId => {
                write!(f, "monitoring event requires a non-empty task id")
            }
            MonitoringError::InvalidIteration(value) => {
                write!(f, "monitoring iteration must be 1-based, got {value}")
            }
            MonitoringError::NonMonotonicIteration { current, requested } => write!(
                f,
                "monitoring iteration must increase: current {current}, requested {requested}"
            ),
            MonitoringError::TaskFinished => {
                write!(
                    f,
                    "cannot emit monitoring events after the task is finished"
                )
            }
            MonitoringError::Serialization(detail) => {
                write!(f, "monitoring event could not be serialized: {detail}")
            }
            MonitoringError::LockPoisoned => {
                write!(f, "monitoring state lock is poisoned")
            }
        }
    }
}

impl std::error::Error for MonitoringError {}

/// Identifies the task (and optionally iteration) an event belongs to.
///
/// The context is used for history queries, deduplication scoping and status
/// reporting. The emitted payload itself is the caller's event, unmodified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventContext {
    pub task_id: String,
    pub iteration: Option<u32>,
}

impl EventContext {
    /// Creates a task-level context.
    pub fn new(task_id: impl Into<String>) -> Result<Self, MonitoringError> {
        Self::build(task_id.into(), None)
    }

    /// Creates a task-and-iteration context. Iterations are 1-based.
    pub fn with_iteration(
        task_id: impl Into<String>,
        iteration: u32,
    ) -> Result<Self, MonitoringError> {
        Self::build(task_id.into(), Some(iteration))
    }

    fn build(task_id: String, iteration: Option<u32>) -> Result<Self, MonitoringError> {
        if task_id.trim().is_empty() {
            return Err(MonitoringError::MissingTaskId);
        }
        if matches!(iteration, Some(0)) {
            return Err(MonitoringError::InvalidIteration(0));
        }
        Ok(Self { task_id, iteration })
    }

    fn validate(&self) -> Result<(), MonitoringError> {
        if self.task_id.trim().is_empty() {
            return Err(MonitoringError::MissingTaskId);
        }
        if matches!(self.iteration, Some(0)) {
            return Err(MonitoringError::InvalidIteration(0));
        }
        Ok(())
    }
}

/// How a valid event was handled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmitOutcome {
    /// The consumer accepted the event.
    Delivered { sequence: u64 },
    /// No consumer is attached (for example, a headless test or closed app).
    NoConsumer { sequence: u64 },
    /// The event duplicated a recent payload and was suppressed.
    Duplicate { first_sequence: u64 },
    /// The consumer rejected the event. The attempt is recorded but not
    /// treated as delivered, and the payload is not added to the dedup window.
    Failed { sequence: u64, error: String },
    /// The event never reached the consumer because it was invalid.
    Rejected { error: MonitoringError },
}

/// Delivery state stored alongside a history record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DeliveryStatus {
    Delivered,
    NoConsumer,
    Failed,
}

/// A bounded, ordered record of an event accepted by the monitoring hub.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RecordedEvent {
    /// Monotonic sequence assigned at acceptance time. Starts at zero.
    pub sequence: u64,
    pub event_name: String,
    pub task_id: String,
    pub iteration: Option<u32>,
    pub payload: Value,
    pub status: DeliveryStatus,
    /// Consumer error, present only when `status == DeliveryStatus::Failed`.
    pub error: Option<String>,
}

/// Transport abstraction for monitoring events.
///
/// The production implementation is [`TauriEventSink`]. Tests and headless
/// runs use their own implementation, which keeps the hub independent of the
/// Tauri runtime.
pub trait EventSink: Send + Sync {
    fn emit(&self, event_name: &str, payload: Value) -> Result<(), String>;
}

/// Emits monitoring events through the Tauri application handle.
pub struct TauriEventSink {
    handle: tauri::AppHandle,
}

impl TauriEventSink {
    pub fn new(handle: tauri::AppHandle) -> Self {
        Self { handle }
    }
}

impl EventSink for TauriEventSink {
    fn emit(&self, event_name: &str, payload: Value) -> Result<(), String> {
        self.handle
            .emit(event_name, payload)
            .map_err(|error| error.to_string())
    }
}

struct HubState {
    sink: Option<Arc<dyn EventSink>>,
    history: VecDeque<RecordedEvent>,
    history_capacity: usize,
    dedup_window: usize,
    /// Fingerprint plus the sequence of the first accepted occurrence.
    recent: VecDeque<(String, u64)>,
    next_sequence: u64,
    /// Optional file path for durable event persistence (JSONL).
    persist_path: Option<std::path::PathBuf>,
}

/// Thread-safe monitoring hub.
///
/// Cloning a hub shares the same state, so an orchestrator can keep one clone
/// for loop-local use and another for read-only history queries.
#[derive(Clone)]
pub struct MonitoringHub {
    state: Arc<Mutex<HubState>>,
}

impl MonitoringHub {
    /// Creates a hub with an optional consumer and default limits.
    pub fn new(sink: Option<Arc<dyn EventSink>>) -> Self {
        Self::with_limits(sink, DEFAULT_HISTORY_CAPACITY, DEFAULT_DEDUP_WINDOW)
    }

    /// Creates a hub with no consumer. Useful for headless runs and tests.
    pub fn headless() -> Self {
        Self::new(None)
    }

    /// Creates a hub from an optional Tauri handle. `None` is safe and yields a
    /// headless hub rather than an error.
    pub fn from_app_handle(handle: Option<tauri::AppHandle>) -> Self {
        let sink = handle.map(|handle| Arc::new(TauriEventSink::new(handle)) as Arc<dyn EventSink>);
        Self::new(sink)
    }

    /// Creates a hub with explicit history and deduplication limits.
    ///
    /// `history_capacity` is clamped to at least one so history never silently
    /// discards every event. A `dedup_window` of zero disables deduplication.
    pub fn with_limits(
        sink: Option<Arc<dyn EventSink>>,
        history_capacity: usize,
        dedup_window: usize,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(HubState {
                sink,
                history: VecDeque::new(),
                history_capacity: history_capacity.max(1),
                dedup_window,
                recent: VecDeque::new(),
                next_sequence: 0,
                persist_path: None,
            })),
        }
    }

    /// Replaces the consumer. Existing history is retained.
    pub fn set_sink(&self, sink: Option<Arc<dyn EventSink>>) -> Result<(), MonitoringError> {
        let mut state = self.lock()?;
        state.sink = sink;
        Ok(())
    }

    /// Enables (or disables) durable persistence of accepted events to a
    /// JSONL file. When enabled, existing events are loaded from the file and
    /// the in-memory history is rehydrated. Subsequent emits append to the
    /// file. Bounded retention is enforced in memory and via file rotation.
    pub fn set_persist_path(
        &self,
        path: Option<std::path::PathBuf>,
    ) -> Result<(), MonitoringError> {
        let mut state = self.lock()?;
        state.persist_path = path.clone();
        state.history.clear();
        state.recent.clear();
        state.next_sequence = 0;
        if let Some(p) = path {
            if let Some(parent) = p.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if p.exists() {
                if let Ok(content) = std::fs::read_to_string(&p) {
                    let mut max_seq: u64 = 0;
                    for line in content.lines() {
                        let line = line.trim();
                        if line.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<RecordedEvent>(line) {
                            Ok(ev) => {
                                if ev.sequence >= max_seq {
                                    max_seq = ev.sequence + 1;
                                }
                                state.history.push_back(ev);
                                while state.history.len() > state.history_capacity {
                                    state.history.pop_front();
                                }
                            }
                            Err(_) => {
                                // Skip malformed/partial lines from interrupted writes.
                            }
                        }
                    }
                    state.next_sequence = max_seq;
                }
            }
        }
        Ok(())
    }

    /// Returns whether a consumer is currently attached.
    pub fn has_consumer(&self) -> bool {
        self.state
            .lock()
            .map(|state| state.sink.is_some())
            .unwrap_or(false)
    }

    /// Accepts, optionally delivers, and records one event.
    ///
    /// Serialisation, context validation and deduplication happen before any
    /// sequence number is consumed. Delivery happens under the state lock so
    /// two threads can never publish events out of sequence order.
    pub fn emit<E>(&self, context: &EventContext, event: &E) -> EmitOutcome
    where
        E: Serialize + ?Sized,
    {
        if let Err(error) = context.validate() {
            return EmitOutcome::Rejected { error };
        }

        let payload = match serde_json::to_value(event) {
            Ok(payload) => payload,
            Err(error) => {
                return EmitOutcome::Rejected {
                    error: MonitoringError::Serialization(error.to_string()),
                };
            }
        };

        // Scope duplicate detection to the task and iteration so identical
        // phase payloads from different iterations are never conflated.
        let fingerprint = format!(
            "{}\u{1f}{:?}\u{1f}{}",
            context.task_id, context.iteration, payload
        );
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                return EmitOutcome::Rejected {
                    error: MonitoringError::LockPoisoned,
                };
            }
        };

        if let Some((_, first_sequence)) = state
            .recent
            .iter()
            .find(|(recent, _)| recent == &fingerprint)
        {
            return EmitOutcome::Duplicate {
                first_sequence: *first_sequence,
            };
        }

        let sequence = state.next_sequence;
        state.next_sequence += 1;

        let (status, error) = match state.sink.as_ref() {
            Some(sink) => match sink.emit(ORCHESTRATION_EVENT, payload.clone()) {
                Ok(()) => (DeliveryStatus::Delivered, None),
                Err(error) => (DeliveryStatus::Failed, Some(error)),
            },
            None => (DeliveryStatus::NoConsumer, None),
        };

        state.history.push_back(RecordedEvent {
            sequence,
            event_name: ORCHESTRATION_EVENT.to_string(),
            task_id: context.task_id.clone(),
            iteration: context.iteration,
            payload,
            status,
            error: error.clone(),
        });
        let capacity = state.history_capacity;
        while state.history.len() > capacity {
            state.history.pop_front();
        }

        // Durable persistence: append the accepted event to the JSONL file.
        if let Some(path) = state.persist_path.clone() {
            if let Some(recorded) = state.history.back().cloned() {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                // Rotate when the file grows beyond the bounded size.
                const MAX_PERSIST_BYTES: u64 = 4 * 1024 * 1024;
                if let Ok(meta) = std::fs::metadata(&path) {
                    if meta.len() >= MAX_PERSIST_BYTES {
                        let backup = path.with_extension("jsonl.1");
                        let _ = std::fs::remove_file(&backup);
                        let _ = std::fs::rename(&path, &backup);
                    }
                }
                if let Ok(mut file) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                {
                    use std::io::Write;
                    if let Ok(line) = serde_json::to_string(&recorded) {
                        let _ = writeln!(file, "{}", line);
                        let _ = file.flush();
                        let _ = file.sync_all();
                    }
                }
            }
        }

        // Remember successful and consumer-less events. A failed delivery is
        // retryable and must not be mistaken for a duplicate on the next call.
        if status != DeliveryStatus::Failed && state.dedup_window > 0 {
            state.recent.push_back((fingerprint, sequence));
            while state.recent.len() > state.dedup_window {
                state.recent.pop_front();
            }
        }

        match status {
            DeliveryStatus::Delivered => EmitOutcome::Delivered { sequence },
            DeliveryStatus::NoConsumer => EmitOutcome::NoConsumer { sequence },
            DeliveryStatus::Failed => EmitOutcome::Failed {
                sequence,
                error: error.unwrap_or_else(|| "consumer rejected event".to_string()),
            },
        }
    }

    /// Convenience for callers that already hold a task id and iteration.
    ///
    /// Invalid identifiers are rejected exactly like [`MonitoringHub::emit`].
    pub fn emit_task_event<E>(
        &self,
        task_id: impl Into<String>,
        iteration: Option<u32>,
        event: &E,
    ) -> EmitOutcome
    where
        E: Serialize + ?Sized,
    {
        match EventContext::build(task_id.into(), iteration) {
            Ok(context) => self.emit(&context, event),
            Err(error) => EmitOutcome::Rejected { error },
        }
    }

    /// Returns a snapshot of all retained history in sequence order.
    pub fn history(&self) -> Vec<RecordedEvent> {
        self.state
            .lock()
            .map(|state| state.history.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Returns retained history for one task.
    pub fn history_for_task(&self, task_id: &str) -> Vec<RecordedEvent> {
        self.history()
            .into_iter()
            .filter(|event| event.task_id == task_id)
            .collect()
    }

    /// Returns retained history for one task iteration.
    pub fn history_for_iteration(&self, task_id: &str, iteration: u32) -> Vec<RecordedEvent> {
        self.history()
            .into_iter()
            .filter(|event| event.task_id == task_id && event.iteration == Some(iteration))
            .collect()
    }

    /// Returns retained events with a sequence strictly greater than `sequence`.
    pub fn history_since(&self, sequence: u64) -> Vec<RecordedEvent> {
        self.history()
            .into_iter()
            .filter(|event| event.sequence > sequence)
            .collect()
    }

    /// Sequence that will be assigned to the next accepted event.
    pub fn next_sequence(&self) -> u64 {
        self.state
            .lock()
            .map(|state| state.next_sequence)
            .unwrap_or(0)
    }

    /// Sequence of the most recently accepted event, if any.
    pub fn last_sequence(&self) -> Option<u64> {
        self.state
            .lock()
            .ok()
            .and_then(|state| state.next_sequence.checked_sub(1))
    }

    /// Number of retained history records.
    pub fn history_len(&self) -> usize {
        self.state
            .lock()
            .map(|state| state.history.len())
            .unwrap_or(0)
    }

    /// Clears retained history without resetting the sequence counter, so
    /// consumers that track sequences never see order go backwards.
    pub fn clear_history(&self) -> usize {
        match self.state.lock() {
            Ok(mut state) => {
                let cleared = state.history.len();
                state.history.clear();
                cleared
            }
            Err(_) => 0,
        }
    }

    /// Aggregated delivery counters for one task.
    pub fn snapshot_for_task(&self, task_id: &str) -> MonitoringSnapshot {
        let events = self.history_for_task(task_id);
        MonitoringSnapshot {
            task_id: task_id.to_string(),
            event_count: events.len(),
            last_sequence: events.iter().map(|event| event.sequence).max(),
            delivered: events
                .iter()
                .filter(|event| event.status == DeliveryStatus::Delivered)
                .count(),
            no_consumer: events
                .iter()
                .filter(|event| event.status == DeliveryStatus::NoConsumer)
                .count(),
            failed: events
                .iter()
                .filter(|event| event.status == DeliveryStatus::Failed)
                .count(),
        }
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, HubState>, MonitoringError> {
        self.state.lock().map_err(|_| MonitoringError::LockPoisoned)
    }
}

/// Read-only delivery totals for a task's retained history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitoringSnapshot {
    pub task_id: String,
    pub event_count: usize,
    pub last_sequence: Option<u64>,
    pub delivered: usize,
    pub no_consumer: usize,
    pub failed: usize,
}

/// Tracks task lifecycle and iteration identity for a single monitored task.
///
/// The monitor does not emit lifecycle events on its own. `begin_iteration`,
/// `end_iteration` and `finish` only update identifiers; the caller must still
/// emit an event when an actual transition is observed.
pub struct TaskMonitor {
    hub: MonitoringHub,
    task_id: String,
    iteration: Option<u32>,
    finished: bool,
}

impl TaskMonitor {
    pub fn new(hub: MonitoringHub, task_id: impl Into<String>) -> Result<Self, MonitoringError> {
        let task_id = task_id.into();
        if task_id.trim().is_empty() {
            return Err(MonitoringError::MissingTaskId);
        }
        Ok(Self {
            hub,
            task_id,
            iteration: None,
            finished: false,
        })
    }

    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    pub fn iteration(&self) -> Option<u32> {
        self.iteration
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn hub(&self) -> &MonitoringHub {
        &self.hub
    }

    /// Enters a new iteration. Iterations are 1-based and strictly increasing.
    pub fn begin_iteration(&mut self, iteration: u32) -> Result<EventContext, MonitoringError> {
        if self.finished {
            return Err(MonitoringError::TaskFinished);
        }
        if iteration == 0 {
            return Err(MonitoringError::InvalidIteration(0));
        }
        if let Some(current) = self.iteration {
            if iteration <= current {
                return Err(MonitoringError::NonMonotonicIteration {
                    current,
                    requested: iteration,
                });
            }
        }
        self.iteration = Some(iteration);
        Ok(self.context())
    }

    /// Leaves the current iteration, returning the iteration that was closed.
    pub fn end_iteration(&mut self) -> Option<u32> {
        self.iteration.take()
    }

    /// Marks the task finished. No further events or iterations are accepted.
    pub fn finish(&mut self) {
        self.finished = true;
    }

    /// Context for the current task state. Always valid because the task id was
    /// validated at construction and iterations are validated before storage.
    pub fn context(&self) -> EventContext {
        EventContext {
            task_id: self.task_id.clone(),
            iteration: self.iteration,
        }
    }

    /// Emits an event with the current task and iteration identifiers.
    pub fn emit<E>(&self, event: &E) -> EmitOutcome
    where
        E: Serialize + ?Sized,
    {
        if self.finished {
            return EmitOutcome::Rejected {
                error: MonitoringError::TaskFinished,
            };
        }
        self.hub.emit(&self.context(), event)
    }
}
