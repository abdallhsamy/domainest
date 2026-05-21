mod resolve;

use std::{collections::HashSet, process::Command, sync::Arc, time::Duration};

pub use resolve::resolve_project;
use uuid::Uuid;

use crate::{
    domain_suffix,
    error::{AppError, AppResult},
    models::{Project, ProjectStatus},
    paths,
    services::{
        caddy::CaddyManager, dnsmasq::DnsmasqManager, mkcert::MkcertManager,
        process_manager::ProcessManager,
    },
    state_store::StateStore,
    store::Store,
};

#[derive(Default)]
pub struct AddProjectOptions {
    pub domain: Option<String>,
    pub port: Option<u16>,
    pub ssl: Option<bool>,
}

pub struct DomainestStatus {
    pub zone: String,
    pub projects: Vec<Project>,
    pub running_count: usize,
}

pub struct DomainestCore {
    pub store: Store,
    pub process_manager: Arc<ProcessManager>,
    pub caddy_manager: Arc<CaddyManager>,
    pub state_store: StateStore,
}

impl DomainestCore {
    pub fn new() -> AppResult<Self> {
        Ok(Self {
            store: Store::new()?,
            process_manager: Arc::new(ProcessManager::new()),
            caddy_manager: Arc::new(CaddyManager::new()),
            state_store: StateStore::new()?,
        })
    }

    pub fn list_projects(&self) -> AppResult<Vec<Project>> {
        self.store.list_projects()
    }

    pub fn status(&self) -> AppResult<DomainestStatus> {
        let zone = self.ensure_valid_zone_in_store()?;
        let projects = self.store.list_projects()?;
        let running_count = projects
            .iter()
            .filter(|p| p.status == ProjectStatus::Running)
            .count();
        Ok(DomainestStatus {
            zone,
            projects,
            running_count,
        })
    }

    pub fn add_project(&self, path: String, opts: AddProjectOptions) -> AppResult<Project> {
        let name = std::path::Path::new(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("project")
            .to_string();
        let zone = self.ensure_valid_zone_in_store()?;
        let domain = opts.domain.unwrap_or_else(|| format!("{name}.{zone}"));
        let domain = domain.trim().trim_start_matches('.').to_lowercase();

        let mut projects = self.store.list_projects()?;
        let used_ports: HashSet<u16> = projects.iter().map(|p| p.port).collect();
        let port = opts.port.unwrap_or_else(|| {
            let mut port: u16 = 3000;
            while used_ports.contains(&port) {
                port = port.saturating_add(1);
            }
            port
        });

        let project = Project {
            id: Uuid::new_v4(),
            name,
            path,
            domain,
            port,
            ssl: opts.ssl.unwrap_or(true),
            status: ProjectStatus::Stopped,
            command: "pnpm".to_string(),
            args: vec!["dev".to_string()],
            pid: None,
        };

        projects.push(project.clone());
        self.store.save_projects(&projects)?;
        self.sync_dns()?;
        Ok(project)
    }

    pub fn remove_project(&self, selector: &str) -> AppResult<()> {
        let project_id = resolve_project(&self.store, selector)?.id;
        let mut projects = self.store.list_projects()?;

        if let Some(p) = projects.iter().find(|p| p.id == project_id).cloned() {
            if p.status == ProjectStatus::Running {
                let _ = self
                    .process_manager
                    .stop(project_id, Duration::from_secs(5));
            }
        }

        projects.retain(|p| p.id != project_id);
        self.store.save_projects(&projects)?;
        self.apply_caddy_config(&projects)?;
        self.sync_dns()?;
        Ok(())
    }

    pub fn update_project(&self, project: Project) -> AppResult<Project> {
        let mut projects = self.store.list_projects()?;
        let mut updated = None;
        for p in projects.iter_mut() {
            if p.id == project.id {
                *p = project.clone();
                updated = Some(p.clone());
                break;
            }
        }
        let updated = updated.ok_or_else(|| AppError::ToolFailed {
            tool: "project".to_string(),
            message: "project not found".to_string(),
        })?;
        self.store.save_projects(&projects)?;
        self.sync_dns()?;
        Ok(updated)
    }

    pub fn start_project(&self, selector: &str) -> AppResult<Project> {
        self.reconcile_projects_on_start()?;
        let project_id = resolve_project(&self.store, selector)?.id;

        self.sync_dns()?;
        MkcertManager::install_local_ca(&self.state_store)?;

        let mut projects = self.store.list_projects()?;
        let mut project = projects
            .iter()
            .find(|p| p.id == project_id)
            .cloned()
            .ok_or_else(|| AppError::ToolFailed {
                tool: "project".to_string(),
                message: "project not found".to_string(),
            })?;

        if project.ssl {
            let _ = MkcertManager::ensure_cert(&project.domain)?;
        }

        let pid = self.process_manager.spawn_dev_server(&project)?;

        for p in projects.iter_mut() {
            if p.id == project_id {
                p.status = ProjectStatus::Running;
                p.pid = Some(pid);
                project = p.clone();
                break;
            }
        }

        self.store.save_projects(&projects)?;
        self.apply_caddy_config(&projects)?;
        Ok(project)
    }

    pub fn stop_project(&self, selector: &str) -> AppResult<Project> {
        let project_id = resolve_project(&self.store, selector)?.id;
        let mut projects = self.store.list_projects()?;
        let mut project = projects
            .iter()
            .find(|p| p.id == project_id)
            .cloned()
            .ok_or_else(|| AppError::ToolFailed {
                tool: "project".to_string(),
                message: "project not found".to_string(),
            })?;

        if let Err(e) = self
            .process_manager
            .stop(project_id, Duration::from_secs(5))
        {
            if let Some(pid) = project.pid {
                let _ = self
                    .process_manager
                    .stop_by_pid(pid, Duration::from_secs(5));
            } else {
                return Err(e);
            }
        }

        for p in projects.iter_mut() {
            if p.id == project_id {
                p.status = ProjectStatus::Stopped;
                p.pid = None;
                project = p.clone();
                break;
            }
        }

        self.store.save_projects(&projects)?;
        self.apply_caddy_config(&projects)?;
        Ok(project)
    }

    pub fn open_project(&self, selector: &str) -> AppResult<()> {
        let p = resolve_project(&self.store, selector)?;
        let scheme = if p.ssl { "https" } else { "http" };
        let url = format!("{scheme}://{}", p.domain);
        open_url(&url)
    }

    pub fn read_log(&self, selector: &str, max_bytes: u64) -> AppResult<String> {
        let p = resolve_project(&self.store, selector)?;
        let log_path = paths::logs_dir()?.join(format!("{}.log", p.id));
        let max = max_bytes.min(500_000);
        let data = std::fs::read(&log_path).unwrap_or_default();
        if (data.len() as u64) <= max {
            return Ok(String::from_utf8_lossy(&data).to_string());
        }
        let start = (data.len() as u64 - max) as usize;
        Ok(String::from_utf8_lossy(&data[start..]).to_string())
    }

    pub fn get_zone(&self) -> AppResult<String> {
        let state = self.state_store.read()?;
        Ok(domain_suffix::normalize_dns_zone(&state.domain_suffix)
            .unwrap_or_else(|_| "test".to_string()))
    }

    pub fn set_zone(&self, raw: &str) -> AppResult<String> {
        let zone = domain_suffix::normalize_dns_zone(raw).map_err(|e| AppError::ToolFailed {
            tool: "zone".to_string(),
            message: e,
        })?;
        domain_suffix::validate_dns_zone(&zone).map_err(|e| AppError::ToolFailed {
            tool: "zone".to_string(),
            message: e,
        })?;
        let mut cur = self.state_store.read()?;
        cur.domain_suffix = zone.clone();
        self.state_store.write(&cur)?;
        self.sync_dns()?;
        Ok(zone)
    }

    pub fn sync_dns(&self) -> AppResult<()> {
        let zone = self.ensure_valid_zone_in_store()?;
        let projects = self.store.list_projects()?;
        let domains: Vec<String> = projects.iter().map(|p| p.domain.clone()).collect();
        DnsmasqManager::setup_system(&zone, &domains)
    }

    pub fn bootstrap(&self) -> AppResult<()> {
        self.reconcile_projects_on_start()?;
        let projects = self.store.list_projects()?;
        self.sync_dns()?;
        MkcertManager::install_local_ca(&self.state_store)?;
        let caddyfile = self.write_caddyfile(&projects)?;
        self.caddy_manager.ensure_running(&caddyfile)?;
        self.caddy_manager.reload(&caddyfile)?;
        Ok(())
    }

    fn ensure_valid_zone_in_store(&self) -> AppResult<String> {
        let mut cur = self.state_store.read()?;
        let normalized = domain_suffix::normalize_dns_zone(&cur.domain_suffix)
            .unwrap_or_else(|_| "test".to_string());
        let zone = if domain_suffix::validate_dns_zone(&normalized).is_ok() {
            normalized
        } else {
            "test".to_string()
        };
        if cur.domain_suffix != zone {
            cur.domain_suffix = zone.clone();
            self.state_store.write(&cur)?;
        }
        Ok(zone)
    }

    fn write_caddyfile(&self, projects: &[Project]) -> AppResult<std::path::PathBuf> {
        self.caddy_manager
            .write_managed_caddyfile(projects, &tls_paths_for_domain)
    }

    fn apply_caddy_config(&self, projects: &[Project]) -> AppResult<()> {
        let caddyfile = self.write_caddyfile(projects)?;
        self.caddy_manager.ensure_running(&caddyfile)?;
        self.caddy_manager.reload(&caddyfile)?;
        Ok(())
    }

    pub fn reconcile_projects_on_start(&self) -> AppResult<()> {
        let mut projects = self.store.list_projects()?;
        let mut changed = false;

        for p in projects.iter_mut() {
            if p.status == ProjectStatus::Running {
                let alive = p.pid.and_then(|pid| is_pid_alive(pid)).unwrap_or(false);
                if !alive {
                    p.status = ProjectStatus::Stopped;
                    p.pid = None;
                    changed = true;
                }
            }
        }

        if changed {
            self.store.save_projects(&projects)?;
        }
        Ok(())
    }
}

fn tls_paths_for_domain(domain: &str) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    let certs_dir = paths::certs_dir().ok()?;
    let cert = certs_dir.join(format!("{domain}.pem"));
    let key = certs_dir.join(format!("{domain}-key.pem"));
    if cert.exists() && key.exists() {
        Some((cert, key))
    } else {
        None
    }
}

fn open_url(url: &str) -> AppResult<()> {
    let status = {
        #[cfg(target_os = "macos")]
        {
            Command::new("open").arg(url).status()
        }
        #[cfg(target_os = "linux")]
        {
            Command::new("xdg-open").arg(url).status()
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "open URL not supported on this OS",
            ))
        }
    }
    .map_err(AppError::Io)?;

    if status.success() {
        Ok(())
    } else {
        Err(AppError::ToolFailed {
            tool: "open".to_string(),
            message: format!("failed to open {url}"),
        })
    }
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
