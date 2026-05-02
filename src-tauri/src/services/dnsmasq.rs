use std::{fs, path::PathBuf, process::Command};

use crate::{
    error::{AppError, AppResult},
    paths,
};

pub struct DnsmasqManager;

impl DnsmasqManager {
    pub fn write_managed_config(domain_suffix: &str) -> AppResult<PathBuf> {
        let dir = paths::dnsmasq_dir()?;
        fs::create_dir_all(&dir)?;

        let path = dir.join("domainest.conf");
        let contents = [
            &format!(
                "address=/.{}/127.0.0.1",
                domain_suffix.trim_start_matches('.')
            ),
            "listen-address=127.0.0.1",
            "bind-interfaces",
            "",
        ]
        .join("\n");
        fs::write(&path, contents)?;
        Ok(path)
    }

    pub fn setup_system(domain_suffix: &str) -> AppResult<()> {
        let _managed = Self::write_managed_config(domain_suffix)?;
        crate::services::dns_server::ensure_running(domain_suffix)?;

        if cfg!(target_os = "macos") {
            Self::setup_macos(domain_suffix)
        } else if cfg!(target_os = "linux") {
            Self::setup_linux()
        } else {
            Err(AppError::ToolFailed {
                tool: "dnsmasq".to_string(),
                message: "unsupported OS for dnsmasq setup".to_string(),
            })
        }
    }

    fn setup_macos(domain_suffix: &str) -> AppResult<()> {
        let suffix = domain_suffix.trim_start_matches('.').to_lowercase();
        let resolver_path = format!("/etc/resolver/{suffix}");

        // Point `.<suffix>` to our embedded DNS server (unprivileged port).
        // macOS supports `port` in /etc/resolver/<domain>.
        if !macos_resolver_ok(&resolver_path)? {
            run_macos_admin_script(&format!(
                "mkdir -p /etc/resolver && printf '%s\\n' 'nameserver 127.0.0.1' 'port 53535' > {}",
                resolver_path
            ))?;
        }
        Ok(())
    }

    fn setup_linux() -> AppResult<()> {
        // Linux wildcard routing is distro-specific. For now, keep an actionable error instead
        // of silently misconfiguring resolution.
        Err(AppError::ToolFailed {
      tool: "dns".to_string(),
      message: "Linux DNS routing for `.test` is not yet implemented without system dnsmasq. Use macOS for now or install dnsmasq and configure systemd-resolved to query 127.0.0.1:53.".to_string(),
    })
    }
}

fn macos_resolver_ok(resolver_path: &str) -> AppResult<bool> {
    let contents = match fs::read_to_string(resolver_path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(AppError::Io(e)),
    };

    let normalized = contents
        .lines()
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect::<Vec<_>>();

    Ok(normalized.iter().any(|l| l == "nameserver 127.0.0.1")
        && normalized.iter().any(|l| l == "port 53535"))
}

fn run_macos_admin_script(shell_script: &str) -> AppResult<()> {
    let escaped = shell_script.replace('\\', "\\\\").replace('"', "\\\"");
    let osa = format!(
        "do shell script \"{}\" with administrator privileges",
        escaped
    );

    let out = Command::new("osascript").args(["-e", &osa]).output()?;
    if !out.status.success() {
        return Err(AppError::ToolFailed {
            tool: "osascript".to_string(),
            message: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        });
    }
    Ok(())
}
