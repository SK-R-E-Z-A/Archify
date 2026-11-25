// ---------------------------------------------------------------------------
// Active project persistence tests
// ---------------------------------------------------------------------------

use archify_lib::workspace::Workspace;
use std::env;
use std::path::PathBuf;
use uuid::Uuid;

fn make_temp_dir() -> PathBuf {
    let dir = env::current_dir().unwrap().join("target").join(format!("archify_active_project_test_{}", Uuid::new_v4()));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        panic!("Failed to create temp directory: {}", e);
    }
    dir
}

#[tokio::test]
async fn select_project_persists_and_restores() {
    let dir = make_temp_dir();
    let project_path = dir.join("my-project");
    std::fs::create_dir_all(&project_path).unwrap();

    // First workspace instance
    let workspace = Workspace::new_with_paths(None, dir.clone());

    // Create a project
    let project = workspace
        .add_project(
            project_path.to_string_lossy().to_string(),
            Some("My Project".to_string()),
        )
        .await
        .unwrap();
    let project_id = project.id.clone();

    // Select the project
    let selected = workspace.project(project_id.clone()).await.unwrap();

    assert_eq!(selected.id, project_id);
    assert_eq!(selected.name, "My Project");

    // Second workspace instance (simulates restart) using SAME directory
    let workspace2 = Workspace::new_with_paths(None, dir);

    // Reload projects - should load from persisted workspace.json
    let projects = workspace2.list_all_projects().await;
    assert_eq!(projects.len(), 1);

    // Select the same project in the new workspace
    let selected2 = workspace2.project(project_id.clone()).await.unwrap();
    assert_eq!(selected2.id, project_id);
}

#[tokio::test]
async fn description_and_tags_persist() {
    let dir = make_temp_dir();
    let project_path = dir.join("my-project");
    std::fs::create_dir_all(&project_path).unwrap();

    // First workspace instance
    let workspace = Workspace::new_with_paths(None, dir.clone());

    // Create a project
    let project = workspace
        .add_project(project_path.to_string_lossy().to_string(), None)
        .await
        .unwrap();

    // Set description and tags
    workspace
        .update_project(
            project.id.clone(),
            None,
            None,
            None,
            Some("Test description".to_string()),
            Some(vec!["tag1".to_string(), "tag2".to_string()]),
        )
        .await
        .unwrap();

    // Second workspace instance (reload from same directory)
    let workspace2 = Workspace::new_with_paths(None, dir);

    // Check that description and tags persist
    let projects = workspace2.list_all_projects().await;
    assert_eq!(projects.len(), 1);

    let loaded_project = &projects[0];
    assert_eq!(
        loaded_project.description,
        Some("Test description".to_string())
    );
    assert_eq!(
        loaded_project.tags,
        vec!["tag1".to_string(), "tag2".to_string()]
    );
}
