use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use std::net::IpAddr;

use crate::app_update::update_app;
use crate::ip::{app_allowed_for_ip, client_ip_from_env};
use crate::lock::UpdateLock;
use crate::registry::UpdaterConfig;
use crate::runner::Runner;
use crate::services::{ServiceControl, WindowsServiceControl};
use crate::settings::{effective_settings, settings_path_for};
use crate::state::{load_state, mark, save_state};
use crate::update::UpdateOptions;

#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub check_only: bool,
    pub app: Option<String>,
    /// Diisi oleh wrapper setelah validasi IP; jika kosong dibaca dari env SSH.
    pub client_ip: Option<IpAddr>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub app_id: String,
    pub status: String,
    pub message: String,
    pub forbidden: bool,
}

pub fn lock_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("updater.lock")
}

pub fn default_log_dir(config_path: &Path) -> PathBuf {
    let parent = config_path.parent().unwrap_or_else(|| Path::new("."));
    if parent
        .file_name()
        .map(|name| name.eq_ignore_ascii_case("data"))
        .unwrap_or(false)
    {
        parent
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("logs")
    } else {
        parent.join("logs")
    }
}

pub fn run_registry(
    config: &UpdaterConfig,
    config_path: &Path,
    runner: &dyn Runner,
    options: RunOptions,
) -> Result<Vec<RunOutcome>, String> {
    run_registry_with(config, config_path, runner, &WindowsServiceControl, options)
}

pub fn run_registry_with(
    config: &UpdaterConfig,
    config_path: &Path,
    runner: &dyn Runner,
    services: &dyn ServiceControl,
    options: RunOptions,
) -> Result<Vec<RunOutcome>, String> {
    let _lock = UpdateLock::acquire(&lock_path(config_path), Duration::from_secs(30 * 60))?;
    let client_ip = options.client_ip.or_else(client_ip_from_env);
    let mut state = load_state(config_path);
    let mut outcomes = Vec::new();

    // settings.json authoritative jika ada; kalau tidak, fallback ke
    // settings/trigger lama di projects.json tanpa menulis file baru.
    let effective = effective_settings(
        &settings_path_for(config_path),
        &config.settings,
        &config.trigger,
    )
    .map_err(|error| error.to_string())?;
    let updater_settings = effective.to_updater_settings();

    let log_dir = if updater_settings.log_dir.trim().is_empty() {
        default_log_dir(config_path)
    } else {
        PathBuf::from(updater_settings.log_dir.trim())
    };

    if let Some(requested) = &options.app {
        if config.find(requested).is_none() {
            return Err(format!("App '{requested}' tidak ditemukan di registry."));
        }
    }

    if !options.check_only
        && selected_apps_need_services(config, &options, client_ip)
        && !services.is_elevated()?
    {
        return Err(
            "Update dengan service lifecycle membutuhkan Administrator/elevated. \
             Jalankan melalui Scheduled Task wrapper atau elevated terminal."
                .to_string(),
        );
    }

    for app in &config.apps {
        if !app.enabled {
            continue;
        }
        if let Some(requested) = &options.app {
            if !app.id.eq_ignore_ascii_case(requested) {
                continue;
            }
        }

        if !app_allowed_for_ip(config, app, client_ip) {
            let message = match client_ip {
                Some(ip) => format!("IP {ip} tidak diizinkan untuk app '{}'.", app.id),
                None => format!("Trigger lokal tidak diizinkan untuk app '{}'.", app.id),
            };
            mark(&mut state, &app.id, "forbidden", Some(&message), 0);
            outcomes.push(RunOutcome {
                app_id: app.id.clone(),
                status: "forbidden".to_string(),
                message,
                forbidden: true,
            });
            continue;
        }

        let started = Instant::now();
        let outcome = update_app(
            app,
            &updater_settings,
            &log_dir,
            runner,
            services,
            UpdateOptions {
                check_only: options.check_only,
            },
        );
        let status = outcome.status.as_str().to_string();
        mark(
            &mut state,
            &app.id,
            &status,
            if matches!(
                outcome.status,
                crate::update::UpdateStatus::Failed
                    | crate::update::UpdateStatus::Degraded
                    | crate::update::UpdateStatus::BlockedMigration
            ) {
                Some(&outcome.message)
            } else {
                None
            },
            started.elapsed().as_millis() as u64,
        );
        outcomes.push(RunOutcome {
            app_id: app.id.clone(),
            status,
            message: outcome.message,
            forbidden: false,
        });

        if let Some(commit) = head_commit(
            &updater_settings.git_path,
            Path::new(&app.local_path),
            runner,
        ) {
            if let Some(entry) = state.apps.get_mut(&app.id) {
                entry.last_commit = commit;
            }
        }
    }

    save_state(config_path, &state)?;

    if outcomes.is_empty() {
        return Err("Tidak ada app yang cocok/diproses.".to_string());
    }
    Ok(outcomes)
}

fn head_commit(git_path: &str, project: &Path, runner: &dyn Runner) -> Option<String> {
    let git_path = Path::new(git_path.trim());
    if !git_path.is_file() {
        return None;
    }
    let args = ["rev-parse".to_string(), "HEAD".to_string()];
    let output = runner
        .run(git_path, &args, project, Duration::from_secs(10))
        .ok()?;
    if !output.success() {
        return None;
    }
    let commit = output.stdout.trim();
    if commit.is_empty() {
        None
    } else {
        Some(commit.to_string())
    }
}

fn selected_apps_need_services(
    config: &UpdaterConfig,
    options: &RunOptions,
    client_ip: Option<IpAddr>,
) -> bool {
    config
        .apps
        .iter()
        .filter(|app| app.enabled)
        .filter(|app| {
            options
                .app
                .as_ref()
                .map(|id| app.id.eq_ignore_ascii_case(id))
                .unwrap_or(true)
        })
        .filter(|app| app_allowed_for_ip(config, app, client_ip))
        .any(|app| {
            !app.services.stop_before_pull.is_empty()
                || !app.services.stop_before_build.is_empty()
                || !app.services.restart_after.is_empty()
                || !app.services.start_after.is_empty()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{save_config, AppConfig, Preset, UpdaterConfig};
    use crate::runner::{Output, RunnerError};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn log_dir_is_sibling_of_data_for_registry_config() {
        assert_eq!(
            default_log_dir(Path::new(r"C:\repo\data\projects.json")),
            PathBuf::from(r"C:\repo\logs")
        );
    }

    #[test]
    fn log_dir_is_sibling_for_custom_config() {
        assert_eq!(
            default_log_dir(Path::new(r"C:\config\projects.json")),
            PathBuf::from(r"C:\config\logs")
        );
    }

    struct FakeRunner {
        head_reads: AtomicUsize,
        programs: std::sync::Mutex<Vec<String>>,
    }

    impl FakeRunner {
        fn new() -> Self {
            Self {
                head_reads: AtomicUsize::new(0),
                programs: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn programs(&self) -> Vec<String> {
            self.programs.lock().unwrap().clone()
        }
    }

    impl Runner for FakeRunner {
        fn run(
            &self,
            program: &Path,
            args: &[String],
            _cwd: &Path,
            _timeout: Duration,
        ) -> Result<Output, RunnerError> {
            self.programs
                .lock()
                .unwrap()
                .push(program.to_string_lossy().to_string());
            let first = args.first().map(String::as_str).unwrap_or("");
            let stdout = match first {
                "status" => String::new(),
                "rev-parse" if args.get(1).map(String::as_str) == Some("--abbrev-ref") => {
                    "main\n".to_string()
                }
                "rev-parse" if args.get(1).map(String::as_str) == Some("HEAD") => {
                    let count = self.head_reads.fetch_add(1, Ordering::SeqCst);
                    if count == 0 {
                        "aaaa1111\n".to_string()
                    } else {
                        "bbbb2222\n".to_string()
                    }
                }
                "rev-parse" => "ok\n".to_string(),
                "ls-remote" => "refs/heads/main\n".to_string(),
                "log" => "abc1234 fix\n".to_string(),
                "diff" if args.iter().any(|arg| arg == "HEAD..origin/main") => {
                    "index.html\n".to_string()
                }
                "diff" => "index.html\n".to_string(),
                "merge" => "Fast-forward\n".to_string(),
                _ => String::new(),
            };
            Ok(Output {
                exit_code: 0,
                stdout,
                stderr: String::new(),
            })
        }
    }

    struct NoServices;

    impl ServiceControl for NoServices {
        fn exists(&self, _name: &str) -> Result<bool, String> {
            Ok(false)
        }
        fn is_running(&self, _name: &str) -> Result<bool, String> {
            Ok(false)
        }
        fn stop(&self, _name: &str, _timeout: Duration) -> Result<(), String> {
            Ok(())
        }
        fn start(&self, _name: &str, _timeout: Duration) -> Result<(), String> {
            Ok(())
        }
        fn restart(&self, _name: &str, _timeout: Duration) -> Result<(), String> {
            Ok(())
        }
    }

    struct NotElevated;

    impl ServiceControl for NotElevated {
        fn exists(&self, _name: &str) -> Result<bool, String> {
            Ok(true)
        }
        fn is_running(&self, _name: &str) -> Result<bool, String> {
            Ok(false)
        }
        fn stop(&self, _name: &str, _timeout: Duration) -> Result<(), String> {
            Ok(())
        }
        fn start(&self, _name: &str, _timeout: Duration) -> Result<(), String> {
            Ok(())
        }
        fn restart(&self, _name: &str, _timeout: Duration) -> Result<(), String> {
            Ok(())
        }
        fn is_elevated(&self) -> Result<bool, String> {
            Ok(false)
        }
    }

    #[test]
    fn non_elevated_run_with_service_apps_aborts_before_git() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("app");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(temp.path().join("data")).unwrap();
        let config_path = temp.path().join("data/projects.json");
        let mut config = UpdaterConfig::default();
        config.apps.push(AppConfig {
            id: "demo".into(),
            repo_url: "https://example.com/demo.git".into(),
            local_path: project.to_string_lossy().to_string(),
            branch: "main".into(),
            preset: Preset::Static,
            composer: sm_core::ToggleMode::Never,
            package_manager: crate::registry::PackageManager::None,
            package_install: sm_core::ToggleMode::Never,
            build: sm_core::ToggleMode::Never,
            services: crate::registry::ServicePlan {
                restart_after: vec!["api".into()],
                ..crate::registry::ServicePlan::default()
            },
            ..AppConfig::default()
        });

        let error = run_registry_with(
            &config,
            &config_path,
            &FakeRunner::new(),
            &NotElevated,
            RunOptions::default(),
        )
        .unwrap_err();

        assert!(error.contains("elevated"));
        assert!(!config_path
            .parent()
            .unwrap()
            .join("updater-state.json")
            .exists());
    }

    #[test]
    fn run_registry_updates_static_app_and_writes_state() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("app");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(temp.path().join("data")).unwrap();
        let config_path = temp.path().join("data/projects.json");
        let config = UpdaterConfig {
            settings: crate::registry::UpdaterSettings {
                git_path: temp.path().join("git.exe").to_string_lossy().to_string(),
                log_dir: temp.path().join("logs").to_string_lossy().to_string(),
                ..crate::registry::UpdaterSettings::default()
            },
            trigger: crate::registry::TriggerSettings {
                allowed_client_ips: Vec::new(),
                allow_local: true,
            },
            apps: vec![AppConfig {
                id: "demo".into(),
                name: "Demo".into(),
                repo_url: "https://example.com/demo.git".into(),
                local_path: project.to_string_lossy().to_string(),
                branch: "main".into(),
                preset: Preset::Static,
                composer: sm_core::ToggleMode::Never,
                package_manager: crate::registry::PackageManager::None,
                package_install: sm_core::ToggleMode::Never,
                build: sm_core::ToggleMode::Never,
                ..AppConfig::default()
            }],
            ..UpdaterConfig::default()
        };
        std::fs::write(temp.path().join("git.exe"), b"fake").unwrap();
        save_config(&config_path, &config).unwrap();

        let outcomes = run_registry_with(
            &config,
            &config_path,
            &FakeRunner::new(),
            &NoServices,
            RunOptions::default(),
        )
        .unwrap();

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].status, "updated");
        let state = load_state(&config_path);
        assert_eq!(state.apps["demo"].status, "updated");
        assert_eq!(state.apps["demo"].last_commit, "bbbb2222");
        assert!(!lock_path(&config_path).exists());
    }

    #[test]
    fn settings_json_overrides_legacy_paths_and_log_dir() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("app");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(temp.path().join("data")).unwrap();
        let config_path = temp.path().join("data/projects.json");

        let legacy_git = temp.path().join("legacy-git.exe");
        let settings_git = temp.path().join("settings-git.exe");
        std::fs::write(&legacy_git, b"fake").unwrap();
        std::fs::write(&settings_git, b"fake").unwrap();
        let legacy_logs = temp.path().join("legacy-logs");
        let settings_logs = temp.path().join("settings-logs");

        let config = UpdaterConfig {
            settings: crate::registry::UpdaterSettings {
                git_path: legacy_git.to_string_lossy().to_string(),
                php_path: temp.path().join("php.exe").to_string_lossy().to_string(),
                composer_path: temp
                    .path()
                    .join("composer.bat")
                    .to_string_lossy()
                    .to_string(),
                npm_path: temp.path().join("npm.cmd").to_string_lossy().to_string(),
                log_dir: legacy_logs.to_string_lossy().to_string(),
                ..crate::registry::UpdaterSettings::default()
            },
            trigger: crate::registry::TriggerSettings {
                allowed_client_ips: Vec::new(),
                allow_local: true,
            },
            apps: vec![AppConfig {
                id: "demo".into(),
                name: "Demo".into(),
                repo_url: "https://example.com/demo.git".into(),
                local_path: project.to_string_lossy().to_string(),
                branch: "main".into(),
                preset: Preset::Static,
                composer: sm_core::ToggleMode::Never,
                package_manager: crate::registry::PackageManager::None,
                package_install: sm_core::ToggleMode::Never,
                build: sm_core::ToggleMode::Never,
                ..AppConfig::default()
            }],
            ..UpdaterConfig::default()
        };
        save_config(&config_path, &config).unwrap();

        let mut global = crate::settings::Settings {
            tools: crate::settings::Tools {
                git: crate::settings::ToolPath::new(settings_git.to_string_lossy().to_string()),
                ..crate::settings::Tools::default()
            },
            ..crate::settings::Settings::default()
        };
        global.updater.log_dir = settings_logs.to_string_lossy().to_string();
        crate::settings::save_settings(&temp.path().join("data/settings.json"), &global).unwrap();

        let runner = FakeRunner::new();
        let outcomes = run_registry_with(
            &config,
            &config_path,
            &runner,
            &NoServices,
            RunOptions::default(),
        )
        .unwrap();

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].status, "updated");
        let programs = runner.programs();
        assert!(!programs.is_empty());
        let expected_git = settings_git.to_string_lossy().to_string();
        assert!(
            programs.iter().all(|program| program == &expected_git),
            "all git calls must use settings.json path: {programs:?}"
        );
        assert!(settings_logs.is_dir());
        assert!(std::fs::read_dir(&settings_logs).unwrap().any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("project-update-")));
        assert!(!legacy_logs.exists());
    }
}
