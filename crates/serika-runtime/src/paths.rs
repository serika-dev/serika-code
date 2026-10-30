use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// On-disk locations. Credentials are not stored here; they go to the OS credential store.
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub home: PathBuf,
    pub config_file: PathBuf,
    pub data_dir: PathBuf,
    pub database: PathBuf,
    pub logs_dir: PathBuf,
    pub log_file: PathBuf,
    pub cache_dir: PathBuf,
}

impl AppPaths {
    /// Resolve directories. `SERIKA_HOME` overrides the platform default so tests and
    /// portable installs can keep config, sessions, and logs together.
    pub fn resolve() -> Result<Self> {
        if let Some(home) = std::env::var_os("SERIKA_HOME") {
            return Self::under(PathBuf::from(home));
        }
        let config_root = dirs::config_dir().ok_or_else(|| Error::msg("could not resolve the configuration directory"))?;
        let data_root = dirs::data_local_dir().unwrap_or_else(|| config_root.clone());
        let cache_root = dirs::cache_dir().unwrap_or_else(|| data_root.clone());
        let home = config_root.join("serika-code");
        Ok(Self {
            config_file: home.join("config.json"),
            data_dir: data_root.join("serika-code"),
            database: data_root.join("serika-code").join("sessions.db"),
            logs_dir: data_root.join("serika-code").join("logs"),
            log_file: data_root.join("serika-code").join("logs").join("serika-code.log"),
            cache_dir: cache_root.join("serika-code"),
            home,
        })
    }

    pub fn under(home: PathBuf) -> Result<Self> {
        Ok(Self {
            config_file: home.join("config.json"),
            data_dir: home.join("data"),
            database: home.join("data").join("sessions.db"),
            logs_dir: home.join("logs"),
            log_file: home.join("logs").join("serika-code.log"),
            cache_dir: home.join("cache"),
            home,
        })
    }

    pub fn ensure(&self) -> Result<()> {
        std::fs::create_dir_all(&self.home)?;
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(&self.logs_dir)?;
        std::fs::create_dir_all(&self.cache_dir)?;
        std::fs::create_dir_all(self.scratch_dir())?;
        if let Some(parent) = self.config_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    pub fn scratch_dir(&self) -> PathBuf {
        self.data_dir.join("scratch")
    }

    pub fn worktrees_dir(&self) -> PathBuf {
        self.data_dir.join("worktrees")
    }
}

pub fn project_settings_path(project: &Path) -> PathBuf {
    project.join(".serika").join("project.json")
}
