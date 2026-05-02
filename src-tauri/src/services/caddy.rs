use std::{
    fs,
    net::{SocketAddr, TcpStream},
    path::PathBuf,
    sync::Mutex,
    time::{Duration, Instant},
};

use tauri_plugin_shell::{process::CommandEvent, ShellExt};

use crate::{
    error::{AppError, AppResult},
    models::Project,
    paths,
};

const GENERATED_BEGIN: &str = "# --- BEGIN dev-domains managed ---";
const GENERATED_END: &str = "# --- END dev-domains managed ---";
const ADMIN_ADDR: &str = "127.0.0.1:2019";

pub struct CaddyManager {
    running: Mutex<Option<tauri_plugin_shell::process::CommandChild>>,
}

impl CaddyManager {
    pub fn new() -> Self {
        Self {
            running: Mutex::new(None),
        }
    }

    pub fn caddyfile_path() -> AppResult<PathBuf> {
        Ok(paths::dev_domains_dir()?.join("Caddyfile"))
    }

    pub fn write_managed_caddyfile(
        &self,
        projects: &[Project],
        tls_paths: &TlsPathLookup,
    ) -> AppResult<PathBuf> {
        let path = Self::caddyfile_path()?;
        let parent = path.parent().unwrap();
        fs::create_dir_all(parent)?;

        let managed = render_managed_block(projects, tls_paths);
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let next = upsert_managed_block(&existing, &managed);

        fs::write(&path, next)?;
        Ok(path)
    }

    fn pidfile_path() -> AppResult<PathBuf> {
        Ok(paths::caddy_dir()?.join("caddy.pid"))
    }

    pub fn ensure_running(&self, app: &tauri::AppHandle, caddyfile: &PathBuf) -> AppResult<()> {
        let mut guard = self.running.lock().unwrap();
        if guard.is_some() {
            return Ok(());
        }

        let caddy_dir = paths::caddy_dir()?;
        fs::create_dir_all(&caddy_dir)?;

        // Best-effort stop of any previously running Caddy on the default admin address.
        // This prevents orphaned instances (from older versions without a pidfile) from continuing
        // to serve stale config.
        let _ = tauri::async_runtime::block_on(async {
            if let Ok(cmd) = app.shell().sidecar("caddy") {
                let _ = cmd.args(["stop", "--address", ADMIN_ADDR]).output().await;
            }
        });

        // If we have a previous pidfile, try to terminate that instance first.
        if let Ok(pidfile) = Self::pidfile_path() {
            if let Ok(pid_str) = fs::read_to_string(&pidfile) {
                if let Ok(pid) = pid_str.trim().parse::<i32>() {
                    #[cfg(unix)]
                    {
                        use nix::sys::signal::kill;
                        use nix::unistd::Pid;
                        let _ = kill(Pid::from_raw(pid), nix::sys::signal::Signal::SIGTERM);
                    }
                }
            }
        }

        let mut cmd = app
            .shell()
            .sidecar("caddy")
            .map_err(|e| AppError::ToolFailed {
                tool: "caddy".to_string(),
                message: e.to_string(),
            })?;
        cmd = cmd.args([
            "run",
            "--config",
            caddyfile.to_string_lossy().as_ref(),
            "--adapter",
            "caddyfile",
            "--pidfile",
            Self::pidfile_path()?.to_string_lossy().as_ref(),
        ]);
        cmd = cmd.env("CADDY_HOME", caddy_dir.to_string_lossy().as_ref());
        cmd = cmd.env("XDG_DATA_HOME", caddy_dir.to_string_lossy().as_ref());

        let (mut rx, child) = cmd.spawn().map_err(|e| AppError::ToolFailed {
            tool: "caddy".to_string(),
            message: e.to_string(),
        })?;

        wait_for_admin_ready(ADMIN_ADDR, Duration::from_secs(3))?;

        tauri::async_runtime::spawn(async move {
            while let Some(event) = rx.recv().await {
                match event {
                    CommandEvent::Error(line) => {
                        log::error!("caddy: {line}");
                    }
                    CommandEvent::Stderr(line) => {
                        log::warn!("caddy: {}", String::from_utf8_lossy(&line));
                    }
                    CommandEvent::Stdout(line) => {
                        log::info!("caddy: {}", String::from_utf8_lossy(&line));
                    }
                    _ => {}
                }
            }
        });

        *guard = Some(child);
        Ok(())
    }

    pub fn reload(&self, app: &tauri::AppHandle, caddyfile: &PathBuf) -> AppResult<()> {
        let caddyfile = caddyfile.clone();
        let out = tauri::async_runtime::block_on(async move {
            app.shell()
                .sidecar("caddy")
                .map_err(|e| AppError::ToolFailed {
                    tool: "caddy".to_string(),
                    message: e.to_string(),
                })?
                .args([
                    "reload",
                    "--config",
                    caddyfile.to_string_lossy().as_ref(),
                    "--adapter",
                    "caddyfile",
                    "--address",
                    ADMIN_ADDR,
                ])
                .output()
                .await
                .map_err(|e| AppError::ToolFailed {
                    tool: "caddy reload".to_string(),
                    message: e.to_string(),
                })
        })?;

        if !out.status.success() {
            return Err(AppError::ToolFailed {
                tool: "caddy reload".to_string(),
                message: format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                )
                .trim()
                .to_string(),
            });
        }

        Ok(())
    }
}

pub type TlsPathLookup = dyn Fn(&str) -> Option<(PathBuf, PathBuf)> + Send + Sync;

fn wait_for_admin_ready(addr: &str, timeout: Duration) -> AppResult<()> {
    let addr: SocketAddr = addr.parse().map_err(|e| AppError::ToolFailed {
        tool: "caddy admin".to_string(),
        message: format!("invalid admin address {addr}: {e}"),
    })?;

    let start = Instant::now();
    while start.elapsed() < timeout {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(200)).is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    Err(AppError::ToolFailed {
        tool: "caddy admin".to_string(),
        message: format!(
            "admin endpoint did not become ready at {addr} within {}ms",
            timeout.as_millis()
        ),
    })
}

fn render_managed_block(projects: &[Project], tls_paths: &TlsPathLookup) -> String {
    let mut out = String::new();
    out.push_str(GENERATED_BEGIN);
    out.push('\n');

    for p in projects {
        if p.status != crate::models::ProjectStatus::Running {
            continue;
        }
        if p.ssl {
            let Some((cert, key)) = tls_paths(&p.domain) else {
                continue;
            };
            out.push_str(&format!(
        "{domain} {{\n  reverse_proxy localhost:{port} {{\n    header_up Host localhost:{port}\n    header_up -X-Forwarded-Host\n    header_up X-Forwarded-Proto https\n  }}\n  tls {cert} {key}\n}}\n\n",
        domain = p.domain,
        port = p.port,
        cert = cert.to_string_lossy(),
        key = key.to_string_lossy()
      ));
        } else {
            out.push_str(&format!(
        "http://{domain} {{\n  header {{\n    -Cross-Origin-Opener-Policy\n    -Origin-Agent-Cluster\n    -Cross-Origin-Embedder-Policy\n    -Cross-Origin-Resource-Policy\n    -Content-Security-Policy\n    -Content-Security-Policy-Report-Only\n    -Strict-Transport-Security\n  }}\n  reverse_proxy localhost:{port} {{\n    header_up Host localhost:{port}\n    header_up -X-Forwarded-Host\n    header_up X-Forwarded-Proto http\n  }}\n}}\n\n",
        domain = p.domain,
        port = p.port
      ));
        }
    }

    out.push_str(GENERATED_END);
    out.push('\n');
    out
}

fn upsert_managed_block(existing: &str, managed: &str) -> String {
    if let (Some(begin), Some(end)) = (existing.find(GENERATED_BEGIN), existing.find(GENERATED_END))
    {
        let end_inclusive = end + GENERATED_END.len();
        let mut next = String::new();
        next.push_str(&existing[..begin]);
        if !next.ends_with('\n') && !next.is_empty() {
            next.push('\n');
        }
        next.push_str(managed);
        if end_inclusive < existing.len() {
            if !managed.ends_with('\n') {
                next.push('\n');
            }
            next.push_str(&existing[end_inclusive..]);
        }
        return next;
    }

    let mut next = existing.trim_end().to_string();
    if !next.is_empty() {
        next.push_str("\n\n");
    }
    next.push_str(managed);
    next
}
