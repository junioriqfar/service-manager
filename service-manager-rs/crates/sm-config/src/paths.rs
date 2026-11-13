use std::env;
use std::path::{Path, PathBuf};

/// Lokasi file runtime aplikasi. Default mengikuti folder executable, sama
/// seperti `$PSScriptRoot` pada versi PowerShell.
#[derive(Debug, Clone)]
pub struct ConfigPaths {
    home: PathBuf,
}

impl ConfigPaths {
    pub fn new(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }

    pub fn detect() -> Self {
        if let Ok(home) = env::var("SERVICE_MANAGER_HOME") {
            if !home.trim().is_empty() {
                return Self::new(home);
            }
        }

        if let Ok(executable) = env::current_exe() {
            if let Some(parent) = executable.parent() {
                return Self::new(parent);
            }
        }

        Self::new(env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn services_path(&self) -> PathBuf {
        self.home.join("services.json")
    }

    pub fn projects_path(&self) -> PathBuf {
        self.home.join("projects.json")
    }

    pub fn backup_dir(&self) -> PathBuf {
        self.home.join("backup")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.home.join("logs")
    }

    pub fn app_log_path(&self) -> PathBuf {
        self.logs_dir().join("app.log")
    }

    /// NSSM dicari satu level di atas folder aplikasi, lalu fallback global.
    pub fn nssm_path(&self) -> PathBuf {
        let sibling = self
            .home
            .parent()
            .map(|parent| parent.join("nssm").join("nssm.exe"))
            .unwrap_or_else(|| PathBuf::from(r"..\nssm\nssm.exe"));
        if sibling.is_file() {
            return sibling;
        }
        PathBuf::from(r"C:\tools\nssm\nssm.exe")
    }
}
