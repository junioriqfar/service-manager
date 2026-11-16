use std::env;
use std::path::{Path, PathBuf};

/// Lokasi file runtime aplikasi.
///
/// Urutan pencarian:
/// 1. `SERVICE_MANAGER_HOME`
/// 2. ancestor executable yang memiliki `Cargo.toml` + `data/` (mode development)
/// 3. folder executable (mode portable)
/// 4. current working directory
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
                for ancestor in parent.ancestors() {
                    if ancestor.join("Cargo.toml").is_file() && ancestor.join("data").is_dir() {
                        return Self::new(ancestor);
                    }
                }
                return Self::new(parent);
            }
        }

        Self::new(env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    pub fn data_dir(&self) -> PathBuf {
        self.home.join("data")
    }

    pub fn services_path(&self) -> PathBuf {
        self.data_dir().join("services.json")
    }

    pub fn projects_path(&self) -> PathBuf {
        self.data_dir().join("projects.json")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_config_lives_in_data_folder() {
        let paths = ConfigPaths::new(r"C:\apps\ServiceManager");
        assert_eq!(
            paths.services_path(),
            PathBuf::from(r"C:\apps\ServiceManager\data\services.json")
        );
        assert_eq!(
            paths.projects_path(),
            PathBuf::from(r"C:\apps\ServiceManager\data\projects.json")
        );
    }

    #[test]
    fn backup_and_logs_live_at_app_root() {
        let paths = ConfigPaths::new(r"C:\apps\ServiceManager");
        assert_eq!(
            paths.backup_dir(),
            PathBuf::from(r"C:\apps\ServiceManager\backup")
        );
        assert_eq!(
            paths.logs_dir(),
            PathBuf::from(r"C:\apps\ServiceManager\logs")
        );
    }
}
