use archify_lib::models::{ProjectRecord, TaskState};
use archify_lib::workspace::Workspace;
use tempfile::tempdir;

#[tokio::test]
async fn p2_project_lifecycle() {
    let dir = tempdir().unwrap();
    let ws = Workspace::new_with_paths(None, dir.path().to_path_buf());
    let proj_dir = tempdir().unwrap();
    let project = ws
        .add_project(
            proj_dir.path().to_string_lossy().to_string(),
            Some("Alpha".to_string()),
        )
        .await
        .unwrap();
    assert_eq!(project.name, "Alpha");
    let renamed = ws
        .rename_project(project.id.clone(), "Beta".to_string())
        .await
        .unwrap();
    assert_eq!(renamed.name, "Beta");
    assert_eq!(renamed.root, project.root);
    let archived = ws.archive_project(project.id.clone(), true).await.unwrap();
    assert!(archived.archived);
    let unarchived = ws.unarchive_project(project.id.clone()).await.unwrap();
    assert!(!unarchived.archived);
    let got = ws.get_project(project.id.clone()).await.unwrap().unwrap();
    assert_eq!(got.name, "Beta");
}

#[tokio::test]
async fn p2_project_switching_restore() {
    let dir = tempdir().unwrap();
    let ws = Workspace::new_with_paths(None, dir.path().to_path_buf());
    let d1 = tempdir().unwrap();
    let d2 = tempdir().unwrap();
    let p1 = ws
        .add_project(
            d1.path().to_string_lossy().to_string(),
            Some("P1".to_string()),
        )
        .await
        .unwrap();
    let p2 = ws
        .add_project(
            d2.path().to_string_lossy().to_string(),
            Some("P2".to_string()),
        )
        .await
        .unwrap();
    // switch not a workspace method; just verify both exist
    assert_ne!(p1.id, p2.id);
    // reload
    let ws2 = Workspace::new_with_paths(None, dir.path().to_path_buf());
    let projects = ws2.list_projects().await.unwrap();
    assert_eq!(projects.len(), 2);
}

#[tokio::test]
async fn p2_duplicate_detection() {
    let dir = tempdir().unwrap();
    let ws = Workspace::new_with_paths(None, dir.path().to_path_buf());
    let pd = tempdir().unwrap();
    ws.add_project(pd.path().to_string_lossy().to_string(), None)
        .await
        .unwrap();
    let err = ws
        .add_project(pd.path().to_string_lossy().to_string(), None)
        .await
        .unwrap_err();
    assert!(err.contains("already added"));
}

#[tokio::test]
async fn p2_archived_cannot_receive_tasks() {
    let dir = tempdir().unwrap();
    let ws = Workspace::new_with_paths(None, dir.path().to_path_buf());
    let pd = tempdir().unwrap();
    let p = ws
        .add_project(pd.path().to_string_lossy().to_string(), None)
        .await
        .unwrap();
    ws.archive_project(p.id.clone(), true).await.unwrap();
    let err = ws
        .start_task(
            "task".to_string(),
            pd.path().to_string_lossy().to_string(),
            archify_lib::models::StartTaskOptions {
                project_id: Some(p.id.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap_err();
    assert!(err.contains("Archived"));
}

#[tokio::test]
async fn p2_missing_dir() {
    let dir = tempdir().unwrap();
    let ws = Workspace::new_with_paths(None, dir.path().to_path_buf());
    let missing = dir.path().join("nope");
    let err = ws
        .add_project(missing.to_string_lossy().to_string(), None)
        .await
        .unwrap_err();
    assert!(err.contains("does not exist"));
    let ids = ws.detect_missing_project_roots().await;
    assert!(ids.is_empty());
}

#[tokio::test]
async fn p2_search_projects() {
    let dir = tempdir().unwrap();
    let ws = Workspace::new_with_paths(None, dir.path().to_path_buf());
    let d1 = tempdir().unwrap();
    ws.add_project(
        d1.path().to_string_lossy().to_string(),
        Some("AlphaProject".to_string()),
    )
    .await
    .unwrap();
    let res = ws.search_projects("Alpha".to_string()).await;
    assert_eq!(res.len(), 1);
    let res2 = ws.search_projects("zzz".to_string()).await;
    assert!(res2.is_empty());
}

#[tokio::test]
async fn p2_dashboard_counts() {
    let dir = tempdir().unwrap();
    let ws = Workspace::new_with_paths(None, dir.path().to_path_buf());
    let pd = tempdir().unwrap();
    let p = ws
        .add_project(
            pd.path().to_string_lossy().to_string(),
            Some("P".to_string()),
        )
        .await
        .unwrap();
    let summary = ws.get_project_dashboard(p.id.clone()).await.unwrap();
    assert_eq!(summary.active_task_count, 0);
    assert_eq!(summary.completed_task_count, 0);
    assert!(summary.directory_available);
}

#[tokio::test]
async fn p2_update_root() {
    let dir = tempdir().unwrap();
    let ws = Workspace::new_with_paths(None, dir.path().to_path_buf());
    let d1 = tempdir().unwrap();
    let d2 = tempdir().unwrap();
    let p = ws
        .add_project(d1.path().to_string_lossy().to_string(), None)
        .await
        .unwrap();
    let updated = ws
        .update_project_root(p.id.clone(), d2.path().to_string_lossy().to_string())
        .await
        .unwrap();
    let expected = dunce::canonicalize(d2.path())
        .unwrap()
        .to_string_lossy()
        .to_string();
    assert_eq!(updated.root, expected);
    assert_eq!(updated.name, p.name);
}

#[tokio::test]
async fn p2_token_attribution() {
    let dir = tempdir().unwrap();
    let ws = Workspace::new_with_paths(None, dir.path().to_path_buf());
    ws.record_token_usage(
        Some(10),
        Some(5),
        None,
        None,
        None,
        None,
        Some("proj-a".to_string()),
        None,
        None,
        None,
        "completion".to_string(),
    )
    .await
    .unwrap();
    let stats = ws
        .get_token_usage_by_project("proj-a".to_string())
        .await
        .unwrap();
    assert_eq!(stats.total_input_tokens, 10);
}

#[tokio::test]
async fn p2_concurrency_different_projects() {
    let dir = tempdir().unwrap();
    let ws = Workspace::new_with_paths(None, dir.path().to_path_buf());
    let d1 = tempdir().unwrap();
    let d2 = tempdir().unwrap();
    let p1 = ws
        .add_project(d1.path().to_string_lossy().to_string(), None)
        .await
        .unwrap();
    let p2 = ws
        .add_project(d2.path().to_string_lossy().to_string(), None)
        .await
        .unwrap();
    // ensure no conflict helper
    assert!(ws.can_start_task_in_project(&p1.id).await.is_ok());
    assert!(ws.can_start_task_in_project(&p2.id).await.is_ok());
}
