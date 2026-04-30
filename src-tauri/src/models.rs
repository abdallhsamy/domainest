use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatus {
  Running,
  Stopped,
}

impl Default for ProjectStatus {
  fn default() -> Self {
    Self::Stopped
  }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
  pub id: Uuid,
  pub name: String,
  pub path: String,
  pub domain: String,
  pub port: u16,
  #[serde(default = "default_ssl")]
  pub ssl: bool,
  #[serde(default)]
  pub status: ProjectStatus,

  #[serde(default = "default_command")]
  pub command: String,
  #[serde(default = "default_args")]
  pub args: Vec<String>,

  #[serde(default)]
  pub pid: Option<u32>,
}

fn default_ssl() -> bool {
  true
}

fn default_command() -> String {
  "pnpm".to_string()
}

fn default_args() -> Vec<String> {
  vec!["dev".to_string()]
}

