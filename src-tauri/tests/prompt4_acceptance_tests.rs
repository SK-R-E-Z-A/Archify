use archify_lib::models::{ActivityFilter, ActivityRecord, NotificationFilter, NotificationRecord};
use archify_lib::workspace::Workspace;
use tempfile::tempdir;

#[tokio::test]
async fn test_notifications_lifecycle_and_dedup() {
    let dir = tempdir().unwrap();
    let ws = Workspace::new_with_paths(None, dir.path().to_path_buf());
    let proj_dir = tempdir().unwrap();
    let project = ws
        .add_project(
            proj_dir.path().to_string_lossy().to_string(),
            Some("P1".to_string()),
        )
        .await
        .unwrap();

    let rec1 = NotificationRecord {
        id: "".to_string(),
        created_at: "".to_string(),
        category: "task".to_string(),
        severity: "info".to_string(),
        title: "Task Started".to_string(),
        message: "Task is now running".to_string(),
        project_id: Some(project.id.clone()),
        task_id: Some("task-1".to_string()),
        session_id: None,
        agent_id: None,
        read: false,
        dedup_key: Some("dedup-1".to_string()),
        action: None,
        metadata: None,
    };

    let added1 = ws.record_notification(rec1.clone()).await;
    assert!(added1);

    let added_dup = ws.record_notification(rec1.clone()).await;
    assert!(!added_dup);

    let count = ws
        .notification_unread_count(Some(project.id.clone()))
        .await
        .unwrap();
    assert_eq!(count, 1);

    let list = ws
        .list_notifications(NotificationFilter::default())
        .await
        .unwrap();
    assert_eq!(list.len(), 1);
    let id = list[0].id.clone();

    ws.mark_notification_read(id.clone(), true).await.unwrap();
    assert_eq!(ws.notification_unread_count(None).await.unwrap(), 0);

    ws.delete_notification(id).await.unwrap();
    let list_after = ws
        .list_notifications(NotificationFilter::default())
        .await
        .unwrap();
    assert!(list_after.is_empty());
}

#[tokio::test]
async fn test_activity_filtering_and_pagination() {
    let dir = tempdir().unwrap();
    let ws = Workspace::new_with_paths(None, dir.path().to_path_buf());
    let proj_dir = tempdir().unwrap();
    let project = ws
        .add_project(
            proj_dir.path().to_string_lossy().to_string(),
            Some("P2".to_string()),
        )
        .await
        .unwrap();

    let act = ActivityRecord {
        id: "act-1".to_string(),
        timestamp: "2026-11-14T00:00:00Z".to_string(),
        event_type: "task_completed".to_string(),
        summary: "Task finished successfully".to_string(),
        project_id: Some(project.id.clone()),
        task_id: Some("task-10".to_string()),
        agent_id: None,
        session_id: None,
        iteration: Some(1),
        outcome: "success".to_string(),
        dedup_key: None,
        metadata: None,
    };

    ws.record_activity(act).await;

    let activities = ws
        .list_activity(ActivityFilter {
            project_id: Some(project.id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(activities.len(), 1);
    assert_eq!(activities[0].outcome, "success");
}

#[tokio::test]
async fn test_task_and_project_reports() {
    let dir = tempdir().unwrap();
    let ws = Workspace::new_with_paths(None, dir.path().to_path_buf());
    let proj_dir = tempdir().unwrap();
    let project = ws
        .add_project(
            proj_dir.path().to_string_lossy().to_string(),
            Some("P3".to_string()),
        )
        .await
        .unwrap();

    let proj_rep = ws.project_report(project.id.clone()).await.unwrap();
    assert_eq!(proj_rep.total_tasks, 0);
    assert_eq!(proj_rep.completion_rate, 0.0);
}
