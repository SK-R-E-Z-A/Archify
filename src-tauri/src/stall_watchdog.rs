//! Automatic background stall watchdog for executor processes.
//!
//! The watchdog runs on the scheduler tick (roughly once per second) and
//! inspects every active executor process. When a running process has produced
//! no new output for the configured stall window, the watchdog emits a
//! `StallDetected` lifecycle event through the shared `MonitoringHub`. When a
//! previously-stalled process resumes producing output, a resumed event is
//! emitted. Stall notifications are deduplicated per task so the same stall is
//! not reported repeatedly.
//!
//! Design rules:
//! * The watchdog never cancels or terminates a process on its own. It only
//!   observes and reports stalls; cancellation remains an explicit user or
//!   scheduler action.
//! * A process that is producing output (even slowly) is never reported as
//!   stalled, so long-running operations (compilation, test suites, large
//!   file edits) do not trigger false stalls.
//! * Stall state is tracked per task so a resume after a stall is detected and
//!   reported exactly once.
//! * Registration and deregistration are coordinated with `run_process` so the
//!   registry never retains a tracker for a process that has already exited.
//! * All locks are held only briefly and never across await points, so the
//!   watchdog cannot deadlock with process termination or completion.
//!
//! The registry is a process-global `OnceLock` because `run_process` is a free
//! function called from executor code that does not have access to the
//! workspace or orchestrator.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::agent_lifecycle::{
    AgentLifecycleTracker, LifecycleEvent, LifecycleEventType, ProcessState,
};
use crate::monitoring::{EventContext, MonitoringHub};

/// Default stall window: a process is only declared stalled after 5 minutes
/// with no new output. This is deliberately long so that long-running but
/// legitimate operations (compilation, test suites, large file edits) do not
/// produce false stall reports.
pub const DEFAULT_STALL_WINDOW_SECS: u64 = 300;

/// Registered active executor process.
struct ProcessTrackerEntry {
    #[allow(dead_code)]
    task_id: String,
    tracker: Arc<tokio::sync::Mutex<AgentLifecycleTracker>>,
    /// Stall window applied to this process (seconds).
    stall_window_secs: u64,
    /// Whether a stall has already been reported for the current stall episode.
    /// Prevents duplicate stall notifications while the process remains stuck.
    currently_stalled: bool,
}

/// Global registry of active executor processes, keyed by task ID.
static REGISTRY: OnceLock<Arc<Mutex<HashMap<String, ProcessTrackerEntry>>>> = OnceLock::new();

fn registry() -> Arc<Mutex<HashMap<String, ProcessTrackerEntry>>> {
    REGISTRY
        .get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
        .clone()
}

/// Register an active executor process with the watchdog.
///
/// Called by `run_process` when a child process is spawned. The tracker is
/// shared so the watchdog can observe output freshness without interfering
/// with the reader tasks that record output.
pub fn register_process(
    task_id: String,
    tracker: Arc<tokio::sync::Mutex<AgentLifecycleTracker>>,
    stall_window_secs: u64,
) {
    let reg = registry();
    let Ok(mut reg) = reg.lock() else { return };
    reg.insert(
        task_id.clone(),
        ProcessTrackerEntry {
            task_id,
            tracker,
            stall_window_secs,
            currently_stalled: false,
        },
    );
}

/// Deregister a process that is no longer running.
///
/// Called by `run_process` on every exit path (completion, failure, timeout,
/// cancellation) so the registry never leaks entries for exited processes.
pub fn deregister_process(task_id: &str) {
    let reg = registry();
    let Ok(mut reg) = reg.lock() else { return };
    reg.remove(task_id);
}

/// Return the number of processes currently registered with the watchdog.
/// Exposed for tests and diagnostics.
pub fn registered_count() -> usize {
    match registry().lock() {
        Ok(reg) => reg.len(),
        Err(_) => 0,
    }
}

/// Clear the registry. Test-only helper; gated behind `#[cfg(test)]` usage by
/// convention (the function itself is always compiled so integration tests in
/// separate crates can call it).
pub fn clear_registry_for_test() {
    let reg = registry();
    let Ok(mut reg) = reg.lock() else { return };
    reg.clear();
}

/// Check every registered process for stall conditions and emit lifecycle
/// events through the monitoring hub.
///
/// Intended to be called from the scheduler tick (roughly once per second).
/// Holds the registry lock only long enough to snapshot which tasks need
/// events, then emits outside the lock.
pub fn check_stalls(hub: &MonitoringHub) {
    let mut to_report_stall: Vec<String> = Vec::new();
    let mut to_report_resume: Vec<String> = Vec::new();

    // First pass: snapshot stall state under the registry lock.
    {
        let reg_handle = registry();
        let Ok(reg) = reg_handle.lock() else { return };

        for (task_id, entry) in reg.iter() {
            let Ok(tracker) = entry.tracker.try_lock() else {
                continue;
            };

            // Only monitor processes that are still running.
            if !matches!(tracker.state(), ProcessState::Running) {
                continue;
            }

            let silence = match tracker.output_silence_elapsed() {
                Some(s) => s,
                None => continue,
            };
            let stall_window = Duration::from_secs(entry.stall_window_secs);

            if silence >= stall_window {
                // Genuinely stalled and not already reported for this episode.
                if !entry.currently_stalled {
                    to_report_stall.push(task_id.clone());
                }
            } else if entry.currently_stalled {
                // Was stalled, now producing output again: report resume.
                to_report_resume.push(task_id.clone());
            }
            drop(tracker);
        }
    } // registry lock dropped here

    // Second pass: mark state and emit events outside the registry lock.
    {
        let reg_handle = registry();
        let Ok(mut reg) = reg_handle.lock() else {
            return;
        };

        for task_id in &to_report_stall {
            if let Some(entry) = reg.get_mut(task_id) {
                entry.currently_stalled = true;
            }
            let event = LifecycleEvent {
                event_type: LifecycleEventType::StallDetected,
                timestamp: chrono::Utc::now(),
                details: Some(format!(
                    "Process for task {} has produced no output for {} seconds; \
                     it may be stalled.",
                    task_id,
                    reg.get(task_id)
                        .map(|e| e.stall_window_secs)
                        .unwrap_or(DEFAULT_STALL_WINDOW_SECS)
                )),
                exit_code: None,
            };
            if let Ok(context) = EventContext::with_iteration(task_id.clone(), 1) {
                let _ = hub.emit(&context, &event);
            }
        }

        for task_id in &to_report_resume {
            if let Some(entry) = reg.get_mut(task_id) {
                entry.currently_stalled = false;
            }
            let event = LifecycleEvent {
                event_type: LifecycleEventType::ProcessRunning,
                timestamp: chrono::Utc::now(),
                details: Some(format!(
                    "Process for task {} has resumed producing output after a \
                     stall; no longer stalled.",
                    task_id
                )),
                exit_code: None,
            };
            if let Ok(context) = EventContext::with_iteration(task_id.clone(), 1) {
                let _ = hub.emit(&context, &event);
            }
        }
    }
}

/// Serializes stall-watchdog unit tests so they don't interfere with each
/// other through the shared global process registry.
#[cfg(test)]
static TEST_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent_lifecycle::TrackerConfig;
    use std::time::Duration;

    fn make_tracker(stall_window_secs: u64) -> Arc<tokio::sync::Mutex<AgentLifecycleTracker>> {
        let config = TrackerConfig {
            stall_window_secs,
            heartbeat_interval_secs: 10,
        };
        Arc::new(tokio::sync::Mutex::new(AgentLifecycleTracker::new(config)))
    }

    #[tokio::test]
    async fn register_and_deregister_track_count() {
        let _guard = TEST_MUTEX.lock().unwrap();
        deregister_process("wd-count-1");
        assert_eq!(registered_count(), 0);

        let t = make_tracker(300);
        register_process("wd-count-1".to_string(), t.clone(), 300);
        assert_eq!(registered_count(), 1);

        // Re-registering the same task replaces, not duplicates.
        register_process("wd-count-1".to_string(), t, 300);
        assert_eq!(registered_count(), 1);

        deregister_process("wd-count-1");
        assert_eq!(registered_count(), 0);
        deregister_process("wd-count-1"); // idempotent
        assert_eq!(registered_count(), 0);
    }

    #[tokio::test]
    async fn stall_detected_when_silent_longer_than_window() {
        let _guard = TEST_MUTEX.lock().unwrap();
        deregister_process("wd-stall");
        let hub = MonitoringHub::headless();
        let t = make_tracker(1); // 1-second stall window
        {
            let mut guard = t.lock().await;
            guard.record_start();
        }
        register_process("wd-stall".to_string(), t.clone(), 1);

        // Immediately after start, not stalled (no time has passed).
        check_stalls(&hub);
        assert_eq!(
            hub.history_len(),
            0,
            "no stall should be reported immediately"
        );

        // Advance time past the stall window by waiting briefly.
        tokio::time::sleep(Duration::from_millis(1100)).await;
        check_stalls(&hub);
        assert_eq!(hub.history_len(), 1, "one stall event should be reported");
        let ev = &hub.history()[0];
        let payload: LifecycleEvent = serde_json::from_value(ev.payload.clone()).unwrap();
        assert_eq!(payload.event_type, LifecycleEventType::StallDetected);
        assert!(payload.details.is_some(), "stall event should have details");
        deregister_process("wd-stall");
    }

    #[tokio::test]
    async fn stall_not_repeated_while_still_stalled() {
        let _guard = TEST_MUTEX.lock().unwrap();
        deregister_process("wd-norepeat");
        let hub = MonitoringHub::headless();
        let t = make_tracker(1);
        {
            let mut guard = t.lock().await;
            guard.record_start();
        }
        register_process("wd-norepeat".to_string(), t.clone(), 1);

        tokio::time::sleep(Duration::from_millis(1100)).await;
        check_stalls(&hub);
        check_stalls(&hub);
        check_stalls(&hub);
        // Only one stall event despite repeated checks.
        assert_eq!(
            hub.history()
                .iter()
                .filter(|e| {
                    serde_json::from_value::<LifecycleEvent>(e.payload.clone())
                        .map(|p| matches!(p.event_type, LifecycleEventType::StallDetected))
                        .unwrap_or(false)
                })
                .count(),
            1,
            "stall should not be reported repeatedly"
        );
        deregister_process("wd-norepeat");
    }

    #[tokio::test]
    async fn resume_after_stall_is_reported() {
        let _guard = TEST_MUTEX.lock().unwrap();
        deregister_process("wd-resume");
        let hub = MonitoringHub::headless();
        let t = make_tracker(1);
        {
            let mut guard = t.lock().await;
            guard.record_start();
        }
        register_process("wd-resume".to_string(), t.clone(), 1);

        // Stall it.
        tokio::time::sleep(Duration::from_millis(1100)).await;
        check_stalls(&hub);
        assert_eq!(
            hub.history()
                .iter()
                .filter(|e| {
                    serde_json::from_value::<LifecycleEvent>(e.payload.clone())
                        .map(|p| matches!(p.event_type, LifecycleEventType::StallDetected))
                        .unwrap_or(false)
                })
                .count(),
            1,
            "one stall event should be reported"
        );

        // Produce output: resets the stall timer.
        {
            let mut guard = t.lock().await;
            guard.record_output(42);
        }
        check_stalls(&hub);
        let resume_count = hub
            .history()
            .iter()
            .filter(|e| {
                serde_json::from_value::<LifecycleEvent>(e.payload.clone())
                    .map(|p| matches!(p.event_type, LifecycleEventType::ProcessRunning))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(resume_count, 1, "resume should be reported after output");

        // Stall again and confirm a new stall episode is reported.
        tokio::time::sleep(Duration::from_millis(1100)).await;
        check_stalls(&hub);
        let stall_count = hub
            .history()
            .iter()
            .filter(|e| {
                serde_json::from_value::<LifecycleEvent>(e.payload.clone())
                    .map(|p| matches!(p.event_type, LifecycleEventType::StallDetected))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(
            stall_count, 2,
            "new stall episode after resume should be reported"
        );
        deregister_process("wd-resume");
    }

    #[tokio::test]
    async fn non_running_process_is_not_monitored() {
        let _guard = TEST_MUTEX.lock().unwrap();
        deregister_process("wd-done");
        let hub = MonitoringHub::headless();
        let t = make_tracker(1);
        {
            let mut guard = t.lock().await;
            guard.record_start();
            guard.record_completed(Some(0));
        }
        register_process("wd-done".to_string(), t.clone(), 1);

        tokio::time::sleep(Duration::from_millis(1100)).await;
        check_stalls(&hub);
        assert_eq!(
            hub.history_len(),
            0,
            "completed process should not generate stall events"
        );
        deregister_process("wd-done");
    }

    #[tokio::test]
    async fn slow_output_resets_stall_timer_no_false_stall() {
        let _guard = TEST_MUTEX.lock().unwrap();
        deregister_process("wd-slow");
        let hub = MonitoringHub::headless();
        let t = make_tracker(1);
        {
            let mut guard = t.lock().await;
            guard.record_start();
        }
        register_process("wd-slow".to_string(), t.clone(), 1);

        // Produce a byte of output every 600ms (faster than the 1s stall window).
        for _ in 0..5 {
            tokio::time::sleep(Duration::from_millis(600)).await;
            {
                let mut guard = t.lock().await;
                guard.record_output(1);
            }
            check_stalls(&hub);
        }
        assert_eq!(
            hub.history_len(),
            0,
            "slow but steady output should not trigger a stall"
        );
        deregister_process("wd-slow");
    }
}
