use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
  #[error("failed to determine home directory")]
  HomeDirUnavailable,

  #[error("io error: {0}")]
  Io(#[from] std::io::Error),

  #[error("json error: {0}")]
  Json(#[from] serde_json::Error),

  #[error("store is corrupted: {path}")]
  StoreCorrupted { path: PathBuf },

  #[error("{tool} is not installed. {help}")]
  ToolMissing { tool: String, help: String },

  #[error("{tool} failed: {message}")]
  ToolFailed { tool: String, message: String },
}

pub type AppResult<T> = Result<T, AppError>;

