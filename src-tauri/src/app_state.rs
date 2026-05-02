use serde::{Deserialize, Serialize};

fn default_domain_suffix() -> String {
    "test".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppStateFile {
    #[serde(default)]
    pub mkcert_installed: bool,
    #[serde(default)]
    pub resolver_installed_macos: bool,
    #[serde(default = "default_domain_suffix")]
    pub domain_suffix: String,
}
