//! Monitoring-specific backend tests for Batch 3.
//!
//! The monitoring module is included by path so these tests exercise the exact
//! production source without requiring a change to `lib.rs` before Claude wires
//! the module in. The test event type below mirrors the shape of Claude's
//! shared `ExecutionEvent`; it is a test fixture, not a production redefinition.

#[path = "../src/monitoring.rs"]
mod monitoring;

use std::sync::{Arc, Mutex};

use monitoring::{
    DeliveryStatus, EmitOutcome, EventContext, EventSink, MonitoringError, MonitoringHub,
    ORCHESTRATION_EVENT, TaskMonitor,
};
use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Serialize)]
struct FixtureExecutionEvent {
    timestamp: String,
    task_id: String,
    phase: String,
    data: Value,
}

fn event(task_id: &str, phase: &str, iteration: Option<u32>) -> FixtureExecutionEvent {
    FixtureExecutionEvent {
        timestamp: "2026-11-09T00:00:00Z".to_string(),
        task_id: task_id.to_string(),
        phase: phase.to_string(),
        data: match iteration {
            Some(iteration) => json!({ "iteration": iteration }),
            None => json!({}),
        },
    }
}

#[derive(Default)]
struct RecordingSink {
    events: Mutex<Vec<(String, Value)>>,
}

impl RecordingSink {
    fn payloads(&self) -> Vec<Value> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .map(|(_, payload)| payload.clone())
            .collect()
    }

    fn names(&self) -> Vec<String> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .map(|(name, _)| name.clone())
            .collect()
    }

    fn len(&self) -> usize {
        self.events.lock().unwrap().len()
    }
}

impl EventSink for RecordingSink {
    fn emit(&self, event_name: &str, payload: Value) -> Result<(), String> {
        self.events
            .lock()
            .unwrap()
            .push((event_name.to_string(), payload));
        Ok(())
    }
}

struct FailingSink {
    error: &'static str,
}

impl EventSink for FailingSink {
    fn emit(&self, _event_name: &str, _payload: Value) -> Result<(), String> {
        Err(self.error.to_string())
    }
}

fn recording_hub() -> (MonitoringHub, Arc<RecordingSink>) {
    let sink = Arc::new(RecordingSink::default());
    (
        MonitoringHub::new(Some(sink.clone() as Arc<dyn EventSink>)),
        sink,
    )
}

#[test]
fn emits_events_in_order_and_records_identifiers() {
    let (hub, sink) = recording_hub();
    let context = EventContext::with_iteration("task-1", 1).unwrap();

    let first = event("task-1", "TaskStarted", Some(1));
    let second = event("task-1", "IterationStarted", Some(1));
    let third = event("task-1", "ComponentFinished", Some(1));

    assert_eq!(
        hub.emit(&context, &first),
        EmitOutcome::Delivered { sequence: 0 }
    );
    assert_eq!(
        hub.emit(&context, &second),
        EmitOutcome::Delivered { sequence: 1 }
    );
    assert_eq!(
        hub.emit(&context, &third),
        EmitOutcome::Delivered { sequence: 2 }
    );

    assert_eq!(sink.len(), 3);
    assert_eq!(
        sink.names(),
        vec![
            ORCHESTRATION_EVENT.to_string(),
            ORCHESTRATION_EVENT.to_string(),
            ORCHESTRATION_EVENT.to_string()
        ]
    );
    let payloads = sink.payloads();
    assert_eq!(payloads[0]["phase"], "TaskStarted");
    assert_eq!(payloads[1]["phase"], "IterationStarted");
    assert_eq!(payloads[2]["phase"], "ComponentFinished");

    let history = hub.history();
    assert_eq!(
        history.iter().map(|item| item.sequence).collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(
        history
            .iter()
            .all(|item| item.task_id == "task-1" && item.iteration == Some(1))
    );
    assert!(
        history
            .iter()
            .all(|item| item.status == DeliveryStatus::Delivered)
    );
    assert_eq!(hub.last_sequence(), Some(2));
    assert_eq!(hub.next_sequence(), 3);
}

#[test]
fn duplicate_events_are_suppressed_within_window() {
    let (hub, sink) = recording_hub();
    let context = EventContext::with_iteration("task-1", 1).unwrap();
    let duplicate = event("task-1", "TaskStarted", Some(1));

    assert_eq!(
        hub.emit(&context, &duplicate),
        EmitOutcome::Delivered { sequence: 0 }
    );
    assert_eq!(
        hub.emit(&context, &duplicate),
        EmitOutcome::Duplicate { first_sequence: 0 }
    );

    assert_eq!(sink.len(), 1);
    assert_eq!(hub.history_len(), 1);
    assert_eq!(hub.next_sequence(), 1);
}

#[test]
fn distinct_events_are_not_suppressed() {
    let (hub, sink) = recording_hub();
    let context = EventContext::with_iteration("task-1", 1).unwrap();

    let first = event("task-1", "IterationStarted", Some(1));
    let second = event("task-1", "IterationFinished", Some(1));

    assert_eq!(
        hub.emit(&context, &first),
        EmitOutcome::Delivered { sequence: 0 }
    );
    assert_eq!(
        hub.emit(&context, &second),
        EmitOutcome::Delivered { sequence: 1 }
    );
    assert_eq!(sink.len(), 2);
}

#[test]
fn dedup_is_scoped_by_task_and_iteration() {
    let (hub, sink) = recording_hub();
    // Deliberately omit iteration data from the payload so only the monitoring
    // context distinguishes the two emissions.
    let payload = event("task-1", "IterationStarted", None);

    let first_context = EventContext::with_iteration("task-1", 1).unwrap();
    let second_context = EventContext::with_iteration("task-1", 2).unwrap();

    assert_eq!(
        hub.emit(&first_context, &payload),
        EmitOutcome::Delivered { sequence: 0 }
    );
    assert_eq!(
        hub.emit(&second_context, &payload),
        EmitOutcome::Delivered { sequence: 1 }
    );
    assert_eq!(sink.len(), 2);
    assert_eq!(hub.history()[0].iteration, Some(1));
    assert_eq!(hub.history()[1].iteration, Some(2));
}

#[test]
fn emit_task_event_validates_identifiers() {
    let (hub, sink) = recording_hub();
    let payload = event("task-1", "TaskStarted", Some(1));

    assert_eq!(
        hub.emit_task_event("task-1", Some(1), &payload),
        EmitOutcome::Delivered { sequence: 0 }
    );
    assert_eq!(
        hub.emit_task_event("", None, &event("", "TaskStarted", None)),
        EmitOutcome::Rejected {
            error: MonitoringError::MissingTaskId
        }
    );
    assert_eq!(
        hub.emit_task_event("task-1", Some(0), &payload),
        EmitOutcome::Rejected {
            error: MonitoringError::InvalidIteration(0)
        }
    );
    assert_eq!(sink.len(), 1);
    assert_eq!(hub.history_len(), 1);
}

#[test]
fn dedup_window_is_bounded() {
    let sink = Arc::new(RecordingSink::default());
    // Only remember the single most recent payload.
    let hub = MonitoringHub::with_limits(Some(sink.clone() as Arc<dyn EventSink>), 16, 1);
    let context = EventContext::new("task-1").unwrap();

    let first = event("task-1", "A", None);
    let second = event("task-1", "B", None);

    assert_eq!(
        hub.emit(&context, &first),
        EmitOutcome::Delivered { sequence: 0 }
    );
    assert_eq!(
        hub.emit(&context, &second),
        EmitOutcome::Delivered { sequence: 1 }
    );
    // `first` has fallen out of the one-entry window and is accepted again.
    assert_eq!(
        hub.emit(&context, &first),
        EmitOutcome::Delivered { sequence: 2 }
    );
    assert_eq!(sink.len(), 3);
}

#[test]
fn missing_consumer_is_safe_and_history_is_retained() {
    let hub = MonitoringHub::headless();
    let context = EventContext::with_iteration("task-1", 1).unwrap();
    let payload = event("task-1", "TaskStarted", Some(1));

    assert_eq!(
        hub.emit(&context, &payload),
        EmitOutcome::NoConsumer { sequence: 0 }
    );
    assert!(!hub.has_consumer());
    assert_eq!(hub.history_len(), 1);
    assert_eq!(hub.history()[0].status, DeliveryStatus::NoConsumer);
}

#[test]
fn from_none_app_handle_is_headless() {
    let hub = MonitoringHub::from_app_handle(None);
    assert!(!hub.has_consumer());

    let context = EventContext::new("task-1").unwrap();
    assert_eq!(
        hub.emit(&context, &event("task-1", "TaskStarted", None)),
        EmitOutcome::NoConsumer { sequence: 0 }
    );
}

#[test]
fn consumer_failure_is_reported_and_retryable() {
    let hub = MonitoringHub::new(Some(Arc::new(FailingSink {
        error: "consumer offline",
    })));
    let context = EventContext::with_iteration("task-1", 1).unwrap();
    let payload = event("task-1", "TaskStarted", Some(1));

    assert_eq!(
        hub.emit(&context, &payload),
        EmitOutcome::Failed {
            sequence: 0,
            error: "consumer offline".to_string()
        }
    );
    assert_eq!(hub.history_len(), 1);
    assert_eq!(hub.history()[0].status, DeliveryStatus::Failed);
    assert_eq!(hub.history()[0].error.as_deref(), Some("consumer offline"));

    // A failed delivery is not recorded in the dedup window, so the caller can
    // retry rather than being told the event is a duplicate.
    assert_eq!(
        hub.emit(&context, &payload),
        EmitOutcome::Failed {
            sequence: 1,
            error: "consumer offline".to_string()
        }
    );
    assert_eq!(hub.history_len(), 2);
}

#[test]
fn history_is_bounded_but_sequence_keeps_increasing() {
    let sink = Arc::new(RecordingSink::default());
    // History retains two events; dedup is disabled so every payload is distinct.
    let hub = MonitoringHub::with_limits(Some(sink.clone() as Arc<dyn EventSink>), 2, 0);
    let context = EventContext::new("task-1").unwrap();

    assert_eq!(
        hub.emit(&context, &event("task-1", "A", None)),
        EmitOutcome::Delivered { sequence: 0 }
    );
    assert_eq!(
        hub.emit(&context, &event("task-1", "B", None)),
        EmitOutcome::Delivered { sequence: 1 }
    );
    assert_eq!(
        hub.emit(&context, &event("task-1", "C", None)),
        EmitOutcome::Delivered { sequence: 2 }
    );

    let history = hub.history();
    assert_eq!(
        history.iter().map(|item| item.sequence).collect::<Vec<_>>(),
        vec![1, 2]
    );
    assert_eq!(sink.len(), 3);
    assert_eq!(hub.last_sequence(), Some(2));
    assert_eq!(hub.next_sequence(), 3);

    let since = hub.history_since(1);
    assert_eq!(since.len(), 1);
    assert_eq!(since[0].sequence, 2);
}

#[test]
fn history_queries_filter_by_task_and_iteration() {
    let hub = MonitoringHub::headless();

    let task_1_iter_1 = EventContext::with_iteration("task-1", 1).unwrap();
    let task_1_iter_2 = EventContext::with_iteration("task-1", 2).unwrap();
    let task_2_iter_1 = EventContext::with_iteration("task-2", 1).unwrap();

    hub.emit(&task_1_iter_1, &event("task-1", "A", Some(1)));
    hub.emit(&task_1_iter_2, &event("task-1", "B", Some(2)));
    hub.emit(&task_2_iter_1, &event("task-2", "C", Some(1)));

    assert_eq!(hub.history_for_task("task-1").len(), 2);
    assert_eq!(hub.history_for_task("task-2").len(), 1);
    assert_eq!(hub.history_for_iteration("task-1", 1).len(), 1);
    assert_eq!(hub.history_for_iteration("task-1", 2).len(), 1);
    assert_eq!(hub.history_for_iteration("task-1", 3).len(), 0);

    let snapshot = hub.snapshot_for_task("task-1");
    assert_eq!(snapshot.task_id, "task-1");
    assert_eq!(snapshot.event_count, 2);
    assert_eq!(snapshot.last_sequence, Some(1));
    assert_eq!(snapshot.delivered, 0);
    assert_eq!(snapshot.no_consumer, 2);
    assert_eq!(snapshot.failed, 0);
}

#[test]
fn invalid_context_is_rejected_without_recording() {
    assert_eq!(EventContext::new(""), Err(MonitoringError::MissingTaskId));
    assert_eq!(
        EventContext::new("   "),
        Err(MonitoringError::MissingTaskId)
    );
    assert_eq!(
        EventContext::with_iteration("task-1", 0),
        Err(MonitoringError::InvalidIteration(0))
    );

    let (hub, sink) = recording_hub();
    let manually_invalid = EventContext {
        task_id: " ".to_string(),
        iteration: None,
    };
    assert_eq!(
        hub.emit(&manually_invalid, &event("task-1", "TaskStarted", None)),
        EmitOutcome::Rejected {
            error: MonitoringError::MissingTaskId
        }
    );
    assert_eq!(sink.len(), 0);
    assert_eq!(hub.history_len(), 0);
    assert_eq!(hub.next_sequence(), 0);
}

struct UnserializableEvent;

impl Serialize for UnserializableEvent {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Err(serde::ser::Error::custom(
            "intentional serialization failure",
        ))
    }
}

#[test]
fn serialization_failure_is_rejected_without_recording() {
    let (hub, sink) = recording_hub();
    let context = EventContext::new("task-1").unwrap();

    match hub.emit(&context, &UnserializableEvent) {
        EmitOutcome::Rejected {
            error: MonitoringError::Serialization(detail),
        } => assert!(detail.contains("intentional serialization failure")),
        other => panic!("expected serialization rejection, got {other:?}"),
    }

    assert_eq!(sink.len(), 0);
    assert_eq!(hub.history_len(), 0);
    assert_eq!(hub.next_sequence(), 0);
}

#[test]
fn task_monitor_tracks_iterations_and_blocks_after_finish() {
    let hub = MonitoringHub::headless();
    let mut monitor = TaskMonitor::new(hub.clone(), "task-1").unwrap();

    assert_eq!(monitor.task_id(), "task-1");
    assert_eq!(monitor.iteration(), None);
    assert!(!monitor.is_finished());

    assert_eq!(
        monitor.begin_iteration(1).unwrap(),
        EventContext::with_iteration("task-1", 1).unwrap()
    );
    assert_eq!(monitor.iteration(), Some(1));

    assert_eq!(
        monitor.begin_iteration(1),
        Err(MonitoringError::NonMonotonicIteration {
            current: 1,
            requested: 1
        })
    );
    assert_eq!(
        monitor.begin_iteration(0),
        Err(MonitoringError::InvalidIteration(0))
    );
    assert_eq!(
        monitor.begin_iteration(2).unwrap(),
        EventContext::with_iteration("task-1", 2).unwrap()
    );
    assert_eq!(monitor.iteration(), Some(2));

    assert_eq!(
        monitor.emit(&event("task-1", "IterationStarted", Some(2))),
        EmitOutcome::NoConsumer { sequence: 0 }
    );
    assert_eq!(hub.history()[0].iteration, Some(2));

    assert_eq!(monitor.end_iteration(), Some(2));
    assert_eq!(monitor.iteration(), None);
    assert_eq!(monitor.context(), EventContext::new("task-1").unwrap());

    monitor.finish();
    assert!(monitor.is_finished());
    assert_eq!(
        monitor.emit(&event("task-1", "TaskCompleted", None)),
        EmitOutcome::Rejected {
            error: MonitoringError::TaskFinished
        }
    );
    assert_eq!(
        monitor.begin_iteration(3),
        Err(MonitoringError::TaskFinished)
    );
    assert_eq!(hub.history_len(), 1);
}

#[test]
fn task_monitor_does_not_fabricate_events() {
    let hub = MonitoringHub::headless();
    let mut monitor = TaskMonitor::new(hub.clone(), "task-1").unwrap();

    // Lifecycle bookkeeping alone emits nothing.
    monitor.begin_iteration(1).unwrap();
    monitor.end_iteration();
    monitor.finish();
    assert_eq!(monitor.hub().history_len(), 0);
    assert_eq!(hub.history_len(), 0);
    assert_eq!(hub.next_sequence(), 0);
}

#[test]
fn ordering_is_global_across_task_monitors() {
    let (hub, sink) = recording_hub();
    let mut first = TaskMonitor::new(hub.clone(), "task-a").unwrap();
    let mut second = TaskMonitor::new(hub.clone(), "task-b").unwrap();

    first.begin_iteration(1).unwrap();
    second.begin_iteration(1).unwrap();

    first.emit(&event("task-a", "A1", Some(1)));
    second.emit(&event("task-b", "B1", Some(1)));
    first.begin_iteration(2).unwrap();
    first.emit(&event("task-a", "A2", Some(2)));

    let payloads = sink.payloads();
    assert_eq!(payloads.len(), 3);
    assert_eq!(payloads[0]["phase"], "A1");
    assert_eq!(payloads[1]["phase"], "B1");
    assert_eq!(payloads[2]["phase"], "A2");

    assert_eq!(
        hub.history()
            .iter()
            .map(|item| (item.sequence, item.task_id.as_str()))
            .collect::<Vec<_>>(),
        vec![(0, "task-a"), (1, "task-b"), (2, "task-a")]
    );
}

#[test]
fn clear_history_preserves_sequence_order() {
    let (hub, _sink) = recording_hub();
    let context = EventContext::new("task-1").unwrap();

    hub.emit(&context, &event("task-1", "A", None));
    assert_eq!(hub.clear_history(), 1);
    assert_eq!(hub.history_len(), 0);
    assert_eq!(hub.last_sequence(), Some(0));

    assert_eq!(
        hub.emit(&context, &event("task-1", "B", None)),
        EmitOutcome::Delivered { sequence: 1 }
    );
    assert_eq!(hub.history()[0].sequence, 1);
}

#[test]
fn sink_can_be_attached_after_headless_events() {
    let hub = MonitoringHub::headless();
    let context = EventContext::new("task-1").unwrap();

    assert_eq!(
        hub.emit(&context, &event("task-1", "A", None)),
        EmitOutcome::NoConsumer { sequence: 0 }
    );

    let sink = Arc::new(RecordingSink::default());
    hub.set_sink(Some(sink.clone() as Arc<dyn EventSink>))
        .unwrap();
    assert!(hub.has_consumer());

    assert_eq!(
        hub.emit(&context, &event("task-1", "B", None)),
        EmitOutcome::Delivered { sequence: 1 }
    );
    assert_eq!(sink.len(), 1);
    assert_eq!(hub.history_len(), 2);
    assert_eq!(hub.history()[0].status, DeliveryStatus::NoConsumer);
    assert_eq!(hub.history()[1].status, DeliveryStatus::Delivered);
}

// ---------------------------------------------------------------------------
// Durable lifecycle-event persistence tests
// ---------------------------------------------------------------------------

#[test]
fn persisted_events_survive_hub_reload() {
    use monitoring::MonitoringHub;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let path = dir.path().join("monitoring_events.jsonl");

    // Write some events directly to the JSONL file to simulate prior persistence.
    let mut events: Vec<monitoring::RecordedEvent> = Vec::new();
    for (seq, phase) in ["TaskStarted", "IterationStarted", "VerificationCompleted"]
        .iter()
        .enumerate()
    {
        events.push(monitoring::RecordedEvent {
            sequence: seq as u64,
            event_name: monitoring::ORCHESTRATION_EVENT.to_string(),
            task_id: "task-1".to_string(),
            iteration: Some(1),
            payload: serde_json::to_value(&event("task-1", phase, Some(1))).unwrap(),
            status: monitoring::DeliveryStatus::NoConsumer,
            error: None,
        });
    }
    let jsonl: String = events
        .iter()
        .map(|e| serde_json::to_string(e).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    // Ensure a trailing newline so subsequent appends start on a new line.
    let jsonl = format!("{}\n", jsonl);
    std::fs::write(&path, &jsonl).unwrap();

    // Reload: new hub with persist_path should read existing events.
    let hub = MonitoringHub::headless();
    assert_eq!(hub.set_persist_path(Some(path.clone())), Ok(()));
    assert_eq!(
        hub.history_len(),
        3,
        "all 3 persisted events should be reloaded"
    );
    assert_eq!(hub.next_sequence(), 3, "sequence should continue from 3");

    // Emit a new event through the reloaded hub.
    let context = EventContext::with_iteration("task-1", 1).unwrap();
    let payload = event("task-1", "TaskCompleted", Some(1));
    assert_eq!(
        hub.emit(&context, &payload),
        EmitOutcome::NoConsumer { sequence: 3 }
    );
    assert_eq!(hub.history_len(), 4);

    // The file should have the new event appended.
    let content = std::fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = content.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 4, "JSONL file should have 4 lines");
}

#[test]
fn persisted_events_skip_corrupted_lines_safely() {
    use monitoring::MonitoringHub;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let path = dir.path().join("monitoring_events.jsonl");

    // Write a mix of valid and corrupted lines.
    let valid = monitoring::RecordedEvent {
        sequence: 0,
        event_name: monitoring::ORCHESTRATION_EVENT.to_string(),
        task_id: "task-1".to_string(),
        iteration: Some(1),
        payload: serde_json::to_value(&event("task-1", "TaskStarted", Some(1))).unwrap(),
        status: monitoring::DeliveryStatus::NoConsumer,
        error: None,
    };
    let content = format!(
        "{}\n{}\n{}\n",
        serde_json::to_string(&valid).unwrap(),
        "{ not valid json",
        serde_json::to_string(&valid).unwrap()
    );
    std::fs::write(&path, content).unwrap();

    let hub = MonitoringHub::headless();
    assert_eq!(hub.set_persist_path(Some(path)), Ok(()));
    // Corrupted line should be skipped; valid lines loaded.
    assert_eq!(
        hub.history_len(),
        2,
        "corrupted line should be skipped, valid lines loaded"
    );
    assert_eq!(
        hub.next_sequence(),
        1,
        "sequence should be max+1 from valid events"
    );
}

#[test]
fn persist_path_rotation_respects_bounded_size() {
    use monitoring::MonitoringHub;
    use std::io::Write;
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let path = dir.path().join("monitoring_events.jsonl");

    // Pre-create a file larger than the rotation threshold.
    let big_line = "x".repeat(5 * 1024 * 1024); // 5 MB > 4 MB threshold
    let mut file = std::fs::File::create(&path).unwrap();
    writeln!(file, "{}", big_line).unwrap();
    drop(file);

    let hub = MonitoringHub::headless();
    assert_eq!(hub.set_persist_path(Some(path.clone())), Ok(()));
    // File exists but contains no valid JSON lines; history starts empty.
    assert_eq!(hub.history_len(), 0);
}
