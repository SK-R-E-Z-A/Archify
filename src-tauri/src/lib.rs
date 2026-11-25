pub mod agent_lifecycle;
pub mod agent_registry;
pub mod connection_test;
pub mod constants;
pub mod credentials;
pub mod instructions;
pub mod model_selection;
pub mod models;
pub mod monitoring;
pub mod orchestrator;
pub mod providers;
pub mod recovery_policy;
pub mod skills;
pub mod stall_watchdog;
pub mod terminal;
pub mod token_usage;
pub mod verification;
pub mod workspace;

#[cfg(test)]
mod tests;

use instructions::{
    AppliedInstructionBlock, InstructionRecord, InstructionUpsertInput, InstructionValidationIssue,
};
use model_selection::{AiResponsibility, ModelInfo, ModelPreferences, ModelSelection};
use models::{
    ActivityFilter, ActivityRecord, AvailabilityCheck, CliAgentPreset, CreateSessionOptions,
    FileEntry, LocalModelPreset, NotificationFilter, NotificationRecord, ProjectRecord,
    ProjectReport, ProjectSummary, ProviderConfigInput, ProviderProfile, ProviderStatus,
    SessionRecord, StartTaskOptions, TaskRecord, TaskReport, WorkspaceSettings, WorkspaceSnapshot,
};
use skills::{AppliedSkillBlock, SkillRecord, SkillUpsertInput, SkillValidationIssue};
use std::path::Path;
use tauri::Manager;
use token_usage::{TokenUsageRecord, TokenUsageStats};
use workspace::Workspace;

// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[tauri::command]
fn list_directory(
    project_root: String,
    directory_path: Option<String>,
) -> Result<Vec<FileEntry>, String> {
    let root = Path::new(&project_root);
    if !root.exists() {
        return Err("Project root directory does not exist".to_string());
    }

    let canonical_root = dunce::canonicalize(root)
        .map_err(|e| format!("Failed to canonicalize project root: {e}"))?;

    if !canonical_root.is_dir() {
        return Err("Project root is not a directory".to_string());
    }

    let target = match directory_path {
        Some(ref p) if !p.trim().is_empty() => {
            let p_path = Path::new(p);
            if p_path.is_absolute() {
                p_path.to_path_buf()
            } else {
                canonical_root.join(p_path)
            }
        }
        _ => canonical_root.clone(),
    };

    if !target.exists() {
        return Err("Directory does not exist".to_string());
    }

    let canonical_target = dunce::canonicalize(&target)
        .map_err(|e| format!("Failed to canonicalize directory path: {e}"))?;

    if !canonical_target.is_dir() {
        return Err("Specified path is not a directory".to_string());
    }

    if !canonical_target.starts_with(&canonical_root) {
        return Err("Access denied: path is outside the project root".to_string());
    }

    let read_dir = std::fs::read_dir(&canonical_target)
        .map_err(|e| format!("Failed to read directory: {e}"))?;

    let mut entries = Vec::new();

    for entry_result in read_dir {
        let entry = match entry_result {
            Ok(e) => e,
            Err(_) => continue,
        };

        let file_type = match entry.file_type() {
            Ok(ft) => ft,
            Err(_) => continue,
        };

        let is_symlink = file_type.is_symlink();
        let mut is_dir = file_type.is_dir();

        if is_symlink {
            match dunce::canonicalize(entry.path()) {
                Ok(resolved) => {
                    if !resolved.starts_with(&canonical_root) {
                        continue;
                    }
                    is_dir = resolved.is_dir();
                }
                Err(_) => continue,
            }
        }

        let name = entry.file_name().to_string_lossy().to_string();
        let path = dunce::simplified(&entry.path())
            .to_string_lossy()
            .to_string();

        entries.push(FileEntry { name, path, is_dir });
    }

    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });

    Ok(entries)
}

// ---------------------------------------------------------------------------
// Workspace / project commands
// ---------------------------------------------------------------------------

#[tauri::command]
async fn get_workspace(
    workspace: tauri::State<'_, Workspace>,
) -> Result<WorkspaceSnapshot, String> {
    Ok(workspace.snapshot().await)
}

#[tauri::command]
async fn update_workspace_settings(
    settings: WorkspaceSettings,
    workspace: tauri::State<'_, Workspace>,
) -> Result<WorkspaceSnapshot, String> {
    workspace.update_settings(settings).await
}

#[tauri::command]
async fn add_project(
    root: String,
    name: Option<String>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ProjectRecord, String> {
    workspace.add_project(root, name).await
}

#[tauri::command]
async fn update_project(
    project_id: String,
    name: Option<String>,
    provider_profile_id: Option<String>,
    pinned: Option<bool>,
    description: Option<String>,
    tags: Option<Vec<String>>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ProjectRecord, String> {
    workspace
        .update_project(
            project_id,
            name,
            provider_profile_id,
            pinned,
            description,
            tags,
        )
        .await
}

#[tauri::command]
async fn remove_project(
    project_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<(), String> {
    workspace.remove_project(project_id).await
}

#[tauri::command]
async fn archive_project(
    project_id: String,
    archived: bool,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ProjectRecord, String> {
    workspace.archive_project(project_id, archived).await
}

#[tauri::command]
async fn get_project(
    project_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<Option<ProjectRecord>, String> {
    workspace.get_project(project_id).await
}

#[tauri::command]
async fn list_projects(
    workspace: tauri::State<'_, Workspace>,
) -> Result<Vec<ProjectRecord>, String> {
    workspace.list_projects().await
}

#[tauri::command]
async fn search_projects(
    query: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<Vec<ProjectRecord>, String> {
    Ok(workspace.search_projects(query).await)
}

#[tauri::command]
async fn update_project_root(
    project_id: String,
    new_root: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ProjectRecord, String> {
    workspace.update_project_root(project_id, new_root).await
}

#[tauri::command]
async fn rename_project(
    project_id: String,
    name: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ProjectRecord, String> {
    workspace.rename_project(project_id, name).await
}

#[tauri::command]
async fn get_project_summary(
    project_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ProjectSummary, String> {
    workspace.get_project_summary(project_id).await
}

#[tauri::command]
async fn create_project(
    root: String,
    name: Option<String>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ProjectRecord, String> {
    workspace.create_project(root, name).await
}

#[tauri::command]
async fn select_project(
    project_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ProjectRecord, String> {
    workspace.select_project(project_id).await
}

#[tauri::command]
async fn detect_missing_project_roots(
    workspace: tauri::State<'_, Workspace>,
) -> Result<Vec<String>, String> {
    Ok(workspace.detect_missing_project_roots().await)
}

// ---------------------------------------------------------------------------
// Provider profile commands
// ---------------------------------------------------------------------------

#[tauri::command]
async fn list_provider_profiles(
    workspace: tauri::State<'_, Workspace>,
) -> Result<Vec<ProviderProfile>, String> {
    Ok(workspace.snapshot().await.provider_profiles)
}

#[tauri::command]
async fn create_provider_profile(
    name: String,
    project_id: Option<String>,
    is_default: bool,
    config: ProviderConfigInput,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ProviderProfile, String> {
    workspace
        .create_provider_profile(name, project_id, is_default, config)
        .await
}

#[tauri::command]
async fn update_provider_profile(
    profile_id: String,
    name: Option<String>,
    is_default: Option<bool>,
    config: ProviderConfigInput,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ProviderProfile, String> {
    workspace
        .update_provider_profile(profile_id, name, is_default, config)
        .await
}

#[tauri::command]
async fn delete_provider_profile(
    profile_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<(), String> {
    workspace.delete_provider_profile(profile_id).await
}

#[tauri::command]
async fn delete_provider_credential(
    profile_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<(), String> {
    workspace.delete_provider_credential(profile_id).await
}

#[tauri::command]
async fn get_provider_status(
    workspace: tauri::State<'_, Workspace>,
) -> Result<ProviderStatus, String> {
    workspace.default_provider_status().await
}

#[tauri::command]
async fn get_provider_status_for(
    profile_id: Option<String>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ProviderStatus, String> {
    workspace.provider_status_for(profile_id).await
}

#[tauri::command]
async fn get_session_provider_status(
    session_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ProviderStatus, String> {
    workspace.session_provider_status(session_id).await
}

#[tauri::command]
async fn configure_providers(
    config: ProviderConfigInput,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ProviderStatus, String> {
    // Legacy entry point: update the default global profile in place.
    let profile_id = {
        let core = workspace.core.lock().await;
        core.provider_profiles
            .iter()
            .find(|profile| profile.project_id.is_none() && profile.is_default)
            .or_else(|| {
                core.provider_profiles
                    .iter()
                    .find(|profile| profile.project_id.is_none())
            })
            .map(|profile| profile.id.clone())
    };
    match profile_id {
        Some(profile_id) => {
            workspace
                .update_provider_profile(profile_id, None, None, config)
                .await?;
        }
        None => {
            workspace
                .create_provider_profile("Default".to_string(), None, true, config)
                .await?;
        }
    }
    workspace.default_provider_status().await
}

#[tauri::command]
async fn discover_local_models(
    endpoint: String,
    api_key: Option<String>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<Vec<String>, String> {
    workspace.discover_models(endpoint, api_key).await
}

#[tauri::command]
fn check_cli_availability(
    executable: String,
    _workspace: tauri::State<'_, Workspace>,
) -> Result<AvailabilityCheck, String> {
    use crate::providers::resolve_executable;
    let path = resolve_executable(&executable);
    match path {
        Some(p) => Ok(AvailabilityCheck {
            ok: true,
            path: Some(p.to_string_lossy().to_string()),
            error: None,
        }),
        None => Ok(AvailabilityCheck {
            ok: false,
            path: None,
            error: Some(format!(
                "Executable '{}' not found on PATH or as a file path.",
                executable
            )),
        }),
    }
}

#[tauri::command]
async fn test_provider_connection(
    endpoint: String,
    model: String,
    api_key: Option<String>,
) -> Result<String, String> {
    connection_test::test_provider_connection(endpoint, model, api_key).await
}

#[tauri::command]
async fn test_cli_agent(executable: String) -> Result<String, String> {
    connection_test::test_cli_agent(executable).await
}

#[tauri::command]
async fn list_registered_agents(
    workspace: tauri::State<'_, Workspace>,
) -> Result<Vec<crate::agent_registry::RegisteredCodingAgent>, String> {
    workspace.list_registered_agents().await
}

#[tauri::command]
async fn refresh_coding_agents(
    workspace: tauri::State<'_, Workspace>,
) -> Result<crate::agent_registry::DetectionReport, String> {
    workspace.refresh_coding_agents().await
}

#[tauri::command]
async fn register_coding_agent(
    workspace: tauri::State<'_, Workspace>,
    input: crate::agent_registry::ManualAgentInput,
) -> Result<crate::agent_registry::RegisteredCodingAgent, String> {
    workspace.register_coding_agent(input).await
}

#[tauri::command]
async fn remove_coding_agent(
    workspace: tauri::State<'_, Workspace>,
    id: String,
) -> Result<(), String> {
    workspace.remove_coding_agent(id).await
}

#[tauri::command]
async fn detect_coding_agents(
    workspace: tauri::State<'_, Workspace>,
) -> Result<crate::agent_registry::DetectionReport, String> {
    workspace.detect_coding_agents().await
}

#[tauri::command]
fn list_cli_agent_presets(workspace: tauri::State<'_, Workspace>) -> Vec<CliAgentPreset> {
    workspace.cli_presets()
}

#[tauri::command]
fn list_local_model_presets(workspace: tauri::State<'_, Workspace>) -> Vec<LocalModelPreset> {
    workspace.local_model_presets()
}

// ---------------------------------------------------------------------------
// Task commands
// ---------------------------------------------------------------------------

#[tauri::command]
async fn start_task(
    user_task: String,
    project_root: String,
    options: Option<StartTaskOptions>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<TaskRecord, String> {
    workspace
        .start_task(user_task, project_root, options.unwrap_or_default())
        .await
}

#[tauri::command]
async fn start_task_from_session(
    session_id: String,
    objective: Option<String>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<TaskRecord, String> {
    workspace
        .start_task_from_session(session_id, objective)
        .await
}

#[tauri::command]
async fn approve_plan(
    task_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<TaskRecord, String> {
    workspace.approve_task(task_id).await
}

#[tauri::command]
async fn reject_plan(
    task_id: String,
    reason: Option<String>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<TaskRecord, String> {
    workspace.reject_task(task_id, reason).await
}

#[tauri::command]
async fn cancel_task(
    task_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<TaskRecord, String> {
    workspace.cancel_task(task_id).await
}

#[tauri::command]
async fn retry_task(
    task_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<TaskRecord, String> {
    workspace.retry_task(task_id).await
}

#[tauri::command]
async fn get_task_status(
    task_id: Option<String>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<Option<TaskRecord>, String> {
    match task_id {
        Some(task_id) => workspace.task_status(&task_id).await,
        None => Ok(workspace.current_task().await),
    }
}

#[tauri::command]
async fn list_task_history(
    workspace: tauri::State<'_, Workspace>,
) -> Result<Vec<TaskRecord>, String> {
    workspace.list_tasks(None).await
}

#[tauri::command]
async fn list_tasks(
    project_id: Option<String>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<Vec<TaskRecord>, String> {
    workspace.list_tasks(project_id).await
}

// ---------------------------------------------------------------------------
// Notification commands
// ---------------------------------------------------------------------------

#[tauri::command]
async fn list_notifications(
    filter: Option<NotificationFilter>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<Vec<NotificationRecord>, String> {
    workspace
        .list_notifications(filter.unwrap_or_default())
        .await
}

#[tauri::command]
async fn get_notification_unread_count(
    project_id: Option<String>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<usize, String> {
    workspace.notification_unread_count(project_id).await
}

#[tauri::command]
async fn mark_notification_read(
    id: String,
    read: bool,
    workspace: tauri::State<'_, Workspace>,
) -> Result<(), String> {
    workspace.mark_notification_read(id, read).await
}

#[tauri::command]
async fn mark_all_notifications_read(
    project_id: Option<String>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<usize, String> {
    workspace.mark_all_notifications_read(project_id).await
}

#[tauri::command]
async fn delete_notification(
    id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<(), String> {
    workspace.delete_notification(id).await
}

#[tauri::command]
async fn clear_notifications(
    project_id: Option<String>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<usize, String> {
    workspace.clear_notifications(project_id).await
}

// ---------------------------------------------------------------------------
// Activity commands
// ---------------------------------------------------------------------------

#[tauri::command]
async fn list_activity(
    filter: Option<ActivityFilter>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<Vec<ActivityRecord>, String> {
    workspace.list_activity(filter.unwrap_or_default()).await
}

// ---------------------------------------------------------------------------
// Report commands
// ---------------------------------------------------------------------------

#[tauri::command]
async fn get_task_report(
    task_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<TaskReport, String> {
    workspace.task_report(task_id).await
}

#[tauri::command]
async fn get_project_report(
    project_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ProjectReport, String> {
    workspace.project_report(project_id).await
}

#[tauri::command]
async fn get_task_timeline(
    task_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<Option<TaskRecord>, String> {
    workspace.task_status(&task_id).await
}

// ---------------------------------------------------------------------------
// Session commands
// ---------------------------------------------------------------------------

#[tauri::command]
async fn create_session(
    options: CreateSessionOptions,
    workspace: tauri::State<'_, Workspace>,
) -> Result<SessionRecord, String> {
    workspace.create_session(options).await
}

#[tauri::command]
async fn list_sessions(
    project_id: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<Vec<SessionRecord>, String> {
    workspace.list_sessions(project_id, limit, offset).await
}

#[tauri::command]
async fn get_session(
    session_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<SessionRecord, String> {
    workspace.get_session(session_id).await
}

#[tauri::command]
async fn validate_session_project(
    session_id: String,
    project_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<(), String> {
    workspace
        .validate_session_project(&session_id, &project_id)
        .await
}

#[tauri::command]
async fn send_session_message(
    session_id: String,
    content: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<SessionRecord, String> {
    workspace.send_session_message(session_id, content).await
}

#[tauri::command]
async fn cancel_cli_session_turn(
    session_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<SessionRecord, String> {
    workspace.cancel_cli_session_turn(session_id).await
}

#[tauri::command]
async fn cancel_prompt_session_turn(
    session_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<SessionRecord, String> {
    workspace.cancel_prompt_session_turn(session_id).await
}

#[tauri::command]
async fn delete_session(
    session_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<(), String> {
    workspace.delete_session(session_id).await
}

#[tauri::command]
async fn archive_session(
    session_id: String,
    archived: bool,
    workspace: tauri::State<'_, Workspace>,
) -> Result<SessionRecord, String> {
    workspace.archive_session(session_id, archived).await
}

#[tauri::command]
async fn update_session(
    session_id: String,
    title: Option<String>,
    project_id: Option<String>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<SessionRecord, String> {
    workspace
        .update_session(session_id, title, project_id)
        .await
}

#[tauri::command]
async fn list_sessions_including_archived(
    project_id: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<Vec<SessionRecord>, String> {
    workspace
        .list_sessions_including_archived(project_id, limit, offset)
        .await
}

#[tauri::command]
async fn get_session_messages(
    session_id: String,
    limit: Option<usize>,
    offset: Option<usize>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<Vec<models::SessionMessage>, String> {
    workspace
        .get_session_messages(session_id, limit, offset)
        .await
}

fn instruction_store_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let base = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    instructions::default_instruction_store_path(&base)
}

// ---------------------------------------------------------------------------
// Instruction Manager commands
// ---------------------------------------------------------------------------

#[tauri::command]
fn list_instructions(
    app: tauri::AppHandle,
    project_root: Option<String>,
    project_id: Option<String>,
    task_id: Option<String>,
) -> Result<Vec<InstructionRecord>, String> {
    let path = instruction_store_path(&app);
    let store = instructions::load_instruction_store(&path);
    let mut out = store.instructions;
    if let Some(ref root) = project_root {
        if !root.trim().is_empty() {
            out.extend(instructions::discover_project_instruction_files(root));
        }
    }
    // Filter view to relevant scope when project/task context given.
    if project_root.is_some() || project_id.is_some() || task_id.is_some() {
        let pr = project_root.as_deref();
        let pid = project_id.as_deref();
        let tid = task_id.as_deref();
        out.retain(|r| match r.scope {
            instructions::InstructionScope::Global => true,
            instructions::InstructionScope::Project => {
                if !r.managed {
                    return true;
                } // discovered shown for the requested project
                if let (Some(a), Some(b)) = (r.project_root.as_deref(), pr) {
                    if dunce::canonicalize(a).ok() == dunce::canonicalize(b).ok() {
                        return true;
                    }
                }
                if let (Some(a), Some(b)) = (r.project_id.as_deref(), pid) {
                    return a == b;
                }
                pr.is_none() && pid.is_none()
            }
            instructions::InstructionScope::Task => {
                if let (Some(a), Some(b)) = (r.task_id.as_deref(), tid) {
                    return a == b;
                }
                false
            }
            instructions::InstructionScope::Agent => {
                if !r.managed {
                    return true;
                }
                if let (Some(a), Some(b)) = (r.project_root.as_deref(), pr) {
                    if dunce::canonicalize(a).ok() == dunce::canonicalize(b).ok() {
                        return true;
                    }
                }
                if let (Some(a), Some(b)) = (r.project_id.as_deref(), pid) {
                    return a == b;
                }
                r.project_root.is_none() && r.project_id.is_none()
            }
        });
    }
    // Stable ordering for UI
    out.sort_by(|a, b| {
        a.priority.cmp(&b.priority).then_with(|| {
            a.title
                .to_ascii_lowercase()
                .cmp(&b.title.to_ascii_lowercase())
        })
    });
    Ok(out)
}

#[tauri::command]
fn upsert_instruction(
    app: tauri::AppHandle,
    input: InstructionUpsertInput,
) -> Result<InstructionRecord, String> {
    let path = instruction_store_path(&app);
    let mut store = instructions::load_instruction_store(&path);
    let rec = instructions::upsert_instruction(&mut store, input)?;
    instructions::save_instruction_store(&path, &store)?;
    Ok(rec)
}

#[tauri::command]
fn delete_instruction(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let path = instruction_store_path(&app);
    let mut store = instructions::load_instruction_store(&path);
    instructions::delete_instruction(&mut store, &id)?;
    instructions::save_instruction_store(&path, &store)?;
    Ok(())
}

#[tauri::command]
fn set_instruction_enabled(
    app: tauri::AppHandle,
    id: String,
    enabled: bool,
) -> Result<InstructionRecord, String> {
    let path = instruction_store_path(&app);
    let mut store = instructions::load_instruction_store(&path);
    let rec = instructions::set_instruction_enabled(&mut store, &id, enabled)?;
    instructions::save_instruction_store(&path, &store)?;
    Ok(rec)
}

#[tauri::command]
fn preview_instruction_application(
    app: tauri::AppHandle,
    project_root: Option<String>,
    project_id: Option<String>,
    agent_kind: Option<String>,
) -> Result<AppliedInstructionBlock, String> {
    let path = instruction_store_path(&app);
    let store = instructions::load_instruction_store(&path);
    Ok(instructions::resolve_instructions(
        &store,
        project_root.as_deref(),
        project_id.as_deref(),
        None,
        agent_kind.as_deref(),
    ))
}

#[tauri::command]
fn discover_project_instructions(project_root: String) -> Result<Vec<InstructionRecord>, String> {
    Ok(instructions::discover_project_instruction_files(
        &project_root,
    ))
}

#[tauri::command]
fn ensure_archify_md(project_root: String) -> Result<bool, String> {
    Ok(instructions::ensure_archify_md(&project_root))
}

#[tauri::command]
fn validate_instruction_store(
    app: tauri::AppHandle,
) -> Result<Vec<InstructionValidationIssue>, String> {
    let path = instruction_store_path(&app);
    let store = instructions::load_instruction_store(&path);
    Ok(instructions::validate_store(&store))
}

// ---------------------------------------------------------------------------
// Skills Manager commands
//
// The Skills Manager is an isolated registry. These commands only manage skill
// definitions and previews; they are not called during task execution. See
// docs/skills-manager-integration.md for the future integration contract.
// ---------------------------------------------------------------------------

fn skill_store_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let base = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    skills::default_skill_store_path(&base)
}

fn skill_library_dir(app: &tauri::AppHandle) -> std::path::PathBuf {
    let base = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    skills::default_skill_library_dir(&base)
}

#[tauri::command]
fn list_skills(
    app: tauri::AppHandle,
    project_root: Option<String>,
) -> Result<Vec<SkillRecord>, String> {
    let path = skill_store_path(&app);
    let library = skill_library_dir(&app);
    let mut store = skills::load_skill_store(&path);
    skills::refresh_validation(&mut store, &library);
    let mut out = store.skills;
    if let Some(ref root) = project_root {
        if !root.trim().is_empty() {
            out.extend(skills::discover_project_skills(root));
        }
    }
    out.sort_by(|a, b| {
        a.managed
            .cmp(&b.managed)
            .reverse()
            .then_with(|| {
                a.category
                    .to_ascii_lowercase()
                    .cmp(&b.category.to_ascii_lowercase())
            })
            .then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
    });
    Ok(out)
}

#[tauri::command]
fn upsert_skill(app: tauri::AppHandle, input: SkillUpsertInput) -> Result<SkillRecord, String> {
    let path = skill_store_path(&app);
    let library = skill_library_dir(&app);
    let mut store = skills::load_skill_store(&path);
    skills::refresh_validation(&mut store, &library);
    let record = skills::upsert_skill(&mut store, &library, input)?;
    skills::save_skill_store(&path, &store)?;
    Ok(record)
}

#[tauri::command]
fn delete_skill(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let path = skill_store_path(&app);
    let library = skill_library_dir(&app);
    let mut store = skills::load_skill_store(&path);
    skills::refresh_validation(&mut store, &library);
    skills::delete_skill(&mut store, &library, &id)?;
    skills::save_skill_store(&path, &store)?;
    Ok(())
}

#[tauri::command]
fn set_skill_enabled(
    app: tauri::AppHandle,
    id: String,
    enabled: bool,
) -> Result<SkillRecord, String> {
    let path = skill_store_path(&app);
    let library = skill_library_dir(&app);
    let mut store = skills::load_skill_store(&path);
    skills::refresh_validation(&mut store, &library);
    let record = skills::set_skill_enabled(&mut store, &id, enabled)?;
    skills::save_skill_store(&path, &store)?;
    Ok(record)
}

#[tauri::command]
fn import_skill(app: tauri::AppHandle, source_path: String) -> Result<SkillRecord, String> {
    let path = skill_store_path(&app);
    let library = skill_library_dir(&app);
    let mut store = skills::load_skill_store(&path);
    skills::refresh_validation(&mut store, &library);
    let record = skills::import_skill(&mut store, &library, &source_path)?;
    skills::save_skill_store(&path, &store)?;
    Ok(record)
}

#[tauri::command]
fn preview_skill_application(
    app: tauri::AppHandle,
    skill_ids: Vec<String>,
    agent_kind: Option<String>,
) -> Result<AppliedSkillBlock, String> {
    let path = skill_store_path(&app);
    let library = skill_library_dir(&app);
    let mut store = skills::load_skill_store(&path);
    skills::refresh_validation(&mut store, &library);
    Ok(skills::resolve_skills_for_agent(
        &store,
        &skill_ids,
        agent_kind.as_deref(),
    ))
}

#[tauri::command]
fn discover_project_skills(project_root: String) -> Result<Vec<SkillRecord>, String> {
    Ok(skills::discover_project_skills(&project_root))
}

#[tauri::command]
fn validate_skill_store(app: tauri::AppHandle) -> Result<Vec<SkillValidationIssue>, String> {
    let path = skill_store_path(&app);
    let library = skill_library_dir(&app);
    let mut store = skills::load_skill_store(&path);
    skills::refresh_validation(&mut store, &library);
    Ok(skills::validate_skill_store(&store, &library))
}

// ---------------------------------------------------------------------------
// Model Selection Commands
// ---------------------------------------------------------------------------

#[tauri::command]
async fn get_ai_responsibilities() -> Vec<AiResponsibility> {
    AiResponsibility::all()
}

#[tauri::command]
async fn list_available_models(
    workspace: tauri::State<'_, Workspace>,
) -> Result<Vec<ModelInfo>, String> {
    workspace.list_available_models().await
}

#[tauri::command]
async fn get_model_preferences(
    workspace: tauri::State<'_, Workspace>,
) -> Result<ModelPreferences, String> {
    workspace.get_model_preferences().await
}

#[tauri::command]
async fn update_model_preferences(
    preferences: ModelPreferences,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ModelPreferences, String> {
    workspace.update_model_preferences(preferences).await
}

#[tauri::command]
async fn select_model_for_task(
    task_id: String,
    model_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ModelSelection, String> {
    workspace.select_model_for_task(task_id, model_id).await
}

#[tauri::command]
async fn select_model_for_responsibility(
    responsibility: AiResponsibility,
    model_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ModelPreferences, String> {
    workspace
        .select_model_for_responsibility(responsibility, model_id)
        .await
}

#[tauri::command]
async fn set_default_model(
    model_id: Option<String>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<ModelPreferences, String> {
    workspace.set_default_model(model_id).await
}

#[tauri::command]
async fn get_model_selection(
    task_id: Option<String>,
    responsibility: Option<AiResponsibility>,
    provider_id: Option<String>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<Option<ModelSelection>, String> {
    workspace
        .get_model_selection(task_id, responsibility, provider_id)
        .await
}

#[tauri::command]
async fn mark_model_deprecated(
    model_id: String,
    deprecated: bool,
    workspace: tauri::State<'_, Workspace>,
) -> Result<(), String> {
    workspace.mark_model_deprecated(model_id, deprecated).await
}

// ---------------------------------------------------------------------------
// Token Usage Commands
// ---------------------------------------------------------------------------

#[tauri::command]
async fn record_token_usage(
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    provider_id: Option<String>,
    provider_name: Option<String>,
    model_id: Option<String>,
    model_name: Option<String>,
    project_id: Option<String>,
    task_id: Option<String>,
    session_id: Option<String>,
    responsibility: Option<String>,
    operation_type: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<TokenUsageRecord, String> {
    workspace
        .record_token_usage(
            input_tokens,
            output_tokens,
            provider_id,
            provider_name,
            model_id,
            model_name,
            project_id,
            task_id,
            session_id,
            responsibility,
            operation_type,
        )
        .await
}

#[tauri::command]
async fn get_token_usage_stats(
    workspace: tauri::State<'_, Workspace>,
) -> Result<TokenUsageStats, String> {
    workspace.get_token_usage_stats().await
}

#[tauri::command]
async fn get_token_usage_by_task(
    task_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<TokenUsageStats, String> {
    workspace.get_token_usage_by_task(task_id).await
}

#[tauri::command]
async fn get_token_usage_by_provider(
    provider_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<TokenUsageStats, String> {
    workspace.get_token_usage_by_provider(provider_id).await
}

#[tauri::command]
async fn get_token_usage_by_model(
    model_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<TokenUsageStats, String> {
    workspace.get_token_usage_by_model(model_id).await
}

#[tauri::command]
async fn get_token_usage_by_project(
    project_id: String,
    workspace: tauri::State<'_, Workspace>,
) -> Result<TokenUsageStats, String> {
    workspace.get_token_usage_by_project(project_id).await
}

#[tauri::command]
async fn get_token_usage_records(
    task_id: Option<String>,
    provider_id: Option<String>,
    model_id: Option<String>,
    project_id: Option<String>,
    start_time: Option<String>,
    end_time: Option<String>,
    workspace: tauri::State<'_, Workspace>,
) -> Result<Vec<TokenUsageRecord>, String> {
    workspace
        .get_token_usage_records(
            task_id,
            provider_id,
            model_id,
            project_id,
            start_time,
            end_time,
        )
        .await
}

#[tauri::command]
async fn clear_token_usage(workspace: tauri::State<'_, Workspace>) -> Result<(), String> {
    workspace.clear_token_usage().await
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let workspace = Workspace::new(Some(app.handle().clone()));
            app.manage(workspace.clone());
            app.manage(terminal::TerminalManager::default());

            // Background scheduler: starts queued tasks as slots free up and
            // resumes tasks that were recovered with a saved plan.
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                    workspace.scheduler_tick().await;
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            list_directory,
            get_workspace,
            update_workspace_settings,
            add_project,
            update_project,
            remove_project,
            archive_project,
            get_project,
            list_projects,
            search_projects,
            update_project_root,
            rename_project,
            get_project_summary,
            create_project,
            select_project,
            detect_missing_project_roots,
            list_provider_profiles,
            create_provider_profile,
            update_provider_profile,
            delete_provider_profile,
            delete_provider_credential,
            get_provider_status,
            get_provider_status_for,
            get_session_provider_status,
            configure_providers,
            discover_local_models,
            list_cli_agent_presets,
            list_local_model_presets,
            check_cli_availability,
            test_provider_connection,
            test_cli_agent,
            list_registered_agents,
            refresh_coding_agents,
            register_coding_agent,
            remove_coding_agent,
            detect_coding_agents,
            start_task,
            start_task_from_session,
            approve_plan,
            reject_plan,
            cancel_task,
            retry_task,
            get_task_status,
            list_task_history,
            list_tasks,
            list_notifications,
            get_notification_unread_count,
            mark_notification_read,
            mark_all_notifications_read,
            delete_notification,
            clear_notifications,
            list_activity,
            get_task_report,
            get_project_report,
            get_task_timeline,
            create_session,
            list_sessions,
            get_session,
            validate_session_project,
            get_session_messages,
            send_session_message,
            cancel_cli_session_turn,
            cancel_prompt_session_turn,
            delete_session,
            archive_session,
            update_session,
            list_sessions_including_archived,
            list_instructions,
            upsert_instruction,
            delete_instruction,
            set_instruction_enabled,
            preview_instruction_application,
            discover_project_instructions,
            ensure_archify_md,
            validate_instruction_store,
            list_skills,
            upsert_skill,
            delete_skill,
            set_skill_enabled,
            import_skill,
            preview_skill_application,
            discover_project_skills,
            validate_skill_store,
            // Model Selection Commands
            get_ai_responsibilities,
            list_available_models,
            get_model_preferences,
            update_model_preferences,
            select_model_for_task,
            select_model_for_responsibility,
            set_default_model,
            get_model_selection,
            mark_model_deprecated,
            // Token Usage Commands
            record_token_usage,
            get_token_usage_stats,
            get_token_usage_by_task,
            get_token_usage_by_provider,
            get_token_usage_by_model,
            get_token_usage_by_project,
            get_token_usage_records,
            clear_token_usage,
            terminal::terminal_spawn,
            terminal::terminal_write,
            terminal::terminal_resize,
            terminal::terminal_history,
            terminal::terminal_close
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // Kill every shell and its descendants before the process exits.
            if let tauri::RunEvent::Exit = event {
                if let Some(manager) = app_handle.try_state::<terminal::TerminalManager>() {
                    manager.close_all();
                }
            }
        });
}

#[cfg(test)]
mod legacy_fs_tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_list_directory_basic() {
        let temp_dir = std::env::current_dir()
            .unwrap()
            .join("test_tauri_explorer_basic");
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(temp_dir.join("subfolder")).unwrap();
        fs::write(temp_dir.join("file1.txt"), "hello").unwrap();
        fs::write(temp_dir.join("file2.txt"), "world").unwrap();

        let root = temp_dir.to_string_lossy().to_string();
        let result = list_directory(root.clone(), None).unwrap();

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].name, "subfolder");
        assert!(result[0].is_dir);

        let sub_result = list_directory(root, Some("subfolder".to_string())).unwrap();
        assert_eq!(sub_result.len(), 0);

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_list_directory_traversal_rejection() {
        let temp_dir = std::env::current_dir()
            .unwrap()
            .join("test_tauri_explorer_traversal");
        let _ = fs::remove_dir_all(&temp_dir);
        let inner = temp_dir.join("inner");
        fs::create_dir_all(&inner).unwrap();
        fs::write(temp_dir.join("secret.txt"), "secret").unwrap();

        let root = inner.to_string_lossy().to_string();
        let result = list_directory(root.clone(), Some("..".to_string()));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Access denied"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_list_directory_symlink_outside_root() {
        let temp_dir = std::env::current_dir()
            .unwrap()
            .join("test_tauri_explorer_symlink");
        let _ = fs::remove_dir_all(&temp_dir);
        let outside = temp_dir.join("outside");
        let root = temp_dir.join("root");
        fs::create_dir_all(&outside).unwrap();
        fs::create_dir_all(&root).unwrap();
        fs::write(outside.join("secret.txt"), "secret").unwrap();

        let link_path = root.join("link_to_outside");
        #[cfg(windows)]
        let link_res = std::os::windows::fs::symlink_dir(&outside, &link_path);
        #[cfg(unix)]
        let link_res = std::os::unix::fs::symlink(&outside, &link_path);

        if link_res.is_ok() {
            let entries = list_directory(root.to_string_lossy().to_string(), None).unwrap();
            assert!(entries.iter().all(|e| e.name != "link_to_outside"));
        }

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
