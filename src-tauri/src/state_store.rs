use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
};

use fs2::FileExt;

use crate::{
    app_state::AppStateFile,
    error::{AppError, AppResult},
    paths,
};

#[derive(Clone)]
pub struct StateStore {
    path: PathBuf,
}

impl StateStore {
    pub fn new() -> AppResult<Self> {
        Ok(Self {
            path: paths::app_state_path()?,
        })
    }

    pub fn read(&self) -> AppResult<AppStateFile> {
        let parent = self.path.parent().ok_or_else(|| AppError::StoreCorrupted {
            path: self.path.clone(),
        })?;
        fs::create_dir_all(parent)?;

        if !self.path.exists() {
            return Ok(AppStateFile::default());
        }

        let mut file = OpenOptions::new().read(true).open(&self.path)?;
        file.lock_shared()?;

        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        file.unlock()?;

        if buf.trim().is_empty() {
            return Ok(AppStateFile::default());
        }

        let state: AppStateFile = serde_json::from_str(&buf)?;
        Ok(state)
    }

    pub fn write(&self, state: &AppStateFile) -> AppResult<()> {
        let parent = self.path.parent().ok_or_else(|| AppError::StoreCorrupted {
            path: self.path.clone(),
        })?;
        fs::create_dir_all(parent)?;

        let lock_path = self.path.clone();
        let lock_file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(&lock_path)?;
        lock_file.lock_exclusive()?;

        let tmp_path = self.path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(state)?;

        {
            let mut tmp = File::create(&tmp_path)?;
            tmp.write_all(json.as_bytes())?;
            tmp.write_all(b"\n")?;
            tmp.sync_all()?;
        }

        fs::rename(&tmp_path, &self.path)?;
        lock_file.unlock()?;
        Ok(())
    }
}
