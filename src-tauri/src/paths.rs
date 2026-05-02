use std::path::PathBuf;

use crate::error::{AppError, AppResult};

pub fn domainest_dir() -> AppResult<PathBuf> {
  let home = dirs::home_dir().ok_or(AppError::HomeDirUnavailable)?;
  Ok(home.join(".domainest"))
}

pub fn projects_json_path() -> AppResult<PathBuf> {
  Ok(domainest_dir()?.join("projects.json"))
}

pub fn certs_dir() -> AppResult<PathBuf> {
  Ok(domainest_dir()?.join("certs"))
}

pub fn logs_dir() -> AppResult<PathBuf> {
  Ok(domainest_dir()?.join("logs"))
}

pub fn caddy_dir() -> AppResult<PathBuf> {
  Ok(domainest_dir()?.join("caddy"))
}

pub fn dnsmasq_dir() -> AppResult<PathBuf> {
  Ok(domainest_dir()?.join("dnsmasq"))
}

pub fn app_state_path() -> AppResult<PathBuf> {
  Ok(domainest_dir()?.join("state.json"))
}
