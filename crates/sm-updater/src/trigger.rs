use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use std::net::IpAddr;

use crate::app_update::update_app;
use crate::history::{append_history, HistoryEntry};
use crate::ip::{app_allowed_for_ip_with_trigger, client_ip_from_env};
use crate::lock::UpdateLock;
use crate::registry::{AppConfig, TriggerSettings, UpdaterConfig, UpdaterSettings};
use crate::runner::Runner;
use crate::services::{ServiceControl, WindowsServiceControl};
use crate::settings::{effective_settings, effective_trigger, settings_path_for};
use crate::state::{load_state, mark, save_state, UpdaterState};
use crate::update::{UpdateOptions, UpdateOutcome, UpdateStatus};

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

struct SelectedApp<'a> {
    app: &'a AppConfig,
    allowed: bool,
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
    let settings_file = settings_path_for(config_path);
    let effective = effective_settings(&settings_file, &config.settings, &config.trigger)
        .map_err(|error| error.to_string())?;
    let updater_settings = effective.to_updater_settings();
    let trigger =
        effective_trigger(&settings_file, &config.trigger).map_err(|error| error.to_string())?;

    let log_dir = if updater_settings.log_dir.trim().is_empty() {
        default_log_dir(config_path)
    } else {
        PathBuf::from(updater_settings.log_dir.trim())
    };

    if !options.check_only && effective.maintenance.enabled {
        let mut message = String::from(
            "Maintenance mode is enabled: updates and the SSH trigger are paused. \
             Disable maintenance mode in Settings when ready.",
        );
        let custom = effective.maintenance.message.trim();
        if !custom.is_empty() {
            message.push_str(&format!(" Maintenance message: {custom}"));
        }
        return Err(message);
    }

    if let Some(requested) = &options.app {
        if config.find(requested).is_none() {
            return Err(format!("App '{requested}' tidak ditemukan di registry."));
        }
    }

    if !options.check_only
        && selected_apps_need_services(config, &options, &trigger, client_ip)
        && !services.is_elevated()?
    {
        return Err(
            "Update dengan service lifecycle membutuhkan Administrator/elevated. \
             Jalankan melalui Scheduled Task wrapper atau elevated terminal."
                .to_string(),
        );
    }

    let selected: Vec<SelectedApp> = config
        .apps
        .iter()
        .filter(|app| app.enabled)
        .filter(|app| {
            options
                .app
                .as_ref()
                .map(|requested| app.id.eq_ignore_ascii_case(requested))
                .unwrap_or(true)
        })
        .map(|app| SelectedApp {
            app,
            allowed: app_allowed_for_ip_with_trigger(app, &trigger, client_ip),
        })
        .collect();

    let allowed_count = selected.iter().filter(|app| app.allowed).count();
    // Stage 1: hanya check-only yang boleh paralel. Update/install/build/
    // service lifecycle tetap serial sampai desain stage 2 selesai.
    let parallel_check =
        options.check_only && updater_settings.max_concurrent_updates > 1 && allowed_count >= 2;

    if parallel_check {
        let allowed: Vec<(usize, &AppConfig)> = selected
            .iter()
            .enumerate()
            .filter(|(_, selected_app)| selected_app.allowed)
            .map(|(index, selected_app)| (index, selected_app.app))
            .collect();
        let max_concurrent = updater_settings.max_concurrent_updates.min(allowed.len());
        let mut updates = run_check_apps_parallel(
            &allowed,
            &updater_settings,
            &log_dir,
            runner,
            services,
            max_concurrent,
        );
        updates.sort_by_key(|(index, _, _)| *index);
        let mut updates = updates.into_iter().peekable();

        for (index, selected_app) in selected.iter().enumerate() {
            if !selected_app.allowed {
                outcomes.push(record_forbidden(
                    selected_app.app,
                    client_ip,
                    &mut state,
                    config_path,
                ));
                continue;
            }

            let matches_next = matches!(
                updates.peek(),
                Some((update_index, _, _)) if *update_index == index
            );
            let result = if matches_next {
                updates
                    .next()
                    .map(|(_, outcome, duration)| (outcome, duration))
            } else {
                None
            };
            let (outcome, duration_ms) = result.unwrap_or_else(|| {
                (
                    UpdateOutcome {
                        project: selected_app.app.id.clone(),
                        status: UpdateStatus::Failed,
                        message: format!(
                            "Check untuk app '{}' tidak mengembalikan hasil worker; status ditandai failed.",
                            selected_app.app.id
                        ),
                        log_path: None,
                    },
                    0,
                )
            });
            outcomes.push(record_app_outcome(
                selected_app.app,
                outcome,
                duration_ms,
                &mut state,
                config_path,
                &updater_settings,
                runner,
                client_ip,
            ));
        }
    } else {
        for selected_app in &selected {
            if !selected_app.allowed {
                outcomes.push(record_forbidden(
                    selected_app.app,
                    client_ip,
                    &mut state,
                    config_path,
                ));
                continue;
            }

            let started = Instant::now();
            let outcome = update_app(
                selected_app.app,
                &updater_settings,
                &log_dir,
                runner,
                services,
                UpdateOptions {
                    check_only: options.check_only,
                },
            );
            let duration_ms = started.elapsed().as_millis() as u64;
            outcomes.push(record_app_outcome(
                selected_app.app,
                outcome,
                duration_ms,
                &mut state,
                config_path,
                &updater_settings,
                runner,
                client_ip,
            ));
        }
    }

    save_state(config_path, &state)?;

    if outcomes.is_empty() {
        return Err("Tidak ada app yang cocok/diproses.".to_string());
    }
    Ok(outcomes)
}

/// Best-effort history write. A history failure must never fail the run or
/// change its outcome, so it is only reported on stderr.
#[allow(clippy::too_many_arguments)]
fn record_history(
    config_path: &Path,
    app_id: &str,
    status: &str,
    message: &str,
    commit: &str,
    duration_ms: u64,
    client_ip: Option<IpAddr>,
) {
    let entry = HistoryEntry {
        timestamp: chrono::Local::now().to_rfc3339(),
        app_id: app_id.to_string(),
        status: status.to_string(),
        message: message.to_string(),
        commit: commit.to_string(),
        duration_ms,
        client_ip: client_ip.map(|ip| ip.to_string()).unwrap_or_default(),
    };
    if let Err(error) = append_history(config_path, &entry) {
        eprintln!("Gagal menulis updater history untuk app '{app_id}': {error}");
    }
}

fn record_forbidden(
    app: &AppConfig,
    client_ip: Option<IpAddr>,
    state: &mut UpdaterState,
    config_path: &Path,
) -> RunOutcome {
    let message = match client_ip {
        Some(ip) => format!("IP {ip} tidak diizinkan untuk app '{}'.", app.id),
        None => format!("Trigger lokal tidak diizinkan untuk app '{}'.", app.id),
    };
    mark(state, &app.id, "forbidden", Some(&message), 0);
    record_history(
        config_path,
        &app.id,
        "forbidden",
        &message,
        "",
        0,
        client_ip,
    );
    RunOutcome {
        app_id: app.id.clone(),
        status: "forbidden".to_string(),
        message,
        forbidden: true,
    }
}

/// Tulis state/history untuk satu app. Selalu dipanggil dari thread utama,
/// termasuk untuk hasil worker check paralel, agar file state dan history tidak
/// ditulis bersamaan.
#[allow(clippy::too_many_arguments)]
fn record_app_outcome(
    app: &AppConfig,
    outcome: UpdateOutcome,
    duration_ms: u64,
    state: &mut UpdaterState,
    config_path: &Path,
    settings: &UpdaterSettings,
    runner: &dyn Runner,
    client_ip: Option<IpAddr>,
) -> RunOutcome {
    let status = outcome.status.as_str().to_string();
    mark(
        state,
        &app.id,
        &status,
        if matches!(
            outcome.status,
            UpdateStatus::Failed | UpdateStatus::Degraded | UpdateStatus::BlockedMigration
        ) {
            Some(&outcome.message)
        } else {
            None
        },
        duration_ms,
    );

    let commit = head_commit(&settings.git_path, Path::new(&app.local_path), runner);
    if let Some(commit) = &commit {
        if let Some(entry) = state.apps.get_mut(&app.id) {
            entry.last_commit = commit.clone();
        }
    }
    record_history(
        config_path,
        &app.id,
        &status,
        &outcome.message,
        commit.as_deref().unwrap_or(""),
        duration_ms,
        client_ip,
    );
    RunOutcome {
        app_id: app.id.clone(),
        status,
        message: outcome.message,
        forbidden: false,
    }
}

/// Jalankan check-only untuk app terpilih dengan worker pool terbatas.
/// Hanya `update_app(check_only)` yang berjalan di worker; state/history
/// ditulis setelah semua worker selesai.
fn run_check_apps_parallel(
    apps: &[(usize, &AppConfig)],
    settings: &UpdaterSettings,
    log_dir: &Path,
    runner: &dyn Runner,
    services: &dyn ServiceControl,
    max_concurrent: usize,
) -> Vec<(usize, UpdateOutcome, u64)> {
    if apps.is_empty() {
        return Vec::new();
    }
    let workers = max_concurrent.clamp(1, apps.len());
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<(usize, UpdateOutcome, u64)>> =
        Mutex::new(Vec::with_capacity(apps.len()));

    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| loop {
                let position = next.fetch_add(1, Ordering::SeqCst);
                if position >= apps.len() {
                    break;
                }
                let (index, app) = apps[position];
                let started = Instant::now();
                // Satu worker yang panic tidak boleh menggagalkan seluruh run;
                // app tersebut dicatat failed oleh thread utama.
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    update_app(
                        app,
                        settings,
                        log_dir,
                        runner,
                        services,
                        UpdateOptions { check_only: true },
                    )
                }));
                let duration_ms = started.elapsed().as_millis() as u64;
                let outcome = match outcome {
                    Ok(outcome) => outcome,
                    Err(_) => UpdateOutcome {
                        project: app.id.clone(),
                        status: UpdateStatus::Failed,
                        message: format!(
                            "Worker check untuk app '{}' berhenti tidak normal; status ditandai failed.",
                            app.id
                        ),
                        log_path: None,
                    },
                };
                results
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push((index, outcome, duration_ms));
            });
        }
    });

    results
        .into_inner()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
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
    trigger: &TriggerSettings,
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
        .filter(|app| app_allowed_for_ip_with_trigger(app, trigger, client_ip))
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
    use crate::history::load_history;
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

    /// Runner check-only dengan penghitung konkurensi. `barrier` memaksa
    /// sejumlah panggilan `status` bertemu sebelum lanjut; `delay` memperlebar
    /// jendela overlap untuk test serial.
    struct CheckRunner {
        barrier: Option<std::sync::Barrier>,
        delay: Duration,
        active: AtomicUsize,
        max_active: AtomicUsize,
        merged: Mutex<std::collections::BTreeSet<PathBuf>>,
    }

    impl CheckRunner {
        fn barrier(parties: usize) -> Self {
            Self {
                barrier: Some(std::sync::Barrier::new(parties)),
                delay: Duration::ZERO,
                active: AtomicUsize::new(0),
                max_active: AtomicUsize::new(0),
                merged: Mutex::new(std::collections::BTreeSet::new()),
            }
        }

        fn delayed(delay: Duration) -> Self {
            Self {
                barrier: None,
                delay,
                active: AtomicUsize::new(0),
                max_active: AtomicUsize::new(0),
                merged: Mutex::new(std::collections::BTreeSet::new()),
            }
        }

        fn max_active(&self) -> usize {
            self.max_active.load(Ordering::SeqCst)
        }
    }

    impl Runner for CheckRunner {
        fn run(
            &self,
            _program: &Path,
            args: &[String],
            cwd: &Path,
            _timeout: Duration,
        ) -> Result<Output, RunnerError> {
            let first = args.first().map(String::as_str).unwrap_or("");
            if first == "status" {
                let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
                self.max_active.fetch_max(active, Ordering::SeqCst);
                if let Some(barrier) = &self.barrier {
                    barrier.wait();
                }
                if !self.delay.is_zero() {
                    std::thread::sleep(self.delay);
                }
                self.active.fetch_sub(1, Ordering::SeqCst);
                return Ok(Output {
                    exit_code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                });
            }

            if first == "merge" {
                self.merged.lock().unwrap().insert(cwd.to_path_buf());
            }
            let stdout = match first {
                "rev-parse" if args.get(1).map(String::as_str) == Some("--abbrev-ref") => {
                    "main\n".to_string()
                }
                "rev-parse" if args.get(1).map(String::as_str) == Some("HEAD") => {
                    if self.merged.lock().unwrap().contains(cwd) {
                        "def5678\n".to_string()
                    } else {
                        "abc1234\n".to_string()
                    }
                }
                "rev-parse" => "ok\n".to_string(),
                "ls-remote" => "refs/heads/main\n".to_string(),
                "log" => "abc1234 fix\n".to_string(),
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

    fn check_config(
        temp: &tempfile::TempDir,
        apps: Vec<AppConfig>,
        max_concurrent_updates: usize,
    ) -> (UpdaterConfig, PathBuf) {
        std::fs::create_dir_all(temp.path().join("data")).unwrap();
        std::fs::write(temp.path().join("git.exe"), b"fake").unwrap();
        let config_path = temp.path().join("data/projects.json");
        let config = UpdaterConfig {
            settings: crate::registry::UpdaterSettings {
                git_path: temp.path().join("git.exe").to_string_lossy().to_string(),
                log_dir: temp.path().join("logs").to_string_lossy().to_string(),
                max_concurrent_updates,
                ..crate::registry::UpdaterSettings::default()
            },
            trigger: crate::registry::TriggerSettings {
                allowed_client_ips: Vec::new(),
                allow_local: true,
            },
            apps,
            ..UpdaterConfig::default()
        };
        save_config(&config_path, &config).unwrap();
        (config, config_path)
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

        let history = load_history(&config_path, None, 10);
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].app_id, "demo");
        assert_eq!(history[0].status, "updated");
        assert_eq!(history[0].commit, "bbbb2222");
        assert!(history[0].client_ip.is_empty());
        assert!(!history[0].timestamp.is_empty());
    }

    #[test]
    fn history_records_allowed_and_forbidden_apps_with_client_ip() {
        let temp = tempfile::tempdir().unwrap();
        let allowed_project = temp.path().join("allowed");
        let denied_project = temp.path().join("denied");
        std::fs::create_dir_all(&allowed_project).unwrap();
        std::fs::create_dir_all(&denied_project).unwrap();
        std::fs::create_dir_all(temp.path().join("data")).unwrap();
        let config_path = temp.path().join("data/projects.json");
        std::fs::write(temp.path().join("git.exe"), b"fake").unwrap();

        let mut denied = static_app("denied", &denied_project);
        denied.allowed_client_ips = vec!["10.0.0.9".into()];
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
            apps: vec![static_app("allowed", &allowed_project), denied],
            ..UpdaterConfig::default()
        };
        save_config(&config_path, &config).unwrap();

        let outcomes = run_registry_with(
            &config,
            &config_path,
            &FakeRunner::new(),
            &NoServices,
            RunOptions {
                client_ip: Some("10.255.240.143".parse().unwrap()),
                ..RunOptions::default()
            },
        )
        .unwrap();

        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].status, "updated");
        assert_eq!(outcomes[1].status, "forbidden");

        let history = load_history(&config_path, None, 50);
        assert_eq!(history.len(), 2);
        let denied_entry = history
            .iter()
            .find(|entry| entry.app_id == "denied")
            .expect("forbidden app history entry");
        assert_eq!(denied_entry.status, "forbidden");
        assert_eq!(denied_entry.client_ip, "10.255.240.143");
        assert!(denied_entry.commit.is_empty());
        let allowed_entry = history
            .iter()
            .find(|entry| entry.app_id == "allowed")
            .expect("allowed app history entry");
        assert_eq!(allowed_entry.status, "updated");
        assert_eq!(allowed_entry.client_ip, "10.255.240.143");
        assert_eq!(allowed_entry.commit, "bbbb2222");
    }

    #[test]
    fn history_write_failure_does_not_fail_the_run() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("app");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(temp.path().join("data")).unwrap();
        let config_path = temp.path().join("data/projects.json");
        // Make the history destination a directory so appending must fail.
        std::fs::create_dir_all(temp.path().join("data/updater-history.jsonl")).unwrap();
        std::fs::write(temp.path().join("git.exe"), b"fake").unwrap();

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
            apps: vec![static_app("demo", &project)],
            ..UpdaterConfig::default()
        };
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
        assert_eq!(load_state(&config_path).apps["demo"].status, "updated");
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

    fn static_app(id: &str, path: &Path) -> AppConfig {
        AppConfig {
            id: id.into(),
            name: id.into(),
            repo_url: "https://example.com/demo.git".into(),
            local_path: path.to_string_lossy().to_string(),
            branch: "main".into(),
            preset: Preset::Static,
            composer: sm_core::ToggleMode::Never,
            package_manager: crate::registry::PackageManager::None,
            package_install: sm_core::ToggleMode::Never,
            build: sm_core::ToggleMode::Never,
            ..AppConfig::default()
        }
    }

    #[test]
    fn maintenance_mode_blocks_update_without_state_or_history() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("app");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(temp.path().join("data")).unwrap();
        let config_path = temp.path().join("data/projects.json");
        std::fs::write(temp.path().join("git.exe"), b"fake").unwrap();

        let config = UpdaterConfig {
            settings: crate::registry::UpdaterSettings {
                git_path: temp.path().join("git.exe").to_string_lossy().to_string(),
                log_dir: temp.path().join("logs").to_string_lossy().to_string(),
                ..crate::registry::UpdaterSettings::default()
            },
            apps: vec![static_app("demo", &project)],
            ..UpdaterConfig::default()
        };
        save_config(&config_path, &config).unwrap();

        let mut settings = crate::settings::Settings::default();
        settings.maintenance.enabled = true;
        settings.maintenance.message = "Deploy window until 17:00".into();
        crate::settings::save_settings(&temp.path().join("data/settings.json"), &settings).unwrap();

        let runner = FakeRunner::new();
        let error = run_registry_with(
            &config,
            &config_path,
            &runner,
            &NoServices,
            RunOptions::default(),
        )
        .unwrap_err();

        assert!(error.contains("Maintenance mode"), "{error}");
        assert!(error.contains("Deploy window until 17:00"), "{error}");
        assert!(
            runner.programs().is_empty(),
            "no Git command may run while maintenance mode blocks the update"
        );
        assert!(!temp.path().join("data/updater-state.json").exists());
        assert!(!temp.path().join("data/updater-history.jsonl").exists());
        assert!(!lock_path(&config_path).exists());
        assert!(!temp.path().join("logs").exists());
    }

    #[test]
    fn maintenance_mode_without_message_still_blocks_and_allows_check() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("app");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(temp.path().join("data")).unwrap();
        let config_path = temp.path().join("data/projects.json");
        std::fs::write(temp.path().join("git.exe"), b"fake").unwrap();

        let config = UpdaterConfig {
            settings: crate::registry::UpdaterSettings {
                git_path: temp.path().join("git.exe").to_string_lossy().to_string(),
                log_dir: temp.path().join("logs").to_string_lossy().to_string(),
                ..crate::registry::UpdaterSettings::default()
            },
            apps: vec![static_app("demo", &project)],
            ..UpdaterConfig::default()
        };
        save_config(&config_path, &config).unwrap();

        let mut settings = crate::settings::Settings::default();
        settings.maintenance.enabled = true;
        crate::settings::save_settings(&temp.path().join("data/settings.json"), &settings).unwrap();

        let error = run_registry_with(
            &config,
            &config_path,
            &FakeRunner::new(),
            &NoServices,
            RunOptions::default(),
        )
        .unwrap_err();
        assert!(error.contains("Maintenance mode"), "{error}");
        assert!(!error.contains("Maintenance message"));
        assert!(!temp.path().join("data/updater-state.json").exists());

        let outcomes = run_registry_with(
            &config,
            &config_path,
            &FakeRunner::new(),
            &NoServices,
            RunOptions {
                check_only: true,
                ..RunOptions::default()
            },
        )
        .unwrap();
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].status, "check");
    }

    #[test]
    fn maintenance_mode_is_ignored_when_settings_are_absent() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("app");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(temp.path().join("data")).unwrap();
        let config_path = temp.path().join("data/projects.json");
        std::fs::write(temp.path().join("git.exe"), b"fake").unwrap();

        let config = UpdaterConfig {
            settings: crate::registry::UpdaterSettings {
                git_path: temp.path().join("git.exe").to_string_lossy().to_string(),
                log_dir: temp.path().join("logs").to_string_lossy().to_string(),
                ..crate::registry::UpdaterSettings::default()
            },
            apps: vec![static_app("demo", &project)],
            ..UpdaterConfig::default()
        };
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
        assert!(!temp.path().join("data/settings.json").exists());
    }

    #[test]
    fn settings_trigger_overrides_projects_trigger_for_authorization() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("app");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(temp.path().join("data")).unwrap();
        let config_path = temp.path().join("data/projects.json");

        let config = UpdaterConfig {
            // Legacy project trigger allows every IP.
            trigger: crate::registry::TriggerSettings {
                allowed_client_ips: Vec::new(),
                allow_local: true,
            },
            apps: vec![static_app("demo", &project)],
            ..UpdaterConfig::default()
        };
        save_config(&config_path, &config).unwrap();

        let mut settings = crate::settings::Settings::default();
        settings.trigger.allowed_client_ips = vec!["10.255.240.0/24".into()];
        crate::settings::save_settings(&temp.path().join("data/settings.json"), &settings).unwrap();

        let outcomes = run_registry_with(
            &config,
            &config_path,
            &FakeRunner::new(),
            &NoServices,
            RunOptions {
                client_ip: Some("10.0.0.9".parse().unwrap()),
                ..RunOptions::default()
            },
        )
        .unwrap();

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].status, "forbidden");
        assert!(outcomes[0].message.contains("10.0.0.9"));
    }

    #[test]
    fn projects_trigger_is_used_when_settings_json_is_absent() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("app");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(temp.path().join("data")).unwrap();
        let config_path = temp.path().join("data/projects.json");

        let config = UpdaterConfig {
            trigger: crate::registry::TriggerSettings {
                allowed_client_ips: vec!["10.255.240.0/24".into()],
                allow_local: true,
            },
            apps: vec![static_app("demo", &project)],
            ..UpdaterConfig::default()
        };
        save_config(&config_path, &config).unwrap();

        let outcomes = run_registry_with(
            &config,
            &config_path,
            &FakeRunner::new(),
            &NoServices,
            RunOptions {
                client_ip: Some("10.0.0.9".parse().unwrap()),
                ..RunOptions::default()
            },
        )
        .unwrap();

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].status, "forbidden");
        assert!(!temp.path().join("data/settings.json").exists());
    }

    #[test]
    fn per_app_allowlist_overrides_settings_trigger() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("app");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(temp.path().join("data")).unwrap();
        let config_path = temp.path().join("data/projects.json");

        let mut app = static_app("demo", &project);
        app.allowed_client_ips = vec!["10.0.0.9".into()];
        let config = UpdaterConfig {
            trigger: crate::registry::TriggerSettings {
                allowed_client_ips: Vec::new(),
                allow_local: true,
            },
            apps: vec![app],
            ..UpdaterConfig::default()
        };
        save_config(&config_path, &config).unwrap();

        let mut settings = crate::settings::Settings::default();
        settings.trigger.allowed_client_ips = vec!["10.255.240.0/24".into()];
        crate::settings::save_settings(&temp.path().join("data/settings.json"), &settings).unwrap();

        let outcomes = run_registry_with(
            &config,
            &config_path,
            &FakeRunner::new(),
            &NoServices,
            RunOptions {
                check_only: true,
                client_ip: Some("10.0.0.9".parse().unwrap()),
                ..RunOptions::default()
            },
        )
        .unwrap();

        assert_eq!(outcomes.len(), 1);
        assert_ne!(outcomes[0].status, "forbidden");
    }

    #[test]
    fn check_all_runs_two_apps_concurrently_when_max_is_two() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let (config, config_path) = check_config(
            &temp,
            vec![static_app("first", &first), static_app("second", &second)],
            2,
        );
        let runner = CheckRunner::barrier(2);

        let outcomes = run_registry_with(
            &config,
            &config_path,
            &runner,
            &NoServices,
            RunOptions {
                check_only: true,
                ..RunOptions::default()
            },
        )
        .unwrap();

        assert_eq!(
            outcomes
                .iter()
                .map(|outcome| outcome.app_id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second"]
        );
        assert!(outcomes.iter().all(|outcome| outcome.status == "check"));
        assert_eq!(
            runner.max_active(),
            2,
            "both check workers must be inside git status at the same time"
        );
    }

    #[test]
    fn check_all_stays_serial_when_max_is_one() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let (config, config_path) = check_config(
            &temp,
            vec![static_app("first", &first), static_app("second", &second)],
            1,
        );
        let runner = CheckRunner::delayed(Duration::from_millis(25));

        let outcomes = run_registry_with(
            &config,
            &config_path,
            &runner,
            &NoServices,
            RunOptions {
                check_only: true,
                ..RunOptions::default()
            },
        )
        .unwrap();

        assert_eq!(outcomes.len(), 2);
        assert!(outcomes.iter().all(|outcome| outcome.status == "check"));
        assert_eq!(runner.max_active(), 1);
    }

    #[test]
    fn normal_update_stays_serial_even_when_max_is_two() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir_all(&first).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        let (config, config_path) = check_config(
            &temp,
            vec![static_app("first", &first), static_app("second", &second)],
            2,
        );
        let runner = CheckRunner::delayed(Duration::from_millis(25));

        let outcomes = run_registry_with(
            &config,
            &config_path,
            &runner,
            &NoServices,
            RunOptions::default(),
        )
        .unwrap();

        assert_eq!(outcomes.len(), 2);
        assert!(outcomes.iter().all(|outcome| outcome.status == "updated"));
        assert_eq!(
            runner.max_active(),
            1,
            "normal updates must not use the check-only worker pool"
        );
    }

    #[test]
    fn parallel_check_records_state_and_history_in_config_order() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        let third = temp.path().join("third");
        for project in [&first, &second, &third] {
            std::fs::create_dir_all(project).unwrap();
        }
        let mut forbidden = static_app("first", &first);
        forbidden.allowed_client_ips = vec!["10.0.0.9".into()];
        let (config, config_path) = check_config(
            &temp,
            vec![
                forbidden,
                static_app("second", &second),
                static_app("third", &third),
            ],
            2,
        );
        let runner = CheckRunner::delayed(Duration::from_millis(10));

        let outcomes = run_registry_with(
            &config,
            &config_path,
            &runner,
            &NoServices,
            RunOptions {
                check_only: true,
                client_ip: Some("10.1.2.3".parse().unwrap()),
                ..RunOptions::default()
            },
        )
        .unwrap();

        assert_eq!(
            outcomes
                .iter()
                .map(|outcome| (outcome.app_id.as_str(), outcome.status.as_str()))
                .collect::<Vec<_>>(),
            [
                ("first", "forbidden"),
                ("second", "check"),
                ("third", "check")
            ]
        );

        let state = load_state(&config_path);
        assert_eq!(state.apps.len(), 3);
        assert_eq!(state.apps["first"].status, "forbidden");
        assert_eq!(state.apps["second"].status, "check");
        assert_eq!(state.apps["third"].status, "check");
        assert_eq!(state.apps["second"].last_commit, "abc1234");
        assert_eq!(state.apps["third"].last_commit, "abc1234");

        let history = load_history(&config_path, None, 10);
        assert_eq!(history.len(), 3);
        let file_order: Vec<_> = history
            .iter()
            .rev()
            .map(|entry| (entry.app_id.as_str(), entry.status.as_str()))
            .collect();
        assert_eq!(
            file_order,
            [
                ("first", "forbidden"),
                ("second", "check"),
                ("third", "check")
            ]
        );
    }

    #[test]
    fn check_only_with_forbidden_and_allowed_app_records_both() {
        let temp = tempfile::tempdir().unwrap();
        let denied_project = temp.path().join("denied");
        let allowed_project = temp.path().join("allowed");
        std::fs::create_dir_all(&denied_project).unwrap();
        std::fs::create_dir_all(&allowed_project).unwrap();
        let mut denied = static_app("denied", &denied_project);
        denied.allowed_client_ips = vec!["10.0.0.9".into()];
        let (config, config_path) = check_config(
            &temp,
            vec![denied, static_app("allowed", &allowed_project)],
            4,
        );

        let outcomes = run_registry_with(
            &config,
            &config_path,
            &CheckRunner::delayed(Duration::ZERO),
            &NoServices,
            RunOptions {
                check_only: true,
                client_ip: Some("10.1.2.3".parse().unwrap()),
                ..RunOptions::default()
            },
        )
        .unwrap();

        assert_eq!(
            outcomes
                .iter()
                .map(|outcome| (outcome.app_id.as_str(), outcome.status.as_str()))
                .collect::<Vec<_>>(),
            [("denied", "forbidden"), ("allowed", "check")]
        );
        assert!(outcomes[0].forbidden);
        assert!(!outcomes[1].forbidden);

        let history = load_history(&config_path, None, 10);
        assert_eq!(history.len(), 2);
        assert!(history.iter().any(|entry| entry.app_id == "denied"
            && entry.status == "forbidden"
            && entry.client_ip == "10.1.2.3"));
        assert!(history
            .iter()
            .any(|entry| entry.app_id == "allowed" && entry.status == "check"));
    }
}
