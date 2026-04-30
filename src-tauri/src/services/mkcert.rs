use std::{
  fs,
  path::{PathBuf},
};

use tauri_plugin_shell::ShellExt;

use crate::{
  error::{AppError, AppResult},
  paths,
  state_store::StateStore,
};

pub struct MkcertManager;

impl MkcertManager {
  pub fn ensure_available(app: &tauri::AppHandle) -> AppResult<()> {
    // If the sidecar isn't present, this will error with a helpful message.
    let _ = app
      .shell()
      .sidecar("mkcert")
      .map_err(|_e| AppError::ToolMissing {
        tool: "mkcert".to_string(),
        help: "Bundled mkcert missing. Run `pnpm setup:mkcert`.".to_string(),
      })?;
    Ok(())
  }

  fn install_local_ca_inner(app: &tauri::AppHandle) -> AppResult<()> {
    Self::ensure_available(app)?;

    let out = tauri::async_runtime::block_on(async move {
      app.shell()
        .sidecar("mkcert")
        .map_err(|e| AppError::ToolFailed {
          tool: "mkcert".to_string(),
          message: e.to_string(),
        })?
        .args(["-install"])
        .output()
        .await
        .map_err(|e| AppError::ToolFailed {
          tool: "mkcert -install".to_string(),
          message: e.to_string(),
        })
    })?;

    if !out.status.success() {
      return Err(AppError::ToolFailed {
        tool: "mkcert -install".to_string(),
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

  pub fn install_local_ca(app: &tauri::AppHandle, state_store: &StateStore) -> AppResult<()> {
    let mut state = state_store.read()?;
    if state.mkcert_installed {
      return Ok(());
    }

    Self::install_local_ca_inner(app)?;
    state.mkcert_installed = true;
    state_store.write(&state)?;
    Ok(())
  }

  pub fn ensure_cert(app: &tauri::AppHandle, domain: &str) -> AppResult<(PathBuf, PathBuf)> {
    Self::ensure_available(app)?;

    let certs_dir = paths::certs_dir()?;
    fs::create_dir_all(&certs_dir)?;

    let cert_path = certs_dir.join(format!("{domain}.pem"));
    let key_path = certs_dir.join(format!("{domain}-key.pem"));

    if cert_path.exists() && key_path.exists() {
      return Ok((cert_path, key_path));
    }

    let cert_path2 = cert_path.clone();
    let key_path2 = key_path.clone();
    let domain2 = domain.to_string();
    let out = tauri::async_runtime::block_on(async move {
      app.shell()
        .sidecar("mkcert")
        .map_err(|e| AppError::ToolFailed {
          tool: "mkcert".to_string(),
          message: e.to_string(),
        })?
        .args([
          "-cert-file",
          cert_path2.to_string_lossy().as_ref(),
          "-key-file",
          key_path2.to_string_lossy().as_ref(),
          domain2.as_str(),
        ])
        .output()
        .await
        .map_err(|e| AppError::ToolFailed {
          tool: format!("mkcert {domain}"),
          message: e.to_string(),
        })
    })?;

    if !out.status.success() {
      return Err(AppError::ToolFailed {
        tool: format!("mkcert {domain}"),
        message: format!(
          "{}{}",
          String::from_utf8_lossy(&out.stdout),
          String::from_utf8_lossy(&out.stderr)
        )
        .trim()
        .to_string(),
      });
    }

    Ok((cert_path, key_path))
  }
}

