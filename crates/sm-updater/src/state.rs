use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

fn default_version() -> u32 {
    1
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub last_checked_at: String,
    #[serde(default)]
    pub last_updated_at: String,
    #[serde(default)]
    pub last_commit: String,
    #[serde(default)]
    pub last_error: String,
    #[serde(default)]
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterState {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub apps: BTreeMap<String, AppState>,
}

impl Default for UpdaterState {
    fn default() -> Self {
        Self {
            version: 1,
            apps: BTreeMap::new(),
        }
    }
}

pub fn state_path(config_path: &Path) -> std::path::PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("updater-state.json")
}

pub fn load_state(config_path: &Path) -> UpdaterState {
    let path = state_path(config_path);
    let Ok(raw) = fs::read_to_string(path) else {
        return UpdaterState::default();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save_state(config_path: &Path, state: &UpdaterState) -> Result<(), String> {
    let path = state_path(config_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let json = serde_json::to_string_pretty(state).map_err(|error| error.to_string())?;
    let temp = path.with_extension(format!("json.tmp-{}", std::process::id()));
    fs::write(&temp, json).map_err(|error| error.to_string())?;
    if let Err(error) = fs::rename(&temp, &path) {
        let _ = fs::remove_file(&temp);
        return Err(error.to_string());
    }
    Ok(())
}

pub fn mark(
    state: &mut UpdaterState,
    app_id: &str,
    status: &str,
    message: Option<&str>,
    duration_ms: u64,
) {
    let now = chrono::Local::now().to_rfc3339();
    let entry = state.apps.entry(app_id.to_string()).or_default();
    entry.status = status.to_string();
    entry.last_checked_at = now.clone();
    entry.duration_ms = duration_ms;
    if matches!(status, "updated" | "degraded") {
        entry.last_updated_at = now;
    }
    if status == "updated" {
        entry.last_error.clear();
    } else if matches!(
        status,
        "failed" | "degraded" | "blocked-migration" | "forbidden"
    ) {
        entry.last_error = message.unwrap_or_default().to_string();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_state() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("data/projects.json");
        let mut state = UpdaterState::default();
        mark(&mut state, "kasirin", "updated", None, 120);
        save_state(&config, &state).unwrap();
        let loaded = load_state(&config);
        assert_eq!(loaded.apps["kasirin"].status, "updated");
        assert!(loaded.apps["kasirin"].last_updated_at.contains('T'));
    }

    #[test]
    fn missing_state_is_empty() {
        let temp = tempfile::tempdir().unwrap();
        let state = load_state(&temp.path().join("data/projects.json"));
        assert!(state.apps.is_empty());
    }

    #[test]
    fn records_error_for_degraded_and_forbidden() {
        let mut state = UpdaterState::default();
        mark(
            &mut state,
            "api",
            "degraded",
            Some("service start failed"),
            10,
        );
        assert_eq!(state.apps["api"].last_error, "service start failed");
        mark(&mut state, "api", "forbidden", Some("IP denied"), 0);
        assert_eq!(state.apps["api"].last_error, "IP denied");
        mark(&mut state, "api", "updated", None, 20);
        assert!(state.apps["api"].last_error.is_empty());
    }
}
