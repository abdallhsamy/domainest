mod app_state;
pub mod cli;
mod core;
mod domain_suffix;
mod error;
mod models;
mod paths;
mod services;
mod sidecar;
mod state_store;
mod store;
mod tray;

use std::sync::Arc;

use tauri::{Emitter, Manager};

use core::DomainestCore;
use tauri_plugin_opener::OpenerExt;

#[derive(Clone)]
pub struct AppState {
    pub core: Arc<DomainestCore>,
}

impl AppState {
    fn from_core(core: DomainestCore) -> Self {
        Self {
            core: Arc::new(core),
        }
    }
}

fn map_err(e: crate::error::AppError) -> String {
    e.to_string()
}

#[tauri::command]
fn get_domain_suffix(state: tauri::State<'_, AppState>) -> Result<String, String> {
    state.core.get_zone().map_err(map_err)
}

#[tauri::command]
fn set_domain_suffix(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    suffix: String,
) -> Result<String, String> {
    let suffix = state.core.set_zone(&suffix).map_err(map_err)?;
    let _ = app.emit("ui:navigate", "settings");
    Ok(suffix)
}

#[tauri::command]
fn list_projects(state: tauri::State<'_, AppState>) -> Result<Vec<models::Project>, String> {
    state.core.list_projects().map_err(map_err)
}

#[tauri::command]
fn add_project(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    path: String,
) -> Result<models::Project, String> {
    let project = state
        .core
        .add_project(path, core::AddProjectOptions::default())
        .map_err(map_err)?;
    let projects = state.core.list_projects().map_err(map_err)?;
    let _ = tray::refresh_tray(&app, &projects);
    Ok(project)
}

#[tauri::command]
fn remove_project(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    state.core.remove_project(&id).map_err(map_err)?;
    let projects = state.core.list_projects().map_err(map_err)?;
    let _ = tray::refresh_tray(&app, &projects);
    Ok(())
}

#[tauri::command]
fn start_project(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<models::Project, String> {
    let project = state.core.start_project(&id).map_err(map_err)?;
    let projects = state.core.list_projects().map_err(map_err)?;
    let _ = tray::refresh_tray(&app, &projects);
    Ok(project)
}

#[tauri::command]
fn stop_project(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<models::Project, String> {
    let project = state.core.stop_project(&id).map_err(map_err)?;
    let projects = state.core.list_projects().map_err(map_err)?;
    let _ = tray::refresh_tray(&app, &projects);
    Ok(project)
}

#[tauri::command]
fn read_project_log(
    state: tauri::State<'_, AppState>,
    id: String,
    max_bytes: Option<u64>,
) -> Result<String, String> {
    state
        .core
        .read_log(&id, max_bytes.unwrap_or(80_000))
        .map_err(map_err)
}

#[tauri::command]
fn open_project(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let projects = state.core.list_projects().map_err(map_err)?;
    let p = projects
        .iter()
        .find(|p| p.id.to_string() == id)
        .ok_or_else(|| "project not found".to_string())?;
    let scheme = if p.ssl { "https" } else { "http" };
    let url = format!("{scheme}://{}", p.domain);
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn update_project(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    project: models::Project,
) -> Result<models::Project, String> {
    let updated = state.core.update_project(project).map_err(map_err)?;
    let projects = state.core.list_projects().map_err(map_err)?;
    let _ = tray::refresh_tray(&app, &projects);
    Ok(updated)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let core =
                DomainestCore::new().map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!(e)))?;
            app.manage(AppState::from_core(core));

            let state = app.state::<AppState>();
            state
                .core
                .bootstrap()
                .map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!(e)))?;

            let projects = state
                .core
                .list_projects()
                .map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!(e)))?;

            tray::init_tray(app, &projects)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_projects,
            add_project,
            remove_project,
            start_project,
            stop_project,
            open_project,
            update_project,
            read_project_log,
            get_domain_suffix,
            set_domain_suffix
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
