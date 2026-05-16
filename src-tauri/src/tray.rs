use tauri::{
    menu::{Menu, MenuItem, Submenu},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};
use uuid::Uuid;

use crate::models::Project;
use crate::{paths, services::mkcert::MkcertManager, AppState};
use tauri::Emitter;
use tauri_plugin_opener::OpenerExt;

const TRAY_ID: &str = "domainest-tray";

pub fn init_tray(app: &tauri::App, projects: &[Project]) -> tauri::Result<()> {
    let menu = build_menu(app.handle(), projects)?;

    TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("Domainest")
        .icon(tauri::include_image!("icons/32x32.png"))
        .menu(&menu)
        .on_menu_event(|app, event| {
            let id = event.id.as_ref();
            handle_menu_event(app, id);
        })
        .build(app)?;

    Ok(())
}

pub fn refresh_tray(app: &AppHandle, projects: &[Project]) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return Ok(());
    };

    let menu = build_menu(app, projects)?;
    tray.set_menu(Some(menu))?;
    Ok(())
}

fn build_menu(app: &AppHandle, projects: &[Project]) -> tauri::Result<Menu<tauri::Wry>> {
    let menu = Menu::new(app)?;

    let projects_item = MenuItem::with_id(app, "nav_projects", "Projects", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "nav_settings", "Settings", true, None::<&str>)?;
    let about_item = MenuItem::with_id(app, "nav_about", "About", true, None::<&str>)?;
    menu.append(&projects_item)?;
    menu.append(&settings_item)?;
    menu.append(&about_item)?;
    menu.append(&tauri::menu::PredefinedMenuItem::separator(app)?)?;

    for p in projects {
        let (toggle_id, toggle_label) = match p.status {
            crate::models::ProjectStatus::Running => (format!("project_stop:{}", p.id), "Stop"),
            crate::models::ProjectStatus::Stopped => (format!("project_start:{}", p.id), "Start"),
        };

        let toggle = MenuItem::with_id(app, toggle_id, toggle_label, true, None::<&str>)?;
        let open = MenuItem::with_id(
            app,
            format!("project_open:{}", p.id),
            "Open in Browser",
            true,
            None::<&str>,
        )?;

        let sub = Submenu::with_id_and_items(
            app,
            format!("project:{}", p.id),
            format!("{} ({})", p.name, p.domain),
            true,
            &[&toggle, &open],
        )?;

        menu.append(&sub)?;
    }

    if !projects.is_empty() {
        menu.append(&tauri::menu::PredefinedMenuItem::separator(app)?)?;
    }

    let add_project = MenuItem::with_id(app, "add_project", "Add Project", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;

    menu.append(&add_project)?;
    menu.append(&tauri::menu::PredefinedMenuItem::separator(app)?)?;
    menu.append(&quit)?;

    Ok(menu)
}

fn handle_menu_event(app: &AppHandle, id: &str) {
    match id {
        "quit" => {
            app.exit(0);
        }
        "nav_projects" => {
            let _ = open_or_focus_dashboard(app);
            let _ = app.emit("ui:navigate", "projects");
        }
        "nav_settings" => {
            let _ = open_or_focus_dashboard(app);
            let _ = app.emit("ui:navigate", "settings");
        }
        "nav_about" => {
            let _ = open_or_focus_dashboard(app);
            let _ = app.emit("ui:navigate", "about");
        }
        "add_project" => {
            let _ = open_or_focus_dashboard(app);
            let _ = app.emit("ui:navigate", "projects");
            let _ = app.emit("ui:add_project", ());
        }
        _ => {
            if let Some((action, uuid)) = id.split_once(':') {
                if let Ok(pid) = Uuid::parse_str(uuid) {
                    match action {
                        "project_start" => {
                            tray_start_project(app, pid);
                        }
                        "project_stop" => {
                            tray_stop_project(app, pid);
                        }
                        "project_open" => {
                            tray_open_project(app, pid);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}

fn tray_open_project(app: &AppHandle, project_id: Uuid) {
    let state = app.state::<AppState>();
    let projects = match state.store.list_projects() {
        Ok(p) => p,
        Err(_) => return,
    };
    let Some(p) = projects.iter().find(|p| p.id == project_id) else {
        return;
    };
    let scheme = if p.ssl { "https" } else { "http" };
    let url = format!("{scheme}://{}", p.domain);
    let _ = app.opener().open_url(url, None::<&str>);
}

fn tray_start_project(app: &AppHandle, project_id: Uuid) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let _ = crate::sync_dns(&state);
        let _ = MkcertManager::install_local_ca(&app, &state.state_store);

        let mut projects = match state.store.list_projects() {
            Ok(p) => p,
            Err(_) => return,
        };
        let project = match projects.iter().find(|p| p.id == project_id).cloned() {
            Some(p) => p,
            None => return,
        };

        if project.ssl {
            let _ = MkcertManager::ensure_cert(&app, &project.domain);
        }

        let pid = match state.process_manager.spawn_dev_server(&project) {
            Ok(pid) => pid,
            Err(_) => return,
        };

        for p in projects.iter_mut() {
            if p.id == project_id {
                p.status = crate::models::ProjectStatus::Running;
                p.pid = Some(pid);
            }
        }
        let _ = state.store.save_projects(&projects);

        let _ = state
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
            .and_then(|caddyfile| {
                state.caddy_manager.ensure_running(&app, &caddyfile)?;
                state.caddy_manager.reload(&app, &caddyfile)?;
                Ok(())
            });

        let _ = refresh_tray(&app, &projects);
    });
}

fn tray_stop_project(app: &AppHandle, project_id: Uuid) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let _ = state
            .process_manager
            .stop(project_id, std::time::Duration::from_secs(5));

        let mut projects = match state.store.list_projects() {
            Ok(p) => p,
            Err(_) => return,
        };
        for p in projects.iter_mut() {
            if p.id == project_id {
                p.status = crate::models::ProjectStatus::Stopped;
                p.pid = None;
            }
        }
        let _ = state.store.save_projects(&projects);

        let _ = state
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
            .and_then(|caddyfile| {
                state.caddy_manager.ensure_running(&app, &caddyfile)?;
                state.caddy_manager.reload(&app, &caddyfile)?;
                Ok(())
            });

        let _ = refresh_tray(&app, &projects);
    });
}

fn open_or_focus_dashboard(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("main") {
        w.show()?;
        w.set_focus()?;
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
        .title("Domainest")
        .inner_size(900.0, 700.0)
        .build()?;

    Ok(())
}
