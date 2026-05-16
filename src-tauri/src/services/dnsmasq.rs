use std::{collections::HashSet, fs, path::PathBuf, process::Command};

use crate::{
    domain_suffix,
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

    pub fn setup_system(global_zone: &str, project_domains: &[String]) -> AppResult<()> {
        let zone = global_zone.trim_start_matches('.').to_lowercase();
        crate::services::dns_server::set_project_domains(project_domains);
        let _managed = Self::write_managed_config(&zone)?;
        crate::services::dns_server::ensure_running(&zone)?;

        if cfg!(target_os = "macos") {
            Self::sync_macos_resolvers(&zone, project_domains)?;
        } else if cfg!(target_os = "linux") {
            Self::setup_linux()?;
        }

        Ok(())
    }

    fn sync_macos_resolvers(global_zone: &str, project_domains: &[String]) -> AppResult<()> {
        let zone = global_zone.trim_start_matches('.').to_lowercase();
        let mut keep = HashSet::new();
        keep.insert(zone.clone());

        ensure_macos_resolver(&zone)?;

        for domain in project_domains {
            let host = domain.trim().trim_end_matches('.').to_lowercase();
            if host.is_empty() {
                continue;
            }
            if domain_suffix::host_covered_by_zone_resolver(&host, &zone) {
                continue;
            }
            ensure_macos_resolver(&host)?;
            keep.insert(host);
        }

        prune_stale_macos_resolvers(&keep)?;
        Ok(())
    }

    fn setup_linux() -> AppResult<()> {
        Err(AppError::ToolFailed {
            tool: "dns".to_string(),
            message: "Linux DNS routing for `.test` is not yet implemented without system dnsmasq. Use macOS for now or install dnsmasq and configure systemd-resolved to query 127.0.0.1:53.".to_string(),
        })
    }
}

fn ensure_macos_resolver(resolver_name: &str) -> AppResult<()> {
    let name = resolver_name.trim_start_matches('.').to_lowercase();
    let resolver_path = format!("/etc/resolver/{name}");

    if macos_resolver_ok(&resolver_path)? {
        return Ok(());
    }

    run_macos_admin_script(&format!(
        "mkdir -p /etc/resolver && printf '%s\\n' 'nameserver 127.0.0.1' 'port 53535' > \"{}\"",
        resolver_path.replace('"', "\\\"")
    ))?;
    Ok(())
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

fn prune_stale_macos_resolvers(keep: &HashSet<String>) -> AppResult<()> {
    let dir = match fs::read_dir("/etc/resolver") {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(AppError::Io(e)),
    };

    let mut remove = Vec::new();
    for entry in dir {
        let entry = entry.map_err(AppError::Io)?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_lowercase();
        if keep.contains(&name) {
            continue;
        }
        let path_str = path.to_string_lossy().into_owned();
        if macos_resolver_ok(&path_str)? {
            remove.push(path_str);
        }
    }

    if remove.is_empty() {
        return Ok(());
    }

    let script = remove
        .iter()
        .map(|p| format!("rm -f \"{}\"", p.replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join(" && ");
    run_macos_admin_script(&script)?;
    Ok(())
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
