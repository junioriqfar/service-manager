use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use sm_config::ConfigPaths;
use sm_core::ServiceConfig;
use sm_win::{CommandRunner, ScmError, ServiceState, SystemRunner};

use crate::util;

pub struct AppState {
    pub paths: ConfigPaths,
    pub nssm_path: PathBuf,
    pub runner: Arc<dyn CommandRunner>,
    services: Mutex<Vec<ServiceConfig>>,
    warning: Mutex<Option<String>>,
    batch_running: AtomicBool,
    cancel: Mutex<Option<Arc<AtomicBool>>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceRow {
    #[serde(flatten)]
    pub config: ServiceConfig,
    pub installed: bool,
    pub state: String,
    pub state_label: String,
    pub pid: u32,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathsView {
    pub home: String,
    pub services_path: String,
    pub projects_path: String,
    pub nssm_path: String,
    pub app_log: String,
}

#[derive(Debug, Clone)]
pub struct StatusSnapshot {
    pub installed: bool,
    pub state: String,
    pub label: String,
    pub pid: u32,
    pub error: Option<String>,
}

impl AppState {
    pub fn new() -> Self {
        Self::with_paths(ConfigPaths::detect())
    }

    pub fn with_paths(paths: ConfigPaths) -> Self {
        let loaded = sm_config::load_services_lenient(&paths.services_path(), &paths.backup_dir());
        let _ = std::fs::create_dir_all(paths.logs_dir());
        Self {
            nssm_path: paths.nssm_path(),
            paths,
            runner: Arc::new(SystemRunner),
            services: Mutex::new(loaded.services),
            warning: Mutex::new(loaded.warning),
            batch_running: AtomicBool::new(false),
            cancel: Mutex::new(None),
        }
    }

    pub fn paths_view(&self) -> PathsView {
        PathsView {
            home: self.paths.home().to_string_lossy().to_string(),
            services_path: self.paths.services_path().to_string_lossy().to_string(),
            projects_path: self.paths.projects_path().to_string_lossy().to_string(),
            nssm_path: self.nssm_path.to_string_lossy().to_string(),
            app_log: self.paths.app_log_path().to_string_lossy().to_string(),
        }
    }

    pub fn services(&self) -> Vec<ServiceConfig> {
        self.services.lock().unwrap().clone()
    }

    pub fn find(&self, name: &str) -> Option<ServiceConfig> {
        self.services
            .lock()
            .unwrap()
            .iter()
            .find(|service| service.name.eq_ignore_ascii_case(name))
            .cloned()
    }

    pub fn replace_services(&self, services: Vec<ServiceConfig>) {
        *self.services.lock().unwrap() = services;
    }

    pub fn save(&self) -> Result<(), String> {
        sm_config::save_services(
            &self.paths.services_path(),
            &self.paths.backup_dir(),
            &self.services(),
        )
        .map_err(|error| error.to_string())
    }

    pub fn upsert(&self, service: ServiceConfig, edit: bool) -> Result<(), String> {
        {
            let mut services = self.services.lock().unwrap();
            if edit {
                let index = services
                    .iter()
                    .position(|existing| existing.name.eq_ignore_ascii_case(&service.name))
                    .ok_or_else(|| format!("Service '{}' tidak ada di config.", service.name))?;
                services[index] = service;
            } else {
                if services
                    .iter()
                    .any(|existing| existing.name.eq_ignore_ascii_case(&service.name))
                {
                    return Err(format!("Service '{}' sudah ada di config.", service.name));
                }
                services.push(service);
            }
        }
        self.save()
    }

    pub fn remove_config(&self, name: &str) -> Result<(), String> {
        {
            let mut services = self.services.lock().unwrap();
            services.retain(|service| !service.name.eq_ignore_ascii_case(name));
        }
        self.save()
    }

    pub fn take_warning(&self) -> Option<String> {
        self.warning.lock().unwrap().take()
    }

    #[allow(dead_code)]
    pub fn set_warning(&self, warning: Option<String>) {
        *self.warning.lock().unwrap() = warning;
    }

    pub fn status(&self, name: &str) -> StatusSnapshot {
        match sm_win::query_status(name) {
            Ok(info) => StatusSnapshot {
                installed: true,
                state: state_key(info.state).to_string(),
                label: info.state.to_string(),
                pid: info.process_id,
                error: None,
            },
            Err(ScmError::NotFound(_)) => StatusSnapshot {
                installed: false,
                state: "notInstalled".to_string(),
                label: "Not Installed".to_string(),
                pid: 0,
                error: None,
            },
            Err(error) => StatusSnapshot {
                installed: false,
                state: "unknown".to_string(),
                label: "Unknown".to_string(),
                pid: 0,
                error: Some(error.to_string()),
            },
        }
    }

    pub fn rows(&self) -> Vec<ServiceRow> {
        self.services()
            .into_iter()
            .map(|config| {
                let status = self.status(&config.name);
                ServiceRow {
                    config,
                    installed: status.installed,
                    state: status.state,
                    state_label: status.label,
                    pid: status.pid,
                    error: status.error,
                }
            })
            .collect()
    }

    pub fn installed_services(&self) -> Vec<ServiceConfig> {
        self.services()
            .into_iter()
            .filter(|service| sm_win::service_exists(&service.name).unwrap_or(false))
            .collect()
    }

    pub fn begin_batch(&self) -> Option<Arc<AtomicBool>> {
        if self
            .batch_running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return None;
        }
        let flag = Arc::new(AtomicBool::new(false));
        *self.cancel.lock().unwrap() = Some(flag.clone());
        Some(flag)
    }

    pub fn finish_batch(&self) {
        self.batch_running.store(false, Ordering::SeqCst);
        *self.cancel.lock().unwrap() = None;
    }

    pub fn request_cancel(&self) {
        if let Some(flag) = self.cancel.lock().unwrap().as_ref() {
            flag.store(true, Ordering::SeqCst);
        }
    }

    #[allow(dead_code)]
    pub fn is_batch_running(&self) -> bool {
        self.batch_running.load(Ordering::SeqCst)
    }

    pub fn log(&self, message: &str) {
        util::append_app_log(&self.paths, message);
    }
}

pub fn state_key(state: ServiceState) -> &'static str {
    match state {
        ServiceState::Running => "running",
        ServiceState::Stopped => "stopped",
        ServiceState::Paused => "paused",
        ServiceState::StartPending => "startPending",
        ServiceState::StopPending => "stopPending",
        ServiceState::ContinuePending => "continuePending",
        ServiceState::PausePending => "pausePending",
        ServiceState::Unknown(_) => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_temp() -> (tempfile::TempDir, AppState) {
        let temp = tempfile::tempdir().unwrap();
        let state = AppState::with_paths(ConfigPaths::new(temp.path()));
        (temp, state)
    }

    #[test]
    fn starts_empty_when_config_missing() {
        let (_temp, state) = state_with_temp();
        assert!(state.services().is_empty());
        assert_eq!(state.status("does-not-exist").state, "notInstalled");
    }

    #[test]
    fn saves_and_finds_services() {
        let (_temp, state) = state_with_temp();
        state.replace_services(vec![ServiceConfig {
            name: "demo".into(),
            executable: r"C:\demo.exe".into(),
            ..ServiceConfig::default()
        }]);
        state.save().unwrap();

        let reloaded = AppState::with_paths(state.paths.clone());
        assert_eq!(reloaded.services().len(), 1);
        assert_eq!(reloaded.find("DEMO").unwrap().name, "demo");
    }

    #[test]
    fn upsert_adds_and_updates_services() {
        let (_temp, state) = state_with_temp();
        let mut service = ServiceConfig {
            name: "demo".into(),
            executable: r"C:\demo.exe".into(),
            ..ServiceConfig::default()
        };
        state.upsert(service.clone(), false).unwrap();
        service.display_name = "Demo Updated".into();
        state.upsert(service, true).unwrap();
        assert_eq!(state.services().len(), 1);
        assert_eq!(state.services()[0].display_name, "Demo Updated");
    }

    #[test]
    fn upsert_rejects_duplicate_names() {
        let (_temp, state) = state_with_temp();
        let service = ServiceConfig {
            name: "demo".into(),
            executable: r"C:\demo.exe".into(),
            ..ServiceConfig::default()
        };
        state.upsert(service.clone(), false).unwrap();
        assert!(state.upsert(service, false).is_err());
    }

    #[test]
    fn remove_config_deletes_service() {
        let (_temp, state) = state_with_temp();
        state
            .upsert(
                ServiceConfig {
                    name: "demo".into(),
                    executable: r"C:\demo.exe".into(),
                    ..ServiceConfig::default()
                },
                false,
            )
            .unwrap();
        state.remove_config("demo").unwrap();
        assert!(state.services().is_empty());
    }

    #[test]
    fn warning_can_be_taken_once() {
        let (_temp, state) = state_with_temp();
        state.set_warning(Some("broken".into()));
        assert_eq!(state.take_warning(), Some("broken".to_string()));
        assert_eq!(state.take_warning(), None);
    }

    #[test]
    fn batch_lock_prevents_overlap() {
        let (_temp, state) = state_with_temp();
        let flag = state.begin_batch().unwrap();
        assert!(state.begin_batch().is_none());
        assert!(state.is_batch_running());
        state.request_cancel();
        assert!(flag.load(Ordering::SeqCst));
        state.finish_batch();
        assert!(!state.is_batch_running());
    }

    #[test]
    fn state_keys_are_stable_for_ui() {
        assert_eq!(state_key(ServiceState::Running), "running");
        assert_eq!(state_key(ServiceState::Stopped), "stopped");
        assert_eq!(state_key(ServiceState::StartPending), "startPending");
        assert_eq!(state_key(ServiceState::Unknown(42)), "unknown");
    }

    #[test]
    fn paths_view_contains_expected_files() {
        let (_temp, state) = state_with_temp();
        let view = state.paths_view();
        assert!(view.services_path.ends_with("services.json"));
        assert!(view.nssm_path.ends_with("nssm.exe"));
    }
}
