use super::models::{TaskRecord, TaskState};

#[allow(dead_code)]
fn task_record_with_state(id: &str, state: TaskState) -> TaskRecord {
    let now = chrono::Utc::now().to_rfc3339();
    TaskRecord {
        id: id.to_string(),
        user_task: format!("Task {}", id),
        project_root: std::env::temp_dir().to_string_lossy().to_string(),
        state,
        plan: None,
        task_graph: None,
        iteration: 0,
        max_iterations: 3,
        iterations: Vec::new(),
        created_at: now.clone(),
        updated_at: now,
        error: None,
        project_id: None,
        provider_profile_id: None,
        agent_name: None,
        priority: 0,
        queued_at: None,
        recovery_attempts: 0,
        task_level_changes: None,
        task_baseline: None,
        execution_config: None,
        planning_context: None,
        source_session_id: None,
        selected_skill_ids: Vec::new(),
        task_model_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::TaskState;

    #[test]
    fn test_state_machine_valid_transitions() {
        assert!(TaskState::Idle.can_transition_to(&TaskState::Planning));
        assert!(TaskState::Planning.can_transition_to(&TaskState::AwaitingApproval));
        assert!(TaskState::Planning.can_transition_to(&TaskState::Failed));
        assert!(TaskState::Planning.can_transition_to(&TaskState::Cancelled));
        assert!(TaskState::AwaitingApproval.can_transition_to(&TaskState::Executing));
        assert!(TaskState::AwaitingApproval.can_transition_to(&TaskState::Cancelled));
        assert!(TaskState::Executing.can_transition_to(&TaskState::Verifying));
        assert!(TaskState::Executing.can_transition_to(&TaskState::Reviewing));
        assert!(TaskState::Verifying.can_transition_to(&TaskState::Reviewing));
        assert!(TaskState::Verifying.can_transition_to(&TaskState::Verifying));
        assert!(TaskState::Verifying.can_transition_to(&TaskState::Completed));
        assert!(TaskState::Verifying.can_transition_to(&TaskState::Retrying));
        assert!(TaskState::Retrying.can_transition_to(&TaskState::Executing));
    }

    #[test]
    fn test_task_state_grouping_helpers() {
        assert!(TaskState::Queued.is_queued());
        assert!(!TaskState::Planning.is_queued());

        assert!(TaskState::Planning.is_planning());
        assert!(!TaskState::Executing.is_planning());

        assert!(TaskState::Executing.is_running());
        assert!(TaskState::Verifying.is_running());
        assert!(TaskState::Reviewing.is_running());
        assert!(TaskState::Retrying.is_running());
        assert!(!TaskState::Planning.is_running());
        assert!(!TaskState::AwaitingApproval.is_running());

        // AwaitingApproval is "Holding"
        assert!(TaskState::AwaitingApproval.is_holding());
        assert!(!TaskState::Executing.is_holding());
    }

    #[test]
    fn test_task_state_transition_matrix() {
        let states = vec![
            TaskState::Idle,
            TaskState::Queued,
            TaskState::Planning,
            TaskState::AwaitingApproval,
            TaskState::Executing,
            TaskState::Verifying,
            TaskState::Reviewing,
            TaskState::Retrying,
            TaskState::Completed,
            TaskState::Failed,
            TaskState::Cancelled,
        ];

        for from in &states {
            for to in &states {
                let can = from.can_transition_to(to);
                // Verify matrix coherence (manual check of complex rules)
                if from.is_terminal() {
                    assert!(
                        !can,
                        "Terminal state {:?} cannot transition to {:?}",
                        from, to
                    );
                }
            }
        }
    }

    #[test]
    fn test_state_machine_invalid_transitions() {
        // Cannot skip states
        assert!(!TaskState::Idle.can_transition_to(&TaskState::Executing));
        assert!(!TaskState::Planning.can_transition_to(&TaskState::Completed));
        assert!(!TaskState::AwaitingApproval.can_transition_to(&TaskState::Completed));
        assert!(!TaskState::Executing.can_transition_to(&TaskState::Completed));

        // Terminal states cannot transition out
        assert!(!TaskState::Completed.can_transition_to(&TaskState::Planning));
        assert!(!TaskState::Failed.can_transition_to(&TaskState::Executing));
        assert!(!TaskState::Cancelled.can_transition_to(&TaskState::Retrying));
    }
}
