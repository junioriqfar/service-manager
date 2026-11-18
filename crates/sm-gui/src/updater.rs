use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sm_config::ConfigPaths;
use sm_core::{ServiceType, ToggleMode};
use sm_updater::history::{load_history, HistoryEntry};
use sm_updater::plan::{plan_app, AppPlan};
use sm_updater::registry::{
    load_config, save_config, AppConfig, HealthCheck, NotifyConfig, PackageManager, Preset,
    ServicePlan, UpdaterConfig,
};
use sm_updater::runner::{Runner, SystemRunner};
use sm_updater::services::{ServiceControl, WindowsServiceControl};
use sm_updater::settings::{effective_settings, settings_path_for, Settings};
use sm_updater::state::{load_state, AppState};
use sm_updater::toolchain::{detect_package_manager, detect_preset};
use sm_updater::trigger::{default_log_dir, run_registry_with, RunOptions};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterServicePlanView {
    pub stop_before_pull: Vec<String>,
    pub stop_before_build: Vec<String>,
    pub restart_after: Vec<String>,
    pub start_after: Vec<String>,
    pub wait_timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterHealthView {
    pub enabled: bool,
    pub url: String,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterNotifyView {
    pub webhook_url: String,
    pub on: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterAppView {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub repo_url: String,
    pub local_path: String,
    pub branch: String,
    pub preset: String,
    pub package_manager: String,
    pub composer: String,
    pub artisan: String,
    pub package_install: String,
    pub build: String,
    pub install_command: String,
    pub build_command: String,
    pub queue_restart: bool,
    pub clone_on_first_run: bool,
    /// Keep the acronym spelling used by the frontend; camelCase alone would
    /// produce `allowedClientIps` and make the edit form crash on `.join()`.
    #[serde(rename = "allowedClientIPs", alias = "allowedClientIps")]
    pub allowed_client_ips: Vec<String>,
    pub dirty_ignore: Vec<String>,
    /// Service Apache di `services.json`; kosong berarti tidak terhubung eksplisit.
    pub web_server_service: String,
    /// Versi PHP efektif dari settings.json, diisi untuk preset Laravel.
    pub php_version: String,
    /// Versi Apache efektif dari settings.json tools.apache.version.
    pub apache_version: String,
    pub services: UpdaterServicePlanView,
    pub health_check: UpdaterHealthView,
    pub notify: UpdaterNotifyView,
    pub status: String,
    pub last_checked_at: String,
    pub last_updated_at: String,
    pub last_commit: String,
    pub last_error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterRunOutcomeView {
    pub app_id: String,
    pub status: String,
    pub message: String,
    pub forbidden: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterLogView {
    pub path: String,
    pub text: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterAppInput {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub repo_url: String,
    #[serde(default)]
    pub local_path: String,
    #[serde(default)]
    pub branch: String,
    #[serde(default)]
    pub preset: Preset,
    #[serde(default)]
    pub package_manager: PackageManager,
    #[serde(default)]
    pub composer: ToggleMode,
    #[serde(default)]
    pub artisan: ToggleMode,
    #[serde(default)]
    pub package_install: ToggleMode,
    #[serde(default)]
    pub build: ToggleMode,
    #[serde(default)]
    pub install_command: String,
    #[serde(default)]
    pub build_command: String,
    #[serde(default = "default_true")]
    pub queue_restart: bool,
    #[serde(default)]
    pub clone_on_first_run: Option<bool>,
    #[serde(default, rename = "allowedClientIPs", alias = "allowedClientIps")]
    pub allowed_client_ips: Vec<String>,
    #[serde(default)]
    pub dirty_ignore: Vec<String>,
    #[serde(default)]
    pub web_server_service: String,
    #[serde(default)]
    pub services: ServicePlan,
    #[serde(default)]
    pub health_check: Option<HealthCheck>,
    #[serde(default)]
    pub notify: Option<NotifyConfig>,
}

fn default_true() -> bool {
    true
}

fn config_path() -> PathBuf {
    ConfigPaths::detect().projects_path()
}

fn toggle_name(value: ToggleMode) -> &'static str {
    match value {
        ToggleMode::Auto => "auto",
        ToggleMode::Always => "always",
        ToggleMode::Never => "never",
        ToggleMode::Unknown => "unknown",
    }
}

fn preset_name(value: Preset) -> &'static str {
    match value {
        Preset::Auto => "auto",
        Preset::Laravel => "laravel",
        Preset::Node => "node",
        Preset::Bun => "bun",
        Preset::Go => "go",
        Preset::Static => "static",
        Preset::Custom => "custom",
        Preset::Unknown => "unknown",
    }
}

fn package_manager_name(value: PackageManager) -> &'static str {
    match value {
        PackageManager::Auto => "auto",
        PackageManager::Npm => "npm",
        PackageManager::Pnpm => "pnpm",
        PackageManager::Yarn => "yarn",
        PackageManager::Bun => "bun",
        PackageManager::None => "none",
        PackageManager::Unknown => "unknown",
    }
}

fn view(app: &AppConfig, state: Option<&AppState>, settings: &Settings) -> UpdaterAppView {
    let preset = if app.preset == Preset::Auto {
        detect_preset(Path::new(&app.local_path))
    } else {
        app.preset
    };
    let php_version = if preset == Preset::Laravel {
        settings.tools.php.version.trim().to_string()
    } else {
        String::new()
    };
    let apache_version = if app.web_server_service.trim().is_empty() {
        String::new()
    } else {
        settings.tools.apache.tool.version.trim().to_string()
    };
    UpdaterAppView {
        id: app.id.clone(),
        name: app.name.clone(),
        enabled: app.enabled,
        repo_url: app.repo_url.clone(),
        local_path: app.local_path.clone(),
        branch: app.branch.clone(),
        preset: preset_name(preset).to_string(),
        package_manager: package_manager_name(app.package_manager).to_string(),
        composer: toggle_name(app.composer).to_string(),
        artisan: toggle_name(app.artisan).to_string(),
        package_install: toggle_name(app.package_install).to_string(),
        build: toggle_name(app.build).to_string(),
        install_command: app.install_command.clone(),
        build_command: app.build_command.clone(),
        queue_restart: app.queue_restart,
        clone_on_first_run: app.clone_on_first_run,
        allowed_client_ips: app.allowed_client_ips.clone(),
        dirty_ignore: app.dirty_ignore.clone(),
        web_server_service: app.web_server_service.clone(),
        php_version,
        apache_version,
        services: UpdaterServicePlanView {
            stop_before_pull: app.services.stop_before_pull.clone(),
            stop_before_build: app.services.stop_before_build.clone(),
            restart_after: app.services.restart_after.clone(),
            start_after: app.services.start_after.clone(),
            wait_timeout_seconds: app.services.wait_timeout_seconds,
        },
        health_check: UpdaterHealthView {
            enabled: app.health_check.enabled,
            url: app.health_check.url.clone(),
            timeout_seconds: app.health_check.timeout_seconds,
        },
        notify: UpdaterNotifyView {
            webhook_url: app.notify.webhook_url.clone(),
            on: app.notify.on.clone(),
        },
        status: state
            .map(|value| value.status.clone())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "idle".to_string()),
        last_checked_at: state
            .map(|value| value.last_checked_at.clone())
            .unwrap_or_default(),
        last_updated_at: state
            .map(|value| value.last_updated_at.clone())
            .unwrap_or_default(),
        last_commit: state
            .map(|value| value.last_commit.clone())
            .unwrap_or_default(),
        last_error: state
            .map(|value| value.last_error.clone())
            .unwrap_or_default(),
    }
}

pub(crate) fn load_views_at(path: &Path) -> Result<Vec<UpdaterAppView>, String> {
    let config = load_config(path)?;
    let state = load_state(path);
    let settings = effective_settings(&settings_path_for(path), &config.settings, &config.trigger)
        .map_err(|error| error.to_string())?;
    Ok(config
        .apps
        .iter()
        .map(|app| view(app, state.apps.get(&app.id), &settings))
        .collect())
}

fn services_backup_dir(path: &Path) -> PathBuf {
    path.parent()
        .and_then(|parent| parent.parent())
        .unwrap_or_else(|| Path::new("."))
        .join("backup")
}

fn validate_service_names(path: &Path, services: &[String]) -> Result<(), String> {
    if services.is_empty() {
        return Ok(());
    }
    let backup_dir = services_backup_dir(path);
    let result = sm_config::load_services_lenient(path, &backup_dir);
    let known: Vec<String> = result
        .services
        .iter()
        .map(|service| service.name.to_ascii_lowercase())
        .collect();
    let missing: Vec<String> = services
        .iter()
        .map(|service| service.trim())
        .filter(|service| !service.is_empty())
        .filter(|service| {
            !known
                .iter()
                .any(|name| name == &service.to_ascii_lowercase())
        })
        .map(|service| service.to_string())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Service not found in data/services.json: {}",
            missing.join(", ")
        ))
    }
}

/// `webServerService` opsional, tetapi jika diisi harus menunjuk service
/// dengan `serviceType=apache` di `data/services.json`.
fn validate_web_server_service(path: &Path, service: &str) -> Result<(), String> {
    let service = service.trim();
    if service.is_empty() {
        return Ok(());
    }
    let result = sm_config::load_services_lenient(path, &services_backup_dir(path));
    if let Some(warning) = result.warning.as_deref() {
        return Err(format!(
            "Could not validate webServerService '{service}': {warning}"
        ));
    }
    let Some(found) = result
        .services
        .iter()
        .find(|candidate| candidate.name.eq_ignore_ascii_case(service))
    else {
        return Err(format!(
            "Web server service '{service}' was not found in data/services.json."
        ));
    };
    if found.service_type.normalized() != ServiceType::Apache {
        return Err(format!(
            "Web server service '{service}' is not an Apache service (serviceType: {}).",
            found.service_type.normalized().label()
        ));
    }
    Ok(())
}

fn save_app_at(
    config_path: &Path,
    services_path: Option<&Path>,
    input: UpdaterAppInput,
) -> Result<Vec<UpdaterAppView>, String> {
    let mut config = load_config(config_path)?;
    let mut app = config.find(&input.id).cloned().unwrap_or_default();
    app.id = input.id;
    app.name = input.name;
    app.enabled = input.enabled;
    app.repo_url = input.repo_url;
    app.local_path = input.local_path;
    app.branch = input.branch;
    app.preset = input.preset;
    app.package_manager = input.package_manager;
    app.composer = input.composer;
    app.artisan = input.artisan;
    app.package_install = input.package_install;
    app.build = input.build;
    app.install_command = input.install_command;
    app.build_command = input.build_command;
    app.queue_restart = input.queue_restart;
    if let Some(value) = input.clone_on_first_run {
        app.clone_on_first_run = value;
    }
    app.allowed_client_ips = input.allowed_client_ips;
    app.dirty_ignore = input.dirty_ignore;
    app.web_server_service = input.web_server_service;
    app.services = input.services;
    if let Some(value) = input.health_check {
        app.health_check = value;
    }
    if let Some(value) = input.notify {
        app.notify = value;
    }
    app.normalize();
    config.validate_app(&app)?;
    if let Some(services_path) = services_path {
        validate_web_server_service(services_path, &app.web_server_service)?;
        let mut names = app.services.stop_before_pull.clone();
        names.extend(app.services.stop_before_build.clone());
        names.extend(app.services.restart_after.clone());
        names.extend(app.services.start_after.clone());
        validate_service_names(services_path, &names)?;
    }
    if let Some(existing) = config.find_mut(&app.id) {
        *existing = app;
    } else {
        config.apps.push(app);
    }
    save_config(config_path, &config)?;
    load_views_at(config_path)
}

fn delete_app_at(config_path: &Path, id: &str) -> Result<Vec<UpdaterAppView>, String> {
    let mut config = load_config(config_path)?;
    let before = config.apps.len();
    config.apps.retain(|app| !app.id.eq_ignore_ascii_case(id));
    if config.apps.len() == before {
        return Err(format!("App '{id}' was not found."));
    }
    save_config(config_path, &config)?;
    load_views_at(config_path)
}

#[tauri::command]
pub fn list_updater_apps() -> Result<Vec<UpdaterAppView>, String> {
    load_views_at(&config_path())
}

#[tauri::command]
pub fn save_updater_app(app: UpdaterAppInput) -> Result<Vec<UpdaterAppView>, String> {
    let paths = ConfigPaths::detect();
    save_app_at(&paths.projects_path(), Some(&paths.services_path()), app)
}

#[tauri::command]
pub fn delete_updater_app(id: String) -> Result<Vec<UpdaterAppView>, String> {
    delete_app_at(&config_path(), &id)
}

#[tauri::command]
pub fn test_updater_app(id: String) -> Result<String, String> {
    let path = config_path();
    let config = load_config(&path)?;
    test_updater_app_at(&path, &config, &id, &SystemRunner)
}

fn test_updater_app_at(
    config_path: &Path,
    config: &UpdaterConfig,
    id: &str,
    runner: &dyn Runner,
) -> Result<String, String> {
    let app = config
        .find(id)
        .ok_or_else(|| format!("App '{id}' was not found."))?;
    config.validate_app(app)?;

    let project = Path::new(app.local_path.trim());
    if !project.is_dir() {
        return Err(format!("Project folder not found: {}", project.display()));
    }
    // Test must follow the same effective settings as plan/run/check.
    let git_path = crate::credentials::effective_git_path_at(config_path)?;

    let branch = runner
        .run(
            &git_path,
            &[
                "-C".to_string(),
                app.local_path.clone(),
                "rev-parse".to_string(),
                "--abbrev-ref".to_string(),
                "HEAD".to_string(),
            ],
            project,
            std::time::Duration::from_secs(30),
        )
        .map_err(|error| error.to_string())?;
    if !branch.success() {
        return Err(format!("Failed to read branch: {}", branch.combined()));
    }

    let remote = runner
        .run(
            &git_path,
            &[
                "-C".to_string(),
                app.local_path.clone(),
                "ls-remote".to_string(),
                "--heads".to_string(),
                "origin".to_string(),
            ],
            project,
            std::time::Duration::from_secs(30),
        )
        .map_err(|error| error.to_string())?;
    if !remote.success() {
        return Err(format!("Remote check failed: {}", remote.combined()));
    }

    let preset = if app.preset == Preset::Auto {
        detect_preset(project)
    } else {
        app.preset
    };
    let package_manager = detect_package_manager(project)
        .map(|value| value.map(package_manager_name).unwrap_or("none"))
        .unwrap_or("ambiguous");

    let mut lines = vec![
        format!("App          : {}", app.id),
        format!("Path         : {}", project.display()),
        format!("Active branch: {}", branch.stdout.trim()),
        "Remote       : OK".to_string(),
        format!("Preset       : {}", preset_name(preset)),
        format!("Package mgr  : {package_manager}"),
    ];
    for marker in [
        "composer.json",
        "composer.lock",
        "artisan",
        "package.json",
        "package-lock.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "bun.lock",
        "bun.lockb",
        "go.mod",
    ] {
        if project.join(marker).is_file() {
            lines.push(format!("Detected     : {marker}"));
        }
    }
    Ok(lines.join("\n"))
}

fn plan_updater_app_at(
    config_path: &Path,
    app_id: &str,
    runner: &dyn Runner,
    services: &dyn ServiceControl,
) -> Result<AppPlan, String> {
    let config = load_config(config_path)?;
    let app = config
        .find(app_id)
        .ok_or_else(|| format!("App '{app_id}' was not found."))?;
    config.validate_app(app)?;
    let settings = effective_settings(
        &settings_path_for(config_path),
        &config.settings,
        &config.trigger,
    )
    .map_err(|error| error.to_string())?;
    Ok(plan_app(
        app,
        &settings.to_updater_settings(),
        Path::new(&app.local_path),
        runner,
        services,
    ))
}

#[tauri::command]
pub async fn plan_updater_app(app_id: String) -> Result<AppPlan, String> {
    let path = config_path();
    tauri::async_runtime::spawn_blocking(move || {
        plan_updater_app_at(&path, &app_id, &SystemRunner, &WindowsServiceControl)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn load_updater_history_at(
    config_path: &Path,
    app: Option<&str>,
    limit: Option<u32>,
) -> Vec<HistoryEntry> {
    load_history(config_path, app, limit.unwrap_or(50) as usize)
}

#[tauri::command]
pub async fn load_updater_history(
    app: Option<String>,
    limit: Option<u32>,
) -> Result<Vec<HistoryEntry>, String> {
    let path = config_path();
    tauri::async_runtime::spawn_blocking(move || {
        load_updater_history_at(&path, app.as_deref(), limit)
    })
    .await
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn run_updater(
    app: Option<String>,
    check: bool,
) -> Result<Vec<UpdaterRunOutcomeView>, String> {
    let config_path = config_path();
    let config = load_config(&config_path)?;
    let outcomes = tauri::async_runtime::spawn_blocking(move || {
        run_registry_with(
            &config,
            &config_path,
            &SystemRunner,
            &WindowsServiceControl,
            RunOptions {
                check_only: check,
                app,
                client_ip: None,
            },
        )
    })
    .await
    .map_err(|error| error.to_string())??;
    Ok(outcomes
        .into_iter()
        .map(|outcome| UpdaterRunOutcomeView {
            app_id: outcome.app_id,
            status: outcome.status,
            message: outcome.message,
            forbidden: outcome.forbidden,
        })
        .collect())
}

fn recent_logs(dir: &Path, limit: usize) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("project-update-") && name.ends_with(".log"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    files.reverse();
    files.truncate(limit);
    files
}

fn tail_text(path: &Path, lines: usize) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let all: Vec<&str> = text.lines().collect();
    let start = all.len().saturating_sub(lines);
    Ok(all[start..].join("\n"))
}

#[tauri::command]
pub fn read_updater_log(app: Option<String>) -> Result<UpdaterLogView, String> {
    let config_path = config_path();
    let config = load_config(&config_path)?;
    let log_dir = if config.settings.log_dir.trim().is_empty() {
        default_log_dir(&config_path)
    } else {
        PathBuf::from(config.settings.log_dir.trim())
    };

    let project_log = app.as_deref().and_then(|id| {
        config.find(id).and_then(|app| {
            recent_logs(&Path::new(&app.local_path).join("storage/logs"), 1)
                .into_iter()
                .next()
        })
    });
    let matching_global = app.as_deref().and_then(|id| {
        let marker = format!("===== {id}:");
        recent_logs(&log_dir, 50).into_iter().find(|path| {
            std::fs::read_to_string(path)
                .map(|text| text.contains(&marker))
                .unwrap_or(false)
        })
    });
    let path = if let Some(id) = app.as_deref() {
        project_log.or(matching_global).ok_or_else(|| {
            format!(
                "No log found for app '{id}' in {} or storage/logs.",
                log_dir.display()
            )
        })?
    } else {
        recent_logs(&log_dir, 1)
            .into_iter()
            .next()
            .ok_or_else(|| format!("No updater log found in {}", log_dir.display()))?
    };
    let text = tail_text(&path, 400)?;
    Ok(UpdaterLogView {
        path: path.to_string_lossy().to_string(),
        text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sm_updater::history::{append_history, HistoryEntry};
    use sm_updater::plan::PlanStatus;
    use sm_updater::registry::{UpdaterConfig, UpdaterSettings};
    use sm_updater::runner::{Output, RunnerError};
    use std::sync::Mutex;
    use std::time::Duration;

    fn app(id: &str, path: &Path) -> UpdaterAppInput {
        UpdaterAppInput {
            id: id.into(),
            name: id.into(),
            enabled: true,
            repo_url: "https://example.com/demo.git".into(),
            local_path: path.to_string_lossy().to_string(),
            branch: "main".into(),
            preset: Preset::Node,
            package_manager: PackageManager::Npm,
            composer: ToggleMode::Auto,
            artisan: ToggleMode::Auto,
            package_install: ToggleMode::Auto,
            build: ToggleMode::Auto,
            install_command: String::new(),
            build_command: String::new(),
            queue_restart: true,
            clone_on_first_run: None,
            allowed_client_ips: Vec::new(),
            dirty_ignore: Vec::new(),
            web_server_service: String::new(),
            services: ServicePlan::default(),
            health_check: None,
            notify: None,
        }
    }

    #[test]
    fn saves_lists_and_deletes_app() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("data/projects.json");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();

        let views = save_app_at(&config, None, app("demo", &project)).unwrap();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].id, "demo");
        assert_eq!(views[0].preset, "node");
        assert_eq!(views[0].package_manager, "npm");
        assert_eq!(views[0].status, "idle");

        let views = load_views_at(&config).unwrap();
        assert_eq!(views.len(), 1);

        let views = delete_app_at(&config, "demo").unwrap();
        assert!(views.is_empty());
    }

    #[test]
    fn rejects_service_missing_from_services_config() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("data/projects.json");
        let services = temp.path().join("data/services.json");
        std::fs::create_dir_all(services.parent().unwrap()).unwrap();
        std::fs::write(&services, r#"{"version":1,"services":[{"name":"api"}]}"#).unwrap();
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let mut app = app("demo", &project);
        app.services.restart_after = vec!["missing".into()];

        let error = save_app_at(&config, Some(&services), app).unwrap_err();
        assert!(error.contains("missing"));
    }

    fn write_services(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            r#"{"version":1,"services":[
                {"name":"Apache2.4","serviceType":"apache","executable":"C:\\apache\\bin\\httpd.exe"},
                {"name":"api","serviceType":"nssm","executable":"C:\\api\\api.exe"}
            ]}"#,
        )
        .unwrap();
    }

    #[test]
    fn accepts_apache_service_as_web_server() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("data/projects.json");
        let services = temp.path().join("data/services.json");
        write_services(&services);
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();

        let mut input = app("demo", &project);
        input.web_server_service = "Apache2.4".into();
        let views = save_app_at(&config, Some(&services), input).unwrap();

        assert_eq!(views[0].web_server_service, "Apache2.4");
    }

    #[test]
    fn rejects_web_server_service_that_is_not_apache() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("data/projects.json");
        let services = temp.path().join("data/services.json");
        write_services(&services);
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();

        let mut input = app("demo", &project);
        input.web_server_service = "api".into();
        let error = save_app_at(&config, Some(&services), input).unwrap_err();
        assert!(error.contains("is not an Apache service"), "{error}");

        let mut input = app("demo", &project);
        input.web_server_service = "missing".into();
        let error = save_app_at(&config, Some(&services), input).unwrap_err();
        assert!(
            error.contains("was not found in data/services.json"),
            "{error}"
        );
    }

    #[test]
    fn editing_preserves_web_server_service_and_runtime_versions() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("data/projects.json");
        let services = temp.path().join("data/services.json");
        let settings_path = temp.path().join("data/settings.json");
        write_services(&services);
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();

        let mut input = app("demo", &project);
        input.preset = Preset::Laravel;
        input.web_server_service = "Apache2.4".into();
        save_app_at(&config_path, Some(&services), input).unwrap();

        let mut settings = sm_updater::settings::Settings::default();
        settings.tools.php.version = "8.3.16".into();
        settings.tools.apache.tool.version = "Apache/2.4.62 (Win64)".into();
        sm_updater::settings::save_settings(&settings_path, &settings).unwrap();

        let views = load_views_at(&config_path).unwrap();
        assert_eq!(views[0].web_server_service, "Apache2.4");
        assert_eq!(views[0].php_version, "8.3.16");
        assert_eq!(views[0].apache_version, "Apache/2.4.62 (Win64)");

        // Editing an unrelated field with the form value preserved keeps webServerService.
        let mut edit = app("demo", &project);
        edit.preset = Preset::Laravel;
        edit.branch = "develop".into();
        edit.web_server_service = "Apache2.4".into();
        save_app_at(&config_path, Some(&services), edit).unwrap();
        let loaded = load_config(&config_path).unwrap();
        assert_eq!(loaded.find("demo").unwrap().web_server_service, "Apache2.4");

        // Empty means explicitly not linked to a web server.
        let mut clear = app("demo", &project);
        clear.preset = Preset::Laravel;
        clear.branch = "develop".into();
        clear.web_server_service = String::new();
        save_app_at(&config_path, Some(&services), clear).unwrap();
        let loaded = load_config(&config_path).unwrap();
        assert_eq!(loaded.find("demo").unwrap().web_server_service, "");
    }

    #[test]
    fn view_includes_state_fields() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("data/projects.json");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        save_app_at(&config_path, None, app("demo", &project)).unwrap();
        let mut state = load_state(&config_path);
        sm_updater::state::mark(&mut state, "demo", "updated", None, 25);
        sm_updater::state::save_state(&config_path, &state).unwrap();

        let views = load_views_at(&config_path).unwrap();
        assert_eq!(views[0].status, "updated");
        assert!(views[0].last_updated_at.contains('T'));
    }

    #[test]
    fn loads_updater_history_for_an_app_with_default_limit() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("data/projects.json");
        for index in 0..60 {
            append_history(
                &config_path,
                &HistoryEntry {
                    timestamp: format!("2026-11-17T10:{:02}:00+07:00", index % 60),
                    app_id: if index % 2 == 0 { "demo" } else { "other" }.into(),
                    status: "updated".into(),
                    message: "ok".into(),
                    commit: "abc1234".into(),
                    duration_ms: index,
                    client_ip: "10.255.240.143".into(),
                },
            )
            .unwrap();
        }

        let entries = load_updater_history_at(&config_path, Some("DEMO"), None);
        assert_eq!(entries.len(), 30);
        assert!(entries.iter().all(|entry| entry.app_id == "demo"));
        assert_eq!(load_updater_history_at(&config_path, None, None).len(), 50);

        let limited = load_updater_history_at(&config_path, Some("demo"), Some(5));
        assert_eq!(limited.len(), 5);
        assert_eq!(limited[0].duration_ms, 58);
        assert_eq!(limited[4].duration_ms, 50);
    }

    #[test]
    fn updater_app_view_uses_allowed_client_ips_acronym() {
        let app = AppConfig {
            id: "demo".into(),
            name: "Demo".into(),
            allowed_client_ips: vec!["10.0.0.1".into(), "10.0.0.0/24".into()],
            ..AppConfig::default()
        };
        let view = view(&app, None, &Settings::default());
        let json = serde_json::to_value(&view).unwrap();

        assert_eq!(
            json.get("allowedClientIPs"),
            Some(&serde_json::json!(["10.0.0.1", "10.0.0.0/24"]))
        );
        assert!(json.get("allowedClientIps").is_none());
    }

    #[test]
    fn dirty_ignore_roundtrips_and_serializes_camel_case() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("data/projects.json");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();

        let mut input = app("demo", &project);
        input.dirty_ignore = vec![" public/build/** ".into(), String::new()];
        let views = save_app_at(&config_path, None, input).unwrap();
        assert_eq!(views[0].dirty_ignore, ["public/build/**"]);

        let json = serde_json::to_value(&views[0]).unwrap();
        assert_eq!(
            json.get("dirtyIgnore"),
            Some(&serde_json::json!(["public/build/**"]))
        );

        let loaded = load_config(&config_path).unwrap();
        assert_eq!(
            loaded.find("demo").unwrap().dirty_ignore,
            ["public/build/**"]
        );

        let mut invalid = app("demo", &project);
        invalid.dirty_ignore = vec!["/absolute".into()];
        let error = save_app_at(&config_path, None, invalid).unwrap_err();
        assert!(error.contains("dirtyIgnore"), "{error}");
    }

    #[test]
    fn test_app_uses_effective_git_path_not_legacy_settings() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("package.json"), "{}").unwrap();
        let tools = temp.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        let good_git = tools.join("git.exe");
        for name in ["git.exe", "php.exe", "composer.bat", "npm.cmd"] {
            std::fs::write(tools.join(name), b"stub").unwrap();
        }
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let config_path = data.join("projects.json");

        let mut settings = Settings::default();
        settings.tools.git.path = good_git.to_string_lossy().to_string();
        settings.tools.php.path = tools.join("php.exe").to_string_lossy().to_string();
        settings.tools.composer.path = tools.join("composer.bat").to_string_lossy().to_string();
        settings.tools.npm.path = tools.join("npm.cmd").to_string_lossy().to_string();
        sm_updater::settings::save_settings(&settings_path_for(&config_path), &settings).unwrap();

        let config = UpdaterConfig {
            settings: UpdaterSettings {
                git_path: temp
                    .path()
                    .join("legacy-missing-git.exe")
                    .to_string_lossy()
                    .to_string(),
                ..UpdaterSettings::default()
            },
            apps: vec![AppConfig {
                id: "demo".into(),
                name: "Demo".into(),
                repo_url: "https://example.com/demo.git".into(),
                local_path: project.to_string_lossy().to_string(),
                branch: "main".into(),
                preset: Preset::Node,
                ..AppConfig::default()
            }],
            ..UpdaterConfig::default()
        };
        save_config(&config_path, &config).unwrap();

        struct RecordingRunner {
            programs: Mutex<Vec<PathBuf>>,
        }
        impl Runner for RecordingRunner {
            fn run(
                &self,
                program: &Path,
                args: &[String],
                _cwd: &Path,
                _timeout: Duration,
            ) -> Result<Output, RunnerError> {
                self.programs.lock().unwrap().push(program.to_path_buf());
                let stdout = if args.iter().any(|arg| arg == "rev-parse") {
                    "main\n"
                } else {
                    "refs/heads/main\n"
                };
                Ok(Output {
                    exit_code: 0,
                    stdout: stdout.to_string(),
                    stderr: String::new(),
                })
            }
        }
        let runner = RecordingRunner {
            programs: Mutex::new(Vec::new()),
        };

        let message = test_updater_app_at(&config_path, &config, "demo", &runner).unwrap();
        assert!(message.contains("Active branch: main"), "{message}");
        let programs = runner.programs.lock().unwrap();
        assert_eq!(programs.len(), 2);
        assert_eq!(programs[0], good_git);
        assert_eq!(programs[1], good_git);
    }

    #[test]
    fn auto_preset_laravel_shows_effective_php_version() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("laravel");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("artisan"), "php").unwrap();
        std::fs::write(project.join("composer.json"), "{}").unwrap();
        let mut app = AppConfig {
            id: "laravel".into(),
            name: "Laravel".into(),
            local_path: project.to_string_lossy().to_string(),
            preset: Preset::Auto,
            ..AppConfig::default()
        };
        app.web_server_service = "Apache2.4".into();
        let mut settings = sm_updater::settings::Settings::default();
        settings.tools.php.version = "8.3.16".into();
        settings.tools.apache.tool.version = "2.4.62".into();

        let first_view = view(&app, None, &settings);
        assert_eq!(first_view.preset, "laravel");
        assert_eq!(first_view.php_version, "8.3.16");
        assert_eq!(first_view.apache_version, "2.4.62");

        app.web_server_service.clear();
        let second_view = view(&app, None, &settings);
        assert!(second_view.apache_version.is_empty());
    }

    #[test]
    fn updater_settings_default_has_paths() {
        let settings = UpdaterSettings::default();
        assert!(!settings.git_path.trim().is_empty());
    }

    #[test]
    fn editing_preserves_advanced_fields_not_in_form() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("data/projects.json");
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();

        let mut original = AppConfig {
            id: "demo".into(),
            name: "Demo".into(),
            repo_url: "https://example.com/demo.git".into(),
            local_path: project.to_string_lossy().to_string(),
            branch: "main".into(),
            clone_on_first_run: true,
            health_check: sm_updater::registry::HealthCheck {
                enabled: true,
                url: "http://127.0.0.1/health".into(),
                timeout_seconds: 10,
            },
            notify: sm_updater::registry::NotifyConfig {
                webhook_url: "https://hooks.example.test/x".into(),
                on: vec!["failed".into()],
            },
            ..AppConfig::default()
        };
        original.normalize();
        let mut config = UpdaterConfig::default();
        config.apps.push(original);
        save_config(&config_path, &config).unwrap();

        let mut input = app("demo", &project);
        input.branch = "develop".into();
        save_app_at(&config_path, None, input).unwrap();

        let loaded = load_config(&config_path).unwrap();
        let saved = loaded.find("demo").unwrap();
        assert_eq!(saved.branch, "develop");
        assert!(saved.clone_on_first_run);
        assert!(saved.health_check.enabled);
        assert_eq!(saved.health_check.url, "http://127.0.0.1/health");
        assert_eq!(saved.notify.webhook_url, "https://hooks.example.test/x");
    }

    #[test]
    fn default_config_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("projects.json");
        save_config(&path, &UpdaterConfig::default()).unwrap();
        let loaded = load_config(&path).unwrap();
        assert_eq!(loaded.version, 2);
    }

    fn plan_ok(stdout: &str) -> Output {
        Output {
            exit_code: 0,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    struct PlanRunner;

    impl Runner for PlanRunner {
        fn run(
            &self,
            _program: &Path,
            args: &[String],
            _cwd: &Path,
            _timeout: Duration,
        ) -> Result<Output, RunnerError> {
            let first = args.first().map(String::as_str).unwrap_or("");
            let output = match first {
                "status" => plan_ok(""),
                "rev-parse" if args.get(1).map(String::as_str) == Some("--abbrev-ref") => {
                    plan_ok("main")
                }
                "rev-parse" if args.get(1).map(String::as_str) == Some("HEAD") => {
                    plan_ok("aaaa1111")
                }
                "rev-parse" if args.get(1).map(String::as_str) == Some("--verify") => {
                    plan_ok("bbbb2222")
                }
                "ls-remote" => plan_ok("refs/heads/main"),
                "fetch" => plan_ok(""),
                "rev-list" => plan_ok("1"),
                "log" => plan_ok("bbbb2222\tfeat: update"),
                "diff" => plan_ok("index.html"),
                _ => plan_ok(""),
            };
            Ok(output)
        }
    }

    struct PlanControl {
        known: std::collections::BTreeSet<String>,
        running: std::collections::BTreeSet<String>,
        calls: Mutex<Vec<String>>,
    }

    impl PlanControl {
        fn new(known: &[&str], running: &[&str]) -> Self {
            Self {
                known: known.iter().map(|value| value.to_string()).collect(),
                running: running.iter().map(|value| value.to_string()).collect(),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl ServiceControl for PlanControl {
        fn exists(&self, name: &str) -> Result<bool, String> {
            self.calls.lock().unwrap().push(format!("exists:{name}"));
            Ok(self.known.contains(name))
        }

        fn is_running(&self, name: &str) -> Result<bool, String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("is_running:{name}"));
            Ok(self.running.contains(name))
        }

        fn stop(&self, name: &str, _timeout: Duration) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("stop:{name}"));
            Ok(())
        }

        fn start(&self, name: &str, _timeout: Duration) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("start:{name}"));
            Ok(())
        }

        fn restart(&self, name: &str, _timeout: Duration) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("restart:{name}"));
            Ok(())
        }
    }

    #[test]
    fn plan_updater_app_builds_ready_plan_without_service_changes() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("data/projects.json");
        let project = temp.path().join("project");
        std::fs::create_dir_all(project.join(".git")).unwrap();
        std::fs::write(project.join("index.html"), "hello").unwrap();
        let tools = temp.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        std::fs::write(tools.join("git.exe"), b"dummy").unwrap();

        let mut settings = UpdaterSettings {
            git_path: tools.join("git.exe").to_string_lossy().to_string(),
            ..UpdaterSettings::default()
        };
        settings.refresh_variables();
        let mut config = UpdaterConfig {
            settings,
            ..UpdaterConfig::default()
        };
        config.apps.push(AppConfig {
            id: "demo".into(),
            name: "Demo".into(),
            repo_url: "https://example.test/demo.git".into(),
            local_path: project.to_string_lossy().to_string(),
            branch: "main".into(),
            preset: Preset::Static,
            composer: ToggleMode::Never,
            artisan: ToggleMode::Never,
            package_install: ToggleMode::Never,
            build: ToggleMode::Never,
            services: ServicePlan {
                restart_after: vec!["api".into()],
                ..ServicePlan::default()
            },
            ..AppConfig::default()
        });
        save_config(&config_path, &config).unwrap();

        let control = PlanControl::new(&["api"], &["api"]);
        let plan = plan_updater_app_at(&config_path, "demo", &PlanRunner, &control).unwrap();

        assert_eq!(plan.status, PlanStatus::Ready);
        assert_eq!(plan.incoming.len(), 1);
        assert_eq!(plan.incoming[0].subject, "feat: update");
        assert_eq!(plan.services.validation, "ok");
        assert_eq!(plan.services.states[0].running, Some(true));
        for call in control.calls() {
            assert!(
                call.starts_with("exists:") || call.starts_with("is_running:"),
                "service mutation attempted: {call}"
            );
        }
    }
}
