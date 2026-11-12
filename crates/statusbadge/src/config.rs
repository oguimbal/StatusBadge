use anyhow::{Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use statusbadge_core::Target;
use std::{fs::File, path::PathBuf};

/// Preferences only: tokens live in the Keychain, never here.
#[derive(Default, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub targets: Vec<Target>,
    /// Native AppKit coordinates (bottom-left origin), in logical points.
    pub position: Option<[f64; 2]>,
}
pub fn data_dir() -> PathBuf {
    directories::ProjectDirs::from("fr", "gmbl", "StatusBadge")
        .expect("user data directory")
        .data_dir()
        .to_path_buf()
}
impl Config {
    pub fn load() -> Self {
        let mut config: Self = std::fs::read(data_dir().join("config.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        statusbadge_core::sort_targets(&mut config.targets);
        config
    }
    pub fn save(&self) -> Result<()> {
        let directory = data_dir();
        std::fs::create_dir_all(&directory)?;
        let mut file = tempfile::NamedTempFile::new_in(&directory)?;
        serde_json::to_writer_pretty(&mut file, self)?;
        file.as_file().sync_all()?;
        file.persist(directory.join("config.json"))?;
        Ok(())
    }
}
pub fn single_instance() -> Result<File> {
    std::fs::create_dir_all(data_dir())?;
    let file = File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(data_dir().join("instance.lock"))?;
    file.try_lock_exclusive()
        .context("Status Badge est déjà lancé")?;
    Ok(file)
}
