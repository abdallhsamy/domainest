use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
};

use crate::error::{AppError, AppResult};

const SETUP_HELP: &str = "Run `pnpm setup:deps` from the Domainest repo.";

/// Host triple for bundled sidecar filenames (`caddy-aarch64-apple-darwin`, etc.).
fn host_target_triple() -> String {
    match (std::env::consts::ARCH, std::env::consts::OS) {
        ("aarch64", "macos") => "aarch64-apple-darwin".to_string(),
        ("x86_64", "macos") => "x86_64-apple-darwin".to_string(),
        ("aarch64", "linux") => "aarch64-unknown-linux-gnu".to_string(),
        ("x86_64", "linux") => "x86_64-unknown-linux-gnu".to_string(),
        (arch, os) => format!("{arch}-unknown-{os}"),
    }
}

fn tool_filename(tool: &str) -> String {
    format!("{tool}-{}", host_target_triple())
}

fn push_bin_candidates(paths: &mut Vec<PathBuf>, dir: &Path, tool: &str) {
    let name = tool_filename(tool);
    paths.push(dir.join(&name));
    paths.push(dir.join("bin").join(&name));
    paths.push(dir.join("src-tauri/bin").join(&name));
}

fn search_paths(tool: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Ok(dir) = std::env::var("DOMAINEST_BIN_DIR") {
        push_bin_candidates(&mut paths, Path::new(&dir), tool);
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(mut dir) = exe.parent().map(Path::to_path_buf) {
            loop {
                push_bin_candidates(&mut paths, &dir, tool);
                if !dir.pop() {
                    break;
                }
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors() {
            push_bin_candidates(&mut paths, ancestor, tool);
        }
    }

    paths
}

/// If the exact triple-named binary is missing, pick `tool-*` in the same directory.
fn find_by_prefix_in_dir(dir: &Path, tool: &str) -> Option<PathBuf> {
    let prefix = format!("{tool}-");
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(&prefix) && path.is_file() {
            return Some(path);
        }
    }
    None
}

fn scan_bin_dirs(tool: &str) -> Option<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(dir) = std::env::var("DOMAINEST_BIN_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(mut dir) = exe.parent().map(Path::to_path_buf) {
            loop {
                dirs.push(dir.join("bin"));
                dirs.push(dir.join("src-tauri/bin"));
                if !dir.pop() {
                    break;
                }
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        for ancestor in cwd.ancestors() {
            dirs.push(ancestor.join("src-tauri/bin"));
            dirs.push(ancestor.join("bin"));
        }
    }
    for dir in dirs {
        if let Some(p) = find_by_prefix_in_dir(&dir, tool) {
            return Some(p);
        }
    }
    None
}

pub fn resolve_tool(tool: &str) -> AppResult<PathBuf> {
    for path in search_paths(tool) {
        if path.is_file() {
            return Ok(path);
        }
    }
    if let Some(path) = scan_bin_dirs(tool) {
        return Ok(path);
    }
    Err(AppError::ToolMissing {
        tool: tool.to_string(),
        help: SETUP_HELP.to_string(),
    })
}

pub fn caddy_path() -> AppResult<PathBuf> {
    resolve_tool("caddy")
}

pub fn mkcert_path() -> AppResult<PathBuf> {
    resolve_tool("mkcert")
}

fn run_tool(tool: &str, args: &[&str]) -> AppResult<Output> {
    let bin = resolve_tool(tool)?;
    let out = Command::new(&bin)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| AppError::ToolFailed {
            tool: tool.to_string(),
            message: e.to_string(),
        })?;
    Ok(out)
}

pub fn run_caddy(args: &[&str]) -> AppResult<Output> {
    run_tool("caddy", args)
}

pub fn run_mkcert(args: &[&str]) -> AppResult<Output> {
    run_tool("mkcert", args)
}

pub fn spawn_caddy(args: &[&str], env: &[(&str, &str)]) -> AppResult<Child> {
    let bin = caddy_path()?;
    let mut cmd = Command::new(&bin);
    cmd.args(args);
    for (k, v) in env {
        cmd.env(k, v);
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    cmd.spawn().map_err(|e| AppError::ToolFailed {
        tool: "caddy".to_string(),
        message: e.to_string(),
    })
}

pub fn tool_output_ok(out: &Output, tool: &str) -> AppResult<()> {
    if out.status.success() {
        return Ok(());
    }
    Err(AppError::ToolFailed {
        tool: tool.to_string(),
        message: format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
        .trim()
        .to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_filename_uses_host_triple() {
        let name = tool_filename("caddy");
        assert!(name.starts_with("caddy-"));
        assert!(!name.ends_with("-unknown"));
    }

    #[test]
    fn resolve_from_domainest_bin_dir() {
        let dir = std::env::temp_dir().join(format!("domainest-bin-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let fake = dir.join(tool_filename("caddy"));
        let _ = std::fs::write(&fake, b"");
        std::env::set_var("DOMAINEST_BIN_DIR", &dir);
        let path = resolve_tool("caddy").expect("should resolve");
        assert_eq!(path, fake);
        std::env::remove_var("DOMAINEST_BIN_DIR");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
