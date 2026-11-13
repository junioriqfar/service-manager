use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sm_core::ProjectOverride;

use crate::ConfigError;

fn default_version() -> u32 {
    1
}

fn default_scan_roots() -> Vec<String> {
    vec![r"D:\Projects\PHPProjects".to_string()]
}

fn default_allowed_ips() -> Vec<String> {
    vec!["127.0.0.1".to_string(), "::1".to_string()]
}

fn default_git_path() -> String {
    r"C:\laragon\bin\git\cmd\git.exe".to_string()
}

fn default_php_path() -> String {
    r"C:\laragon\bin\php\php.exe".to_string()
}

fn default_composer_path() -> String {
    r"C:\laragon\bin\composer\composer.bat".to_string()
}

fn default_npm_path() -> String {
    r"C:\Program Files\nodejs\npm.cmd".to_string()
}

/// Isi `projects.json` versi 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectsFile {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default = "default_scan_roots")]
    pub scan_roots: Vec<String>,
    #[serde(default = "default_allowed_ips")]
    pub allowed_client_ips: Vec<String>,
    #[serde(default = "default_git_path")]
    pub git_path: String,
    #[serde(default = "default_php_path")]
    pub php_path: String,
    #[serde(default = "default_composer_path")]
    pub composer_path: String,
    #[serde(default = "default_npm_path")]
    pub npm_path: String,
    #[serde(default)]
    pub log_dir: String,
    #[serde(default)]
    pub projects: BTreeMap<String, ProjectOverride>,
}

impl Default for ProjectsFile {
    fn default() -> Self {
        Self {
            version: 1,
            scan_roots: default_scan_roots(),
            allowed_client_ips: default_allowed_ips(),
            git_path: default_git_path(),
            php_path: default_php_path(),
            composer_path: default_composer_path(),
            npm_path: default_npm_path(),
            log_dir: String::new(),
            projects: BTreeMap::new(),
        }
    }
}

impl ProjectsFile {
    pub fn normalize(&mut self) {
        self.scan_roots = self
            .scan_roots
            .iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        self.allowed_client_ips = self
            .allowed_client_ips
            .iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();
        self.git_path = self.git_path.trim().to_string();
        self.php_path = self.php_path.trim().to_string();
        self.composer_path = self.composer_path.trim().to_string();
        self.npm_path = self.npm_path.trim().to_string();
        self.log_dir = self.log_dir.trim().to_string();

        for project in self.projects.values_mut() {
            project.normalize();
        }
    }
}

pub fn load_projects(path: &Path) -> Result<ProjectsFile, ConfigError> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(ProjectsFile::default()),
        Err(error) => return Err(error.into()),
    };

    if raw.trim().is_empty() {
        return Ok(ProjectsFile::default());
    }

    let mut projects: ProjectsFile = serde_json::from_str(&raw)?;
    projects.normalize();
    Ok(projects)
}

pub fn save_projects(path: &Path, projects: &ProjectsFile) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(projects)?;
    let temp_path = path.with_extension(format!("json.tmp-{}", std::process::id()));
    fs::write(&temp_path, json)?;
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sm_core::ToggleMode;

    #[test]
    fn missing_projects_file_uses_defaults() {
        let temp = tempfile::tempdir().unwrap();
        let projects = load_projects(&temp.path().join("projects.json")).unwrap();
        assert_eq!(projects.version, 1);
        assert_eq!(projects.allowed_client_ips, ["127.0.0.1", "::1"]);
        assert!(projects.projects.is_empty());
    }

    #[test]
    fn loads_projects_and_normalizes_values() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("projects.json");
        fs::write(
            &path,
            r#"{
                "version": 1,
                "scanRoots": [" D:\\Projects\\PHPProjects ", ""],
                "allowedClientIPs": [" 127.0.0.1 ", "::1"],
                "gitPath": " C:\\git.exe ",
                "phpPath": "C:\\php.exe",
                "composerPath": "C:\\composer.bat",
                "npmPath": "C:\\npm.cmd",
                "projects": {
                    "kasirin": {
                        "enabled": true,
                        "branch": " main ",
                        "composer": "auto",
                        "build": "never",
                        "queueRestart": false,
                        "postUpdateCommand": " echo ok "
                    }
                }
            }"#,
        )
        .unwrap();

        let projects = load_projects(&path).unwrap();
        assert_eq!(projects.scan_roots, [r"D:\Projects\PHPProjects"]);
        assert_eq!(projects.git_path, r"C:\git.exe");
        let override_config = projects.projects.get("kasirin").unwrap();
        assert_eq!(override_config.branch, "main");
        assert_eq!(override_config.build, ToggleMode::Never);
        assert!(!override_config.queue_restart);
        assert_eq!(override_config.post_update_command, "echo ok");
    }

    #[test]
    fn unknown_project_fields_are_ignored() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("projects.json");
        fs::write(
            &path,
            r#"{"projects":{"app":{"enabled":true,"branch":"develop","future":123}}}"#,
        )
        .unwrap();

        let projects = load_projects(&path).unwrap();
        assert_eq!(projects.projects["app"].branch, "develop");
    }

    #[test]
    fn roundtrip_save_and_load_projects() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("projects.json");
        let mut projects = ProjectsFile {
            log_dir: " C:\\logs ".into(),
            ..ProjectsFile::default()
        };
        let override_config = ProjectOverride {
            branch: "develop".into(),
            ..ProjectOverride::default()
        };
        projects.projects.insert("demo".into(), override_config);

        save_projects(&path, &projects).unwrap();
        let loaded = load_projects(&path).unwrap();
        assert_eq!(loaded.log_dir, "C:\\logs");
        assert_eq!(loaded.projects["demo"].branch, "develop");
    }

    #[test]
    fn broken_projects_file_returns_error() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("projects.json");
        fs::write(&path, "{ broken").unwrap();
        assert!(load_projects(&path).is_err());
    }
}
