use std::{fs, path::PathBuf};

use crate::{
    error::AppResult,
    paths,
    sidecar::{mkcert_path, run_mkcert, tool_output_ok},
    state_store::StateStore,
};

pub struct MkcertManager;

impl MkcertManager {
    pub fn ensure_available() -> AppResult<()> {
        let _ = mkcert_path()?;
        Ok(())
    }

    fn install_local_ca_inner() -> AppResult<()> {
        Self::ensure_available()?;
        let out = run_mkcert(&["-install"])?;
        tool_output_ok(&out, "mkcert -install")
    }

    pub fn install_local_ca(state_store: &StateStore) -> AppResult<()> {
        let mut state = state_store.read()?;
        if state.mkcert_installed {
            return Ok(());
        }

        Self::install_local_ca_inner()?;
        state.mkcert_installed = true;
        state_store.write(&state)?;
        Ok(())
    }

    pub fn ensure_cert(domain: &str) -> AppResult<(PathBuf, PathBuf)> {
        Self::ensure_available()?;

        let certs_dir = paths::certs_dir()?;
        fs::create_dir_all(&certs_dir)?;

        let cert_path = certs_dir.join(format!("{domain}.pem"));
        let key_path = certs_dir.join(format!("{domain}-key.pem"));

        if cert_path.exists() && key_path.exists() {
            return Ok((cert_path, key_path));
        }

        let out = run_mkcert(&[
            "-cert-file",
            cert_path.to_string_lossy().as_ref(),
            "-key-file",
            key_path.to_string_lossy().as_ref(),
            domain,
        ])?;
        tool_output_ok(&out, &format!("mkcert {domain}"))?;

        Ok((cert_path, key_path))
    }
}
