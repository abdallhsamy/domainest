use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use fs2::FileExt;

use crate::{
    error::{AppError, AppResult},
    models::Project,
    paths,
};

#[derive(Clone)]
pub struct Store {
    path: PathBuf,
}

impl Store {
    pub fn new() -> AppResult<Self> {
        Ok(Self {
            path: paths::projects_json_path()?,
        })
    }

    pub fn list_projects(&self) -> AppResult<Vec<Project>> {
        let parent = self.path.parent().ok_or_else(|| AppError::StoreCorrupted {
            path: self.path.clone(),
        })?;
        fs::create_dir_all(parent)?;

        if !self.path.exists() {
            return Ok(vec![]);
        }

        let mut file = OpenOptions::new().read(true).open(&self.path)?;
        file.lock_shared()?;

        let mut buf = String::new();
        file.read_to_string(&mut buf)?;

        file.unlock()?;

        if buf.trim().is_empty() {
            return Ok(vec![]);
        }

        let projects: Vec<Project> = serde_json::from_str(&buf)?;
        Ok(projects)
    }

    pub fn save_projects(&self, projects: &[Project]) -> AppResult<()> {
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

        let tmp_path = tmp_path_for(&self.path);
        let json = serde_json::to_string_pretty(projects)?;

        {
            let mut tmp = File::create(&tmp_path)?;
            tmp.write_all(json.as_bytes())?;
            tmp.write_all(b"\n")?;
            tmp.sync_all()?;
        }

        rename_atomic(&tmp_path, &self.path)?;

        lock_file.unlock()?;
        Ok(())
    }
}

fn tmp_path_for(path: &Path) -> PathBuf {
    let mut tmp = path.to_path_buf();
    let ext = match path.extension().and_then(|e| e.to_str()) {
        Some(e) => format!("{e}.tmp"),
        None => "tmp".to_string(),
    };
    tmp.set_extension(ext);
    tmp
}

fn rename_atomic(from: &Path, to: &Path) -> std::io::Result<()> {
    match fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(e) => {
            if cfg!(windows) {
                let _ = fs::remove_file(to);
                fs::rename(from, to)
            } else {
                Err(e)
            }
        }
    }
}
