mod error;
mod app_state;
mod models;
mod paths;
mod services;
mod store;
mod state_store;
mod tray;

use std::sync::Arc;

use tauri::{Emitter, Manager};

use services::{
  caddy::CaddyManager,
  dnsmasq::DnsmasqManager,
  mkcert::MkcertManager,
  process_manager::ProcessManager,
};
use store::Store;
use tauri_plugin_opener::OpenerExt;
use uuid::Uuid;
use state_store::StateStore;

fn normalize_suffix(raw: &str) -> Result<String, String> {
  let s = raw.trim().trim_start_matches('.').to_lowercase();
  if s.is_empty() {
    return Err("suffix cannot be empty".to_string());
  }
  if s.len() > 63 {
    return Err("suffix is too long".to_string());
  }
  let ok = s
    .chars()
    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
  if !ok || s.starts_with('-') || s.ends_with('-') {
    return Err("suffix must match [a-z0-9-] and not start/end with '-'".to_string());
  }
  Ok(s)
}

#[tauri::command]
fn get_domain_suffix(state: tauri::State<'_, AppState>) -> Result<String, String> {
  state
    .state_store
    .read()
    .map(|s| normalize_suffix(&s.domain_suffix).unwrap_or_else(|_| "test".to_string()))
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn set_domain_suffix(app: tauri::AppHandle, state: tauri::State<'_, AppState>, suffix: String) -> Result<String, String> {
  let suffix = normalize_suffix(&suffix)?;
  let mut cur = state.state_store.read().map_err(|e| e.to_string())?;
  cur.domain_suffix = suffix.clone();
  state.state_store.write(&cur).map_err(|e| e.to_string())?;

  // Apply DNS routing for the new suffix immediately.
  DnsmasqManager::setup_system(&suffix).map_err(|e| e.to_string())?;
  let _ = app.emit("ui:navigate", "settings");

  Ok(suffix)
}

#[derive(Clone)]
struct AppState {
  store: Store,
  process_manager: Arc<ProcessManager>,
  caddy_manager: Arc<CaddyManager>,
  state_store: StateStore,
}

#[tauri::command]
fn list_projects(state: tauri::State<'_, AppState>) -> Result<Vec<models::Project>, String> {
  state
    .store
    .list_projects()
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn add_project(
  app: tauri::AppHandle,
  state: tauri::State<'_, AppState>,
  path: String,
) -> Result<models::Project, String> {
  let name = std::path::Path::new(&path)
    .file_name()
    .and_then(|s| s.to_str())
    .unwrap_or("project")
    .to_string();
  let suffix = state
    .state_store
    .read()
    .map(|s| s.domain_suffix)
    .unwrap_or_else(|_| "test".to_string());
  let suffix = suffix.trim_start_matches('.').to_lowercase();
  let domain = format!("{name}.{suffix}");

  let mut projects = state.store.list_projects().map_err(|e| e.to_string())?;
  let used_ports = projects.iter().map(|p| p.port).collect::<std::collections::HashSet<_>>();
  let mut port: u16 = 3000;
  while used_ports.contains(&port) {
    port = port.saturating_add(1);
    if port == u16::MAX {
      return Err("no available port".to_string());
    }
  }
  let project = models::Project {
    id: Uuid::new_v4(),
    name,
    path,
    domain,
    port,
    ssl: true,
    status: models::ProjectStatus::Stopped,
    command: "pnpm".to_string(),
    args: vec!["dev".to_string()],
    pid: None,
  };

  projects.push(project.clone());
  state
    .store
    .save_projects(&projects)
    .map_err(|e| e.to_string())?;
  let _ = tray::refresh_tray(&app, &projects);
  Ok(project)
}

#[tauri::command]
fn remove_project(
  app: tauri::AppHandle,
  state: tauri::State<'_, AppState>,
  id: String,
) -> Result<(), String> {
  let project_id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
  let mut projects = state.store.list_projects().map_err(|e| e.to_string())?;

  if let Some(p) = projects.iter().find(|p| p.id == project_id).cloned() {
    if p.status == models::ProjectStatus::Running {
      let _ = state
        .process_manager
        .stop(project_id, std::time::Duration::from_secs(5));
    }
  }

  projects.retain(|p| p.id != project_id);
  state
    .store
    .save_projects(&projects)
    .map_err(|e| e.to_string())?;

  apply_caddy_config(&app, &state, &projects).map_err(|e| e.to_string())?;
  let _ = tray::refresh_tray(&app, &projects);
  Ok(())
}

#[tauri::command]
fn start_project(
  app: tauri::AppHandle,
  state: tauri::State<'_, AppState>,
  id: String,
) -> Result<models::Project, String> {
  let project_id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;

  let suffix = state
    .state_store
    .read()
    .map(|s| s.domain_suffix)
    .unwrap_or_else(|_| "test".to_string());
  DnsmasqManager::setup_system(&suffix).map_err(|e| e.to_string())?;

  MkcertManager::install_local_ca(&app, &state.state_store).map_err(|e| e.to_string())?;

  let mut projects = state.store.list_projects().map_err(|e| e.to_string())?;
  let mut project = projects
    .iter()
    .find(|p| p.id == project_id)
    .cloned()
    .ok_or_else(|| "project not found".to_string())?;

  if project.ssl {
    let _ = MkcertManager::ensure_cert(&app, &project.domain).map_err(|e| e.to_string())?;
  }

  let pid = state
    .process_manager
    .spawn_dev_server(&project)
    .map_err(|e| e.to_string())?;

  for p in projects.iter_mut() {
    if p.id == project_id {
      p.status = models::ProjectStatus::Running;
      p.pid = Some(pid);
      project = p.clone();
      break;
    }
  }

  state
    .store
    .save_projects(&projects)
    .map_err(|e| e.to_string())?;

  apply_caddy_config(&app, &state, &projects).map_err(|e| e.to_string())?;
  let _ = tray::refresh_tray(&app, &projects);
  Ok(project)
}

#[tauri::command]
fn stop_project(
  app: tauri::AppHandle,
  state: tauri::State<'_, AppState>,
  id: String,
) -> Result<models::Project, String> {
  let project_id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;

  let mut projects = state.store.list_projects().map_err(|e| e.to_string())?;
  let mut project = projects
    .iter()
    .find(|p| p.id == project_id)
    .cloned()
    .ok_or_else(|| "project not found".to_string())?;

  // Stop the tracked child if present; if the app restarted and lost the child handle,
  // fall back to killing by the last known pid.
  if let Err(e) = state
    .process_manager
    .stop(project_id, std::time::Duration::from_secs(5))
  {
    if let Some(pid) = project.pid {
      let _ = state
        .process_manager
        .stop_by_pid(pid, std::time::Duration::from_secs(5));
    } else {
      return Err(e.to_string());
    }
  }

  for p in projects.iter_mut() {
    if p.id == project_id {
      p.status = models::ProjectStatus::Stopped;
      p.pid = None;
      project = p.clone();
      break;
    }
  }

  state
    .store
    .save_projects(&projects)
    .map_err(|e| e.to_string())?;

  apply_caddy_config(&app, &state, &projects).map_err(|e| e.to_string())?;
  let _ = tray::refresh_tray(&app, &projects);
  Ok(project)
}

#[tauri::command]
fn read_project_log(_state: tauri::State<'_, AppState>, id: String, max_bytes: Option<u64>) -> Result<String, String> {
  let project_id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
  let log_path = paths::logs_dir()
    .map_err(|e| e.to_string())?
    .join(format!("{project_id}.log"));
  let max = max_bytes.unwrap_or(80_000).min(500_000);
  let data = std::fs::read(&log_path).unwrap_or_default();
  if (data.len() as u64) <= max {
    return Ok(String::from_utf8_lossy(&data).to_string());
  }
  let start = (data.len() as u64 - max) as usize;
  Ok(String::from_utf8_lossy(&data[start..]).to_string())
}

fn apply_caddy_config(
  app: &tauri::AppHandle,
  state: &tauri::State<'_, AppState>,
  projects: &[models::Project],
) -> Result<(), crate::error::AppError> {
  let caddyfile = state.caddy_manager.write_managed_caddyfile(projects, &|domain| {
    let certs_dir = paths::certs_dir().ok()?;
    let cert = certs_dir.join(format!("{domain}.pem"));
    let key = certs_dir.join(format!("{domain}-key.pem"));
    if cert.exists() && key.exists() {
      Some((cert, key))
    } else {
      None
    }
  })?;

  state.caddy_manager.ensure_running(app, &caddyfile)?;
  state.caddy_manager.reload(app, &caddyfile)?;
  Ok(())
}

#[tauri::command]
fn open_project(app: tauri::AppHandle, state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
  let project_id = Uuid::parse_str(&id).map_err(|e| e.to_string())?;
  let projects = state.store.list_projects().map_err(|e| e.to_string())?;
  let p = projects
    .iter()
    .find(|p| p.id == project_id)
    .ok_or_else(|| "project not found".to_string())?;

  let scheme = if p.ssl { "https" } else { "http" };
  let url = format!("{scheme}://{}", p.domain);
  app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())?;
  Ok(())
}

#[tauri::command]
fn update_project(
  app: tauri::AppHandle,
  state: tauri::State<'_, AppState>,
  project: models::Project,
) -> Result<models::Project, String> {
  let mut projects = state.store.list_projects().map_err(|e| e.to_string())?;

  let mut updated = None;
  for p in projects.iter_mut() {
    if p.id == project.id {
      *p = project.clone();
      updated = Some(p.clone());
      break;
    }
  }

  let updated = updated.ok_or_else(|| "project not found".to_string())?;
  state
    .store
    .save_projects(&projects)
    .map_err(|e| e.to_string())?;
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

      let store = Store::new().map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!(e)))?;
      app.manage(AppState {
        store,
        process_manager: Arc::new(ProcessManager::new()),
        caddy_manager: Arc::new(CaddyManager::new()),
        state_store: StateStore::new().map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!(e)))?,
      });

      let state = app.state::<AppState>();
      reconcile_projects_on_start(&state.store)
        .map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!(e)))?;
      let projects = state
        .store
        .list_projects()
        .map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!(e)))?;

      let handle = app.handle();

      // Ensure wildcard `.test` resolution + local CA are ready up-front so domains
      // resolve immediately when opening a project in the browser.
      let suffix = state
        .state_store
        .read()
        .map(|s| s.domain_suffix)
        .unwrap_or_else(|_| "test".to_string());
      DnsmasqManager::setup_system(&suffix).map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!(e)))?;
      MkcertManager::install_local_ca(&handle, &state.state_store)
        .map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!(e)))?;

      // Always rewrite managed Caddy config on startup to avoid stale domains
      // lingering after project removal or crashes.
      let caddyfile = state
        .caddy_manager
        .write_managed_caddyfile(&projects, &|domain| {
          let certs_dir = paths::certs_dir().ok()?;
          let cert = certs_dir.join(format!("{domain}.pem"));
          let key = certs_dir.join(format!("{domain}-key.pem"));
          if cert.exists() && key.exists() {
            Some((cert, key))
          } else {
            None
          }
        })
        .map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!(e)))?;
      // Start caddy even when there are 0 projects so we can immediately apply
      // removals (and so opening projects later is fast).
      state
        .caddy_manager
        .ensure_running(&handle, &caddyfile)
        .map_err(|e| tauri::Error::Anyhow(anyhow::anyhow!(e)))?;
      state
        .caddy_manager
        .reload(&handle, &caddyfile)
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

fn reconcile_projects_on_start(store: &Store) -> Result<(), crate::error::AppError> {
  let mut projects = store.list_projects()?;
  let mut changed = false;

  for p in projects.iter_mut() {
    if p.status == models::ProjectStatus::Running {
      let alive = p.pid.and_then(|pid| is_pid_alive(pid)).unwrap_or(false);
      if !alive {
        p.status = models::ProjectStatus::Stopped;
        p.pid = None;
        changed = true;
      }
    }
  }

  if changed {
    store.save_projects(&projects)?;
  }
  Ok(())
}

fn is_pid_alive(pid: u32) -> Option<bool> {
  #[cfg(unix)]
  {
    use nix::sys::signal::kill;
    use nix::unistd::Pid;
    let p = Pid::from_raw(pid as i32);
    match kill(p, None) {
      Ok(()) => Some(true),
      Err(nix::errno::Errno::ESRCH) => Some(false),
      Err(_) => Some(false),
    }
  }

  #[cfg(not(unix))]
  {
    let _ = pid;
    None
  }
}
