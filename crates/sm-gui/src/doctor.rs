//! Read-only readiness report ("Doctor").
//!
//! `run_doctor_at` never writes settings, services, projects, or the pending
//! Apache change. The only write is a temporary probe file inside the existing
//! `data/`, `logs/`, and `backup/` folders; the file is removed immediately and
//! the probe does nothing when the folder is missing.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{Local, SecondsFormat};
use serde::Serialize;
use sm_config::ConfigPaths;
use sm_core::{redact_secrets, ServiceConfig, ServiceType};
use sm_updater::registry::{load_config, AppConfig, CommandSpec, UpdaterConfig};
use sm_updater::runner::{Runner, SystemRunner};
use sm_updater::settings::{from_legacy, load_settings, settings_path_for, Settings};
use sm_win::ScmError;

use crate::apache;
use crate::settings::{self, TaskStatusView, ToolSpec, TOOL_SPECS};

const TOOL_PROBE_TIMEOUT: Duration = Duration::from_secs(8);
const REMOTE_PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const SSH_PORT_TIMEOUT: Duration = Duration::from_millis(500);
const LOW_DISK_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_ERROR_CHARS: usize = 500;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum DoctorStatus {
    Ok,
    Warning,
    Error,
    Skipped,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheck {
    pub id: String,
    pub category: String,
    pub label: String,
    pub status: DoctorStatus,
    pub detail: String,
    pub hint: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCounts {
    pub ok: usize,
    pub warning: usize,
    pub error: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub generated_at: String,
    pub overall: DoctorStatus,
    pub checks: Vec<DoctorCheck>,
    pub counts: DoctorCounts,
    pub base_path: String,
}

/// Any error wins; otherwise any warning; skipped never escalates.
pub(crate) fn overall_status(checks: &[DoctorCheck]) -> DoctorStatus {
    if checks
        .iter()
        .any(|check| check.status == DoctorStatus::Error)
    {
        return DoctorStatus::Error;
    }
    if checks
        .iter()
        .any(|check| check.status == DoctorStatus::Warning)
    {
        return DoctorStatus::Warning;
    }
    DoctorStatus::Ok
}

pub(crate) fn doctor_counts(checks: &[DoctorCheck]) -> DoctorCounts {
    let mut counts = DoctorCounts::default();
    for check in checks {
        match check.status {
            DoctorStatus::Ok => counts.ok += 1,
            DoctorStatus::Warning => counts.warning += 1,
            DoctorStatus::Error => counts.error += 1,
            DoctorStatus::Skipped => counts.skipped += 1,
        }
    }
    counts
}

fn check(
    id: impl Into<String>,
    category: &str,
    label: impl Into<String>,
    status: DoctorStatus,
    detail: impl Into<String>,
    hint: impl Into<String>,
) -> DoctorCheck {
    DoctorCheck {
        id: id.into(),
        category: category.to_string(),
        label: label.into(),
        status,
        detail: detail.into(),
        hint: hint.into(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SettingsFileState {
    Valid,
    Missing,
    Invalid(String),
}

pub(crate) fn settings_file_check(state: &SettingsFileState) -> DoctorCheck {
    match state {
        SettingsFileState::Valid => check(
            "settings.file",
            "settings",
            "settings.json",
            DoctorStatus::Ok,
            "data/settings.json exists and is valid.",
            "",
        ),
        SettingsFileState::Missing => check(
            "settings.file",
            "settings",
            "settings.json",
            DoctorStatus::Warning,
            "data/settings.json was not found; effective values come from the legacy projects.json settings.",
            "Review the values and click Save Settings to create data/settings.json.",
        ),
        SettingsFileState::Invalid(error) => check(
            "settings.file",
            "settings",
            "settings.json",
            DoctorStatus::Error,
            format!("data/settings.json is invalid: {error}"),
            "Fix or migrate data/settings.json in Settings before running updates.",
        ),
    }
}

fn apache_settings_check(settings: &Settings) -> DoctorCheck {
    let path = settings.tools.apache.path().trim();
    if path.is_empty() {
        return check(
            "settings.apache",
            "settings",
            "Apache root/config",
            DoctorStatus::Skipped,
            "Apache is not configured.",
            "",
        );
    }
    let root = settings.tools.apache.root.trim();
    if root.is_empty() {
        return check(
            "settings.apache",
            "settings",
            "Apache root/config",
            DoctorStatus::Warning,
            format!("Apache path is set ({path}) but the Apache root is empty."),
            "Set the Apache root folder in Settings > Toolchain so services can be propagated.",
        );
    }
    let root_path = Path::new(root);
    if !root_path.is_dir() {
        return check(
            "settings.apache",
            "settings",
            "Apache root/config",
            DoctorStatus::Warning,
            format!("Apache root does not exist or is not a directory: {root}"),
            "Set the correct Apache root in Settings > Toolchain.",
        );
    }
    let config = settings.tools.apache.config.trim();
    if config.is_empty() {
        return check(
            "settings.apache",
            "settings",
            "Apache root/config",
            DoctorStatus::Warning,
            format!("Apache root is set ({root}) but the Apache config path is empty."),
            "Set the Apache config path, for example conf/httpd.conf.",
        );
    }
    let config_path = if Path::new(config).is_absolute() {
        PathBuf::from(config)
    } else {
        root_path.join(config.replace('/', "\\"))
    };
    if !config_path.is_file() {
        return check(
            "settings.apache",
            "settings",
            "Apache root/config",
            DoctorStatus::Warning,
            format!(
                "Apache config file was not found: {}",
                config_path.display()
            ),
            "Check the Apache config path in Settings > Toolchain.",
        );
    }
    check(
        "settings.apache",
        "settings",
        "Apache root/config",
        DoctorStatus::Ok,
        format!("Apache root and config are present: {} / {}.", root, config),
        "",
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ToolProbe {
    NotConfigured,
    MissingPath,
    Tested {
        success: bool,
        version: String,
        error: String,
    },
}

pub(crate) fn tool_check(spec: &ToolSpec, path: &str, probe: ToolProbe) -> DoctorCheck {
    let id = format!("tools.{}", spec.name);
    let label = spec.label;
    let hint = if spec.required {
        "Set the path in Settings > Toolchain, then run Test."
    } else {
        "Configure this optional tool only when an app needs it."
    };
    match probe {
        ToolProbe::NotConfigured if spec.required => check(
            id,
            "tools",
            label,
            DoctorStatus::Error,
            format!("{label} is not configured. It is required for updates."),
            hint,
        ),
        ToolProbe::NotConfigured => check(
            id,
            "tools",
            label,
            DoctorStatus::Warning,
            format!("{label} is not configured. Some apps may need it."),
            hint,
        ),
        ToolProbe::MissingPath if spec.required => check(
            id,
            "tools",
            label,
            DoctorStatus::Error,
            format!("{label} path does not exist: {path}"),
            hint,
        ),
        ToolProbe::MissingPath => check(
            id,
            "tools",
            label,
            DoctorStatus::Warning,
            format!("{label} path does not exist: {path}"),
            hint,
        ),
        ToolProbe::Tested {
            success: true,
            version,
            ..
        } => check(
            id,
            "tools",
            label,
            DoctorStatus::Ok,
            if version.trim().is_empty() {
                format!("{label} version check succeeded: {path}")
            } else {
                format!("{label}: {version}")
            },
            "",
        ),
        ToolProbe::Tested {
            success: false,
            error,
            ..
        } if spec.required => check(
            id,
            "tools",
            label,
            DoctorStatus::Error,
            format!("{label} version check failed at {path}: {error}"),
            hint,
        ),
        ToolProbe::Tested {
            success: false,
            error,
            ..
        } => check(
            id,
            "tools",
            label,
            DoctorStatus::Warning,
            format!("{label} version check failed at {path}: {error}"),
            hint,
        ),
    }
}

pub(crate) fn registry_checks(
    projects_exists: bool,
    config: &Result<UpdaterConfig, String>,
    services: Option<&[ServiceConfig]>,
) -> Vec<DoctorCheck> {
    let mut checks = Vec::new();
    let config = match config {
        Ok(config) => config,
        Err(error) => {
            checks.push(check(
                "registry.config",
                "registry",
                "projects.json",
                DoctorStatus::Error,
                format!("data/projects.json is invalid: {error}"),
                "Fix the registry file before running updates.",
            ));
            return checks;
        }
    };
    if !projects_exists {
        checks.push(check(
            "registry.config",
            "registry",
            "projects.json",
            DoctorStatus::Warning,
            "data/projects.json was not found; no updater apps are registered.",
            "Add updater apps in the Updater tab.",
        ));
        return checks;
    }

    let enabled = config.apps.iter().filter(|app| app.enabled).count();
    checks.push(check(
        "registry.config",
        "registry",
        "projects.json",
        DoctorStatus::Ok,
        format!(
            "data/projects.json is valid: {} app(s), {enabled} enabled.",
            config.apps.len()
        ),
        "",
    ));

    let mut seen = BTreeSet::new();
    let duplicates: Vec<String> = config
        .apps
        .iter()
        .map(|app| app.id.to_ascii_lowercase())
        .filter(|id| !id.is_empty() && !seen.insert(id.clone()))
        .collect();
    if !duplicates.is_empty() {
        checks.push(check(
            "registry.duplicates",
            "registry",
            "Duplicate app ids",
            DoctorStatus::Warning,
            format!("Duplicate app id(s): {}.", duplicates.join(", ")),
            "App ids must be unique; remove duplicates from projects.json.",
        ));
    }

    for app in &config.apps {
        let name = app_name(app);
        let id = app.id.trim();
        if id.is_empty() {
            continue;
        }
        if app.enabled {
            let local_path = app.local_path.trim();
            if local_path.is_empty() {
                checks.push(check(
                    format!("registry.app.{id}.path"),
                    "registry",
                    format!("App path: {name}"),
                    DoctorStatus::Warning,
                    "localPath is empty.",
                    "Set the app local path in the Updater tab.",
                ));
            } else if !Path::new(local_path).is_dir() {
                checks.push(check(
                    format!("registry.app.{id}.path"),
                    "registry",
                    format!("App path: {name}"),
                    DoctorStatus::Warning,
                    format!("localPath does not exist or is not a directory: {local_path}"),
                    "Check the app local path in the Updater tab.",
                ));
            }
        }

        let web_service = app.web_server_service.trim();
        if web_service.is_empty() {
            continue;
        }
        let Some(services) = services else {
            continue;
        };
        match services
            .iter()
            .find(|service| service.name.eq_ignore_ascii_case(web_service))
        {
            None => checks.push(check(
                format!("registry.app.{id}.webServerService"),
                "registry",
                format!("Web server: {name}"),
                DoctorStatus::Warning,
                format!("webServerService '{web_service}' does not exist in data/services.json."),
                "Pick an Apache service from data/services.json or clear webServerService.",
            )),
            Some(service)
                if service.service_type.normalized() != ServiceType::Apache =>
            {
                checks.push(check(
                    format!("registry.app.{id}.webServerService"),
                    "registry",
                    format!("Web server: {name}"),
                    DoctorStatus::Warning,
                    format!(
                        "webServerService '{web_service}' is not an Apache service (serviceType: {}).",
                        service.service_type.normalized().label()
                    ),
                    "Set webServerService to a service with serviceType=apache.",
                ))
            }
            Some(service) => checks.push(check(
                format!("registry.app.{id}.webServerService"),
                "registry",
                format!("Web server: {name}"),
                DoctorStatus::Ok,
                format!(
                    "Served by Apache service '{}'.",
                    service.name
                ),
                "",
            )),
        }
    }

    checks
}

fn app_name(app: &AppConfig) -> String {
    if app.name.trim().is_empty() {
        app.id.trim().to_string()
    } else {
        app.name.trim().to_string()
    }
}

pub(crate) fn services_config_check(
    services_exists: bool,
    warning: Option<&str>,
    services: &[ServiceConfig],
) -> DoctorCheck {
    if let Some(warning) = warning {
        return check(
            "services.config",
            "services",
            "services.json",
            DoctorStatus::Error,
            format!("data/services.json is invalid: {warning}"),
            "Fix data/services.json before installing or updating services.",
        );
    }
    if !services_exists {
        return check(
            "services.config",
            "services",
            "services.json",
            DoctorStatus::Warning,
            "data/services.json was not found; no Windows services are configured.",
            "Add services in the Services tab if the update needs service control.",
        );
    }
    let apache = services
        .iter()
        .filter(|service| service.service_type.normalized() == ServiceType::Apache)
        .count();
    check(
        "services.config",
        "services",
        "services.json",
        DoctorStatus::Ok,
        format!(
            "data/services.json is valid: {} service(s), {apache} Apache.",
            services.len()
        ),
        "",
    )
}

pub(crate) fn apache_service_presence_check(
    apache_configured: bool,
    services_valid: bool,
    apache_services: usize,
) -> DoctorCheck {
    if !services_valid {
        return check(
            "services.apache",
            "services",
            "Apache service",
            DoctorStatus::Skipped,
            "Apache service presence cannot be checked because data/services.json is invalid.",
            "",
        );
    }
    if !apache_configured {
        return check(
            "services.apache",
            "services",
            "Apache service",
            DoctorStatus::Skipped,
            "Apache is not configured in Settings.",
            "",
        );
    }
    if apache_services == 0 {
        return check(
            "services.apache",
            "services",
            "Apache service",
            DoctorStatus::Warning,
            "Apache path is configured but no service with serviceType=apache exists in data/services.json.",
            "Add or import the Apache service, or clear the Apache path if this server does not use Apache.",
        );
    }
    check(
        "services.apache",
        "services",
        "Apache service",
        DoctorStatus::Ok,
        format!("{apache_services} Apache service(s) configured in data/services.json."),
        "",
    )
}

pub(crate) fn service_state_check(
    service_name: &str,
    result: Result<Option<String>, String>,
) -> DoctorCheck {
    match result {
        Ok(Some(state)) => check(
            format!("services.{service_name}"),
            "services",
            format!("Service: {service_name}"),
            DoctorStatus::Ok,
            format!("Service '{service_name}' is installed (state: {state})."),
            "",
        ),
        Ok(None) => check(
            format!("services.{service_name}"),
            "services",
            format!("Service: {service_name}"),
            DoctorStatus::Warning,
            format!("Service '{service_name}' is not installed."),
            "Install the service in the Services tab or fix webServerService.",
        ),
        Err(error) => check(
            format!("services.{service_name}"),
            "services",
            format!("Service: {service_name}"),
            DoctorStatus::Warning,
            format!("Could not query service '{service_name}': {error}"),
            "Check the service name and whether the Service Control Manager is available.",
        ),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RemoteProbe {
    pub heads: usize,
}

pub(crate) fn remote_check(
    app: &AppConfig,
    git_ready: bool,
    result: Option<Result<RemoteProbe, String>>,
) -> DoctorCheck {
    let url = app.repo_url.trim();
    if url.is_empty() {
        return check(
            format!("remote.{}", app.id),
            "remote",
            format!("Remote: {}", app_name(app)),
            DoctorStatus::Skipped,
            "repoUrl is empty.",
            "",
        );
    }
    if !git_ready {
        return check(
            format!("remote.{}", app.id),
            "remote",
            format!("Remote: {}", app_name(app)),
            DoctorStatus::Skipped,
            "Git is not configured; remote access was not checked.",
            "Configure Git in Settings > Toolchain.",
        );
    }
    let display_url = redact_display_url(url);
    match result {
        Some(Ok(probe)) => check(
            format!("remote.{}", app.id),
            "remote",
            format!("Remote: {}", app_name(app)),
            DoctorStatus::Ok,
            format!(
                "Remote is reachable ({display_url}): {} branch head(s).",
                probe.heads
            ),
            "",
        ),
        Some(Err(error)) => check(
            format!("remote.{}", app.id),
            "remote",
            format!("Remote: {}", app_name(app)),
            DoctorStatus::Warning,
            format!("Remote check failed ({display_url}): {error}"),
            "Network or credentials may be transient; run Test in the Updater tab after fixing access.",
        ),
        None => check(
            format!("remote.{}", app.id),
            "remote",
            format!("Remote: {}", app_name(app)),
            DoctorStatus::Skipped,
            "Remote access was not checked.",
            "",
        ),
    }
}

pub(crate) fn task_check(task_name: &str, status: Result<TaskStatusView, String>) -> DoctorCheck {
    let label = format!("Scheduled Task: {task_name}");
    match status {
        Err(error) => check(
            "task.status",
            "task",
            label,
            DoctorStatus::Warning,
            format!("Could not read Scheduled Task '{task_name}': {error}"),
            "Check the task name and PowerShell availability, or install the task.",
        ),
        Ok(status) if !status.installed => check(
            "task.status",
            "task",
            label,
            DoctorStatus::Warning,
            format!("Scheduled Task '{task_name}' is not installed."),
            "Install the updater task from Settings > Privileged Execution.",
        ),
        Ok(status) if !status.state.trim().eq_ignore_ascii_case("ready") => check(
            "task.status",
            "task",
            label,
            DoctorStatus::Warning,
            format!(
                "Scheduled Task '{task_name}' is installed but its state is '{}' (expected Ready).",
                status.state
            ),
            "Inspect the task in Windows Task Scheduler or repair it from Settings.",
        ),
        Ok(status) if status.last_result != 0 => check(
            "task.status",
            "task",
            label,
            DoctorStatus::Warning,
            format!(
                "Scheduled Task '{task_name}' last result is {} (expected 0).",
                status.last_result
            ),
            "Open the updater task log and fix the last failure.",
        ),
        Ok(status) => check(
            "task.status",
            "task",
            label,
            DoctorStatus::Ok,
            format!(
                "Scheduled Task '{task_name}' is Ready (last result 0{}).",
                if status.last_run.trim().is_empty() {
                    String::new()
                } else {
                    format!(", last run {}", status.last_run.trim())
                }
            ),
            "",
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SshServiceProbe {
    Running,
    Stopped(String),
    NotFound,
    Error(String),
}

pub(crate) fn ssh_service_check(probe: SshServiceProbe) -> DoctorCheck {
    match probe {
        SshServiceProbe::Running => check(
            "ssh.service",
            "ssh",
            "SSH service (sshd)",
            DoctorStatus::Ok,
            "Service 'sshd' is installed and running.",
            "",
        ),
        SshServiceProbe::Stopped(state) => check(
            "ssh.service",
            "ssh",
            "SSH service (sshd)",
            DoctorStatus::Warning,
            format!("Service 'sshd' is installed but its state is '{state}'."),
            "Start OpenSSH Server if this server is triggered remotely over SSH.",
        ),
        SshServiceProbe::NotFound => check(
            "ssh.service",
            "ssh",
            "SSH service (sshd)",
            DoctorStatus::Warning,
            "Service 'sshd' was not found.",
            "Install OpenSSH Server if this server is triggered remotely over SSH.",
        ),
        SshServiceProbe::Error(error) => check(
            "ssh.service",
            "ssh",
            "SSH service (sshd)",
            DoctorStatus::Skipped,
            format!("Could not query service 'sshd': {error}"),
            "",
        ),
    }
}

pub(crate) fn ssh_port_check(result: Result<bool, String>) -> DoctorCheck {
    match result {
        Ok(true) => check(
            "ssh.port",
            "ssh",
            "SSH port 22",
            DoctorStatus::Ok,
            "Port 22 accepts local connections.",
            "",
        ),
        Ok(false) => check(
            "ssh.port",
            "ssh",
            "SSH port 22",
            DoctorStatus::Warning,
            "Port 22 does not accept local connections.",
            "Check the OpenSSH service and sshd_config if remote triggers use SSH. No firewall changes are made by Doctor.",
        ),
        Err(error) => check(
            "ssh.port",
            "ssh",
            "SSH port 22",
            DoctorStatus::Skipped,
            format!("Could not check port 22: {error}"),
            "",
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WriteProbe {
    Missing,
    Writable,
    NotWritable(String),
}

/// Create and immediately remove a temporary file in an existing directory.
/// A missing directory is only reported and never created.
pub(crate) fn probe_writable(dir: &Path) -> WriteProbe {
    if !dir.exists() {
        return WriteProbe::Missing;
    }
    if !dir.is_dir() {
        return WriteProbe::NotWritable("path is not a directory".to_string());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_nanos())
        .unwrap_or(0);
    let probe_path = dir.join(format!(".doctor-probe-{}-{stamp}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<()> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&probe_path)?;
        file.write_all(b"probe")?;
        file.flush()?;
        drop(file);
        fs::remove_file(&probe_path)
    })();
    match result {
        Ok(()) => WriteProbe::Writable,
        Err(error) => {
            let _ = fs::remove_file(&probe_path);
            WriteProbe::NotWritable(error.to_string())
        }
    }
}

pub(crate) fn storage_check(id: &str, label: &str, path: &Path, probe: WriteProbe) -> DoctorCheck {
    match probe {
        WriteProbe::Writable => check(
            id,
            "storage",
            label,
            DoctorStatus::Ok,
            format!("{} exists and is writable.", path.display()),
            "",
        ),
        WriteProbe::Missing => check(
            id,
            "storage",
            label,
            DoctorStatus::Warning,
            format!("{} does not exist.", path.display()),
            "Create the folder or let Service Manager create it before updating.",
        ),
        WriteProbe::NotWritable(error) => check(
            id,
            "storage",
            label,
            DoctorStatus::Error,
            format!("{} is not writable: {error}", path.display()),
            "Fix folder permissions or free up the path before updating.",
        ),
    }
}

pub(crate) fn disk_space_check(result: Result<u64, String>) -> DoctorCheck {
    match result {
        Ok(free) if free < LOW_DISK_BYTES => check(
            "storage.disk",
            "storage",
            "Free disk space",
            DoctorStatus::Warning,
            format!("Only {} MiB free on the data volume.", free / (1024 * 1024)),
            "Free disk space before running updates, builds, or backups.",
        ),
        Ok(free) => check(
            "storage.disk",
            "storage",
            "Free disk space",
            DoctorStatus::Ok,
            format!(
                "{} GiB free on the data volume.",
                free / (1024 * 1024 * 1024)
            ),
            "",
        ),
        Err(error) => check(
            "storage.disk",
            "storage",
            "Free disk space",
            DoctorStatus::Skipped,
            format!("Could not read free disk space: {error}"),
            "",
        ),
    }
}

pub(crate) fn pending_check(result: Result<Option<String>, String>) -> DoctorCheck {
    match result {
        Ok(Some(summary)) => check(
            "pending.apache",
            "pending",
            "Apache change pending",
            DoctorStatus::Warning,
            format!("A pending Apache change exists: {summary}"),
            "Review and apply it, or remove the pending change, before updating.",
        ),
        Ok(None) => check(
            "pending.apache",
            "pending",
            "Apache change pending",
            DoctorStatus::Ok,
            "No pending Apache change.",
            "",
        ),
        Err(error) => check(
            "pending.apache",
            "pending",
            "Apache change pending",
            DoctorStatus::Warning,
            format!("Could not read the pending Apache change: {error}"),
            "Fix or remove data/apache-pending.json.",
        ),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PhpReference {
    pub app_id: String,
    pub app_name: String,
    pub field: String,
    pub value: String,
}

fn normalize_match(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .replace('/', "\\")
        .to_ascii_lowercase()
}

fn is_token_delimiter(character: char) -> bool {
    character.is_whitespace()
        || matches!(
            character,
            '"' | '\'' | ';' | '&' | '|' | '(' | ')' | ',' | '='
        )
}

fn php_tokens(value: &str) -> Vec<String> {
    let lower = value.to_ascii_lowercase();
    let mut tokens = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative) = lower[cursor..].find("php.exe") {
        let end = cursor + relative + "php.exe".len();
        let start = value[..cursor + relative]
            .char_indices()
            .rev()
            .find(|(_, character)| is_token_delimiter(*character))
            .map(|(index, character)| index + character.len_utf8())
            .unwrap_or(0);
        let token = value[start..end].trim();
        if !token.is_empty() {
            tokens.push(token.to_string());
        }
        cursor = end;
    }
    tokens
}

fn push_php_reference(
    references: &mut Vec<PhpReference>,
    app: &AppConfig,
    field: impl Into<String>,
    value: &str,
    effective_php: &str,
) {
    if value.trim().is_empty() {
        return;
    }
    for token in php_tokens(value) {
        if !token.contains('\\') && !token.contains('/') {
            continue;
        }
        if normalize_match(&token) == normalize_match(effective_php) {
            continue;
        }
        references.push(PhpReference {
            app_id: app.id.clone(),
            app_name: app_name(app),
            field: field.into(),
            value: value.to_string(),
        });
        return;
    }
}

fn push_php_spec(
    references: &mut Vec<PhpReference>,
    app: &AppConfig,
    prefix: &str,
    spec: &CommandSpec,
    effective_php: &str,
) {
    push_php_reference(
        references,
        app,
        format!("{prefix}.program"),
        &spec.program,
        effective_php,
    );
    for (index, arg) in spec.args.iter().enumerate() {
        push_php_reference(
            references,
            app,
            format!("{prefix}.args[{index}]"),
            arg,
            effective_php,
        );
    }
    push_php_reference(
        references,
        app,
        format!("{prefix}.cwd"),
        &spec.cwd,
        effective_php,
    );
    push_php_reference(
        references,
        app,
        format!("{prefix}.shell"),
        &spec.shell,
        effective_php,
    );
    for (key, value) in &spec.env {
        push_php_reference(
            references,
            app,
            format!("{prefix}.env.{key}"),
            value,
            effective_php,
        );
    }
}

pub(crate) fn scan_hardcoded_php(config: &UpdaterConfig, effective_php: &str) -> Vec<PhpReference> {
    let effective_php = effective_php.trim();
    if effective_php.is_empty() {
        return Vec::new();
    }
    let mut references = Vec::new();
    for app in &config.apps {
        for (field, value) in [
            ("installCommand", app.install_command.as_str()),
            ("buildCommand", app.build_command.as_str()),
            ("preUpdateCommand", app.pre_update_command.as_str()),
            ("postUpdateCommand", app.post_update_command.as_str()),
        ] {
            push_php_reference(&mut references, app, field, value, effective_php);
        }
        for (group, specs) in [
            ("commands.install", app.commands.install.as_slice()),
            ("commands.build", app.commands.build.as_slice()),
            ("commands.preUpdate", app.commands.pre_update.as_slice()),
            ("commands.postUpdate", app.commands.post_update.as_slice()),
        ] {
            for (index, spec) in specs.iter().enumerate() {
                push_php_spec(
                    &mut references,
                    app,
                    &format!("{group}[{index}]"),
                    spec,
                    effective_php,
                );
            }
        }
    }
    references
}

pub(crate) fn php_reference_check(
    config: Option<&UpdaterConfig>,
    effective_php: &str,
) -> DoctorCheck {
    if effective_php.trim().is_empty() {
        return check(
            "php.references",
            "php",
            "PHP references",
            DoctorStatus::Skipped,
            "PHP is not configured; hardcoded references were not checked.",
            "",
        );
    }
    let Some(config) = config else {
        return check(
            "php.references",
            "php",
            "PHP references",
            DoctorStatus::Skipped,
            "projects.json is invalid; hardcoded PHP references were not checked.",
            "",
        );
    };
    let references = scan_hardcoded_php(config, effective_php);
    if references.is_empty() {
        return check(
            "php.references",
            "php",
            "PHP references",
            DoctorStatus::Ok,
            "No hardcoded php.exe paths differ from the effective PHP path.",
            "",
        );
    }
    let apps: Vec<String> = references
        .iter()
        .map(|reference| reference.app_name.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let fields: Vec<String> = references
        .iter()
        .take(6)
        .map(|reference| format!("{} ({})", reference.app_name, reference.field))
        .collect();
    let more = references.len().saturating_sub(6);
    check(
        "php.references",
        "php",
        "PHP references",
        DoctorStatus::Warning,
        format!(
            "{} hardcoded PHP reference(s) in {} app(s): {}{}.",
            references.len(),
            apps.len(),
            fields.join(", "),
            if more == 0 {
                String::new()
            } else {
                format!(", and {more} more")
            }
        ),
        "Review the app commands and use ${PHP} or the effective PHP path. Doctor does not modify projects.json.",
    )
}

fn compact_error(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.chars().count() <= MAX_ERROR_CHARS {
        trimmed.to_string()
    } else {
        let mut truncated: String = trimmed.chars().take(MAX_ERROR_CHARS).collect();
        truncated.push_str("...");
        truncated
    }
}

fn redact_display_url(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut index = 0usize;
    while let Some(relative) = input[index..].find("://") {
        let scheme_end = index + relative + 3;
        output.push_str(&input[index..scheme_end]);
        let rest = &input[scheme_end..];
        let authority_end = rest
            .find(['/', '\\', ' ', '\n', '\r', '"', '\'', '?', '#'])
            .map(|offset| scheme_end + offset)
            .unwrap_or(input.len());
        let authority = &input[scheme_end..authority_end];
        if let Some(at) = authority.rfind('@') {
            output.push_str("[REDACTED]@");
            output.push_str(&input[scheme_end + at + 1..authority_end]);
        } else {
            output.push_str(authority);
        }
        index = authority_end;
    }
    output.push_str(&input[index..]);
    output
}

fn redact_text(text: &str) -> String {
    redact_secrets(&redact_display_url(text))
}

fn sanitize_checks(checks: Vec<DoctorCheck>) -> Vec<DoctorCheck> {
    checks
        .into_iter()
        .map(|mut check| {
            check.detail = redact_text(&check.detail);
            check.hint = redact_text(&check.hint);
            check
        })
        .collect()
}

fn load_services_read_only(path: &Path) -> (Vec<ServiceConfig>, Option<String>, bool) {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return (Vec::new(), None, false)
        }
        Err(error) => {
            return (
                Vec::new(),
                Some(format!("Failed to read services.json: {error}")),
                true,
            )
        }
    };
    if raw.trim().is_empty() {
        return (Vec::new(), None, true);
    }
    match sm_config::parse_services_json(&raw) {
        Ok(services) => (services, None, true),
        Err(error) => (Vec::new(), Some(error.to_string()), true),
    }
}

fn load_doctor_settings(
    paths: &ConfigPaths,
    config: &Result<UpdaterConfig, String>,
) -> (Settings, SettingsFileState) {
    let settings_file = settings_path_for(&paths.projects_path());
    match load_settings(&settings_file) {
        Ok(Some(settings)) => (settings, SettingsFileState::Valid),
        Ok(None) => {
            let fallback = config
                .as_ref()
                .map(|config| from_legacy(&config.settings, &config.trigger))
                .unwrap_or_default();
            (fallback, SettingsFileState::Missing)
        }
        Err(error) => {
            let fallback = config
                .as_ref()
                .map(|config| from_legacy(&config.settings, &config.trigger))
                .unwrap_or_default();
            (fallback, SettingsFileState::Invalid(error.to_string()))
        }
    }
}

fn probe_tool(spec: &ToolSpec, path: &str) -> ToolProbe {
    let path = path.trim();
    if path.is_empty() {
        return ToolProbe::NotConfigured;
    }
    let candidate = Path::new(path);
    if !candidate.is_file() {
        return ToolProbe::MissingPath;
    }
    let test = settings::test_tool_path_with_timeout(spec, candidate, TOOL_PROBE_TIMEOUT);
    ToolProbe::Tested {
        success: test.success,
        version: test.version,
        error: test
            .error
            .unwrap_or_else(|| "version check failed".to_string()),
    }
}

fn git_ls_remote(git_path: &Path, repo_url: &str) -> Result<RemoteProbe, String> {
    let cwd = git_path
        .parent()
        .filter(|parent| parent.is_dir())
        .unwrap_or_else(|| Path::new("."));
    let args = vec![
        "ls-remote".to_string(),
        "--heads".to_string(),
        repo_url.to_string(),
    ];
    let mut env = std::collections::BTreeMap::new();
    env.insert("GIT_TERMINAL_PROMPT".to_string(), "0".to_string());
    env.insert("GCM_INTERACTIVE".to_string(), "never".to_string());
    let output = SystemRunner
        .run_with_env(git_path, &args, cwd, REMOTE_PROBE_TIMEOUT, &env)
        .map_err(|error| compact_error(&error.to_string()))?;
    if !output.success() {
        let text = output.combined();
        return Err(if text.trim().is_empty() {
            format!("git ls-remote exited with code {}.", output.exit_code)
        } else {
            compact_error(&text)
        });
    }
    let heads = output
        .stdout
        .lines()
        .filter(|line| !line.trim().is_empty())
        .count();
    Ok(RemoteProbe { heads })
}

fn probe_ssh_service() -> SshServiceProbe {
    match sm_win::query_status("sshd") {
        Ok(status) if status.state.is_running() => SshServiceProbe::Running,
        Ok(status) => SshServiceProbe::Stopped(status.state.to_string()),
        Err(ScmError::NotFound(_)) => SshServiceProbe::NotFound,
        Err(error) => SshServiceProbe::Error(error.to_string()),
    }
}

fn probe_ssh_port() -> Result<bool, String> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, 22));
    match TcpStream::connect_timeout(&address, SSH_PORT_TIMEOUT) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

fn pending_summary(pending: &apache::ApachePendingChange) -> String {
    format!(
        "saved {}; {} service(s), {} app(s)",
        pending.created_at,
        pending.services.len(),
        pending.apps.len()
    )
}

fn build_report(base_path: &Path, checks: Vec<DoctorCheck>) -> DoctorReport {
    let checks = sanitize_checks(checks);
    let counts = doctor_counts(&checks);
    let overall = overall_status(&checks);
    DoctorReport {
        generated_at: Local::now().to_rfc3339_opts(SecondsFormat::Millis, true),
        overall,
        checks,
        counts,
        base_path: base_path.to_string_lossy().to_string(),
    }
}

/// Build a complete readiness report with real system probes.
pub(crate) fn run_doctor_at(paths: &ConfigPaths) -> DoctorReport {
    let config = load_config(&paths.projects_path());
    let (settings, settings_state) = load_doctor_settings(paths, &config);
    let (services, services_warning, services_exists) =
        load_services_read_only(&paths.services_path());
    let services_valid = services_warning.is_none();

    let mut checks = Vec::new();

    // Environment
    match sm_win::is_elevated() {
        Ok(true) => checks.push(check(
            "environment.admin",
            "environment",
            "Administrator",
            DoctorStatus::Ok,
            "Service Manager is running with Administrator privileges.",
            "",
        )),
        Ok(false) => checks.push(check(
            "environment.admin",
            "environment",
            "Administrator",
            DoctorStatus::Warning,
            "Service Manager is not running as Administrator.",
            "Apply/install and service control need elevation; restart Service Manager as Administrator.",
        )),
        Err(error) => checks.push(check(
            "environment.admin",
            "environment",
            "Administrator",
            DoctorStatus::Warning,
            format!("Could not check Administrator privileges: {error}"),
            "Restart Service Manager as Administrator if updates need service control.",
        )),
    }

    // Tools (effective settings.json paths, legacy fallback when absent)
    for spec in TOOL_SPECS {
        let path = settings::settings_tool_path(&settings, spec.name);
        checks.push(tool_check(spec, path.trim(), probe_tool(spec, path)));
    }

    // Settings
    checks.push(settings_file_check(&settings_state));
    checks.push(apache_settings_check(&settings));

    // Registry
    checks.extend(registry_checks(
        paths.projects_path().is_file(),
        &config,
        services_valid.then_some(services.as_slice()),
    ));

    // Services
    let apache_services = services
        .iter()
        .filter(|service| service.service_type.normalized() == ServiceType::Apache)
        .count();
    checks.push(services_config_check(
        services_exists,
        services_warning.as_deref(),
        &services,
    ));
    checks.push(apache_service_presence_check(
        !settings.tools.apache.path().trim().is_empty(),
        services_valid,
        apache_services,
    ));

    let mut referenced = BTreeSet::new();
    if let Ok(config) = &config {
        for app in config.apps.iter() {
            let name = app.web_server_service.trim();
            if name.is_empty() || !referenced.insert(name.to_ascii_lowercase()) {
                continue;
            }
            let Some(service) = services
                .iter()
                .find(|service| service.name.eq_ignore_ascii_case(name))
            else {
                continue;
            };
            if service.service_type.normalized() != ServiceType::Apache {
                continue;
            }
            let result = match sm_win::query_status(&service.name) {
                Ok(info) => Ok(Some(info.state.to_string())),
                Err(ScmError::NotFound(_)) => Ok(None),
                Err(error) => Err(error.to_string()),
            };
            checks.push(service_state_check(&service.name, result));
        }
    }

    // Remote access (read-only ls-remote per enabled app with repoUrl)
    let git_path = settings.tools.git.path.trim();
    let git_ready = !git_path.is_empty() && Path::new(git_path).is_file();
    if let Ok(config) = &config {
        for app in config.apps.iter().filter(|app| app.enabled) {
            if app.repo_url.trim().is_empty() {
                continue;
            }
            let probe = if git_ready {
                Some(git_ls_remote(Path::new(git_path), app.repo_url.trim()))
            } else {
                None
            };
            checks.push(remote_check(app, git_ready, probe));
        }
    }

    // Scheduled Task
    let task_name = settings.privileged_execution.task_name.trim();
    let task = settings::task_status_blocking(task_name);
    checks.push(task_check(task_name, task));

    // SSH (best effort, read-only; no firewall changes)
    let ssh_service = probe_ssh_service();
    let ssh_error = matches!(ssh_service, SshServiceProbe::Error(_));
    checks.push(ssh_service_check(ssh_service));
    if ssh_error {
        checks.push(ssh_port_check(
            Err("sshd status is unavailable".to_string()),
        ));
    } else {
        checks.push(ssh_port_check(probe_ssh_port()));
    }

    // Storage
    for (id, label, dir) in [
        ("storage.data", "data folder", paths.data_dir()),
        ("storage.logs", "logs folder", paths.logs_dir()),
        ("storage.backup", "backup folder", paths.backup_dir()),
    ] {
        checks.push(storage_check(id, label, &dir, probe_writable(&dir)));
    }
    checks.push(disk_space_check(sm_win::free_space(paths.home())));

    // Pending Apache change
    let pending_path = apache::pending_path_for(&paths.data_dir());
    let pending =
        apache::load_pending_at(&pending_path).map(|pending| pending.as_ref().map(pending_summary));
    checks.push(pending_check(pending));

    // Hardcoded PHP references (read-only)
    checks.push(php_reference_check(
        config.as_ref().ok(),
        settings.tools.php.path.trim(),
    ));

    build_report(paths.home(), checks)
}

#[tauri::command]
pub async fn run_doctor() -> Result<DoctorReport, String> {
    let paths = ConfigPaths::detect();
    tauri::async_runtime::spawn_blocking(move || run_doctor_at(&paths))
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sm_updater::registry::{AppConfig, CustomCommands, Preset};

    fn sample_check(id: &str, status: DoctorStatus) -> DoctorCheck {
        check(id, "test", id, status, "detail", "")
    }

    #[test]
    fn build_report_aggregates_and_redacts_for_the_tauri_payload() {
        let report = build_report(
            Path::new(r"C:\tools\ServiceManager"),
            vec![
                check("test.ok", "test", "OK", DoctorStatus::Ok, "ready", ""),
                check(
                    "test.secret",
                    "test",
                    "Secret",
                    DoctorStatus::Warning,
                    "https://user:tokenvalue@example.com/x ghp_abcdefghijklmnop12345",
                    "",
                ),
            ],
        );

        assert_eq!(report.overall, DoctorStatus::Warning);
        assert_eq!(report.counts.ok, 1);
        assert_eq!(report.counts.warning, 1);
        assert_eq!(report.counts.error, 0);
        assert_eq!(report.counts.skipped, 0);
        assert_eq!(report.base_path, r"C:\tools\ServiceManager");
        assert!(report.generated_at.contains('T'));
        assert!(!report.checks[1].detail.contains("tokenvalue"));
        assert!(!report.checks[1]
            .detail
            .contains("ghp_abcdefghijklmnop12345"));
    }

    #[test]
    fn aggregates_overall_status_with_errors_first_and_skipped_ignored() {
        assert_eq!(
            overall_status(&[
                sample_check("a", DoctorStatus::Ok),
                sample_check("b", DoctorStatus::Skipped),
            ]),
            DoctorStatus::Ok
        );
        assert_eq!(
            overall_status(&[
                sample_check("a", DoctorStatus::Warning),
                sample_check("b", DoctorStatus::Skipped),
            ]),
            DoctorStatus::Warning
        );
        assert_eq!(
            overall_status(&[
                sample_check("a", DoctorStatus::Error),
                sample_check("b", DoctorStatus::Warning),
            ]),
            DoctorStatus::Error
        );
        assert_eq!(
            overall_status(&[sample_check("a", DoctorStatus::Skipped)]),
            DoctorStatus::Ok
        );
    }

    #[test]
    fn counts_each_status() {
        let counts = doctor_counts(&[
            sample_check("a", DoctorStatus::Ok),
            sample_check("b", DoctorStatus::Warning),
            sample_check("c", DoctorStatus::Error),
            sample_check("d", DoctorStatus::Skipped),
            sample_check("e", DoctorStatus::Error),
        ]);
        assert_eq!(
            counts,
            DoctorCounts {
                ok: 1,
                warning: 1,
                error: 2,
                skipped: 1
            }
        );
    }

    fn spec(name: &str) -> &'static ToolSpec {
        TOOL_SPECS
            .iter()
            .find(|spec| spec.name == name)
            .expect("known tool spec")
    }

    #[test]
    fn required_tool_missing_is_error_and_optional_missing_is_warning() {
        let required = tool_check(spec("git"), "", ToolProbe::NotConfigured);
        assert_eq!(required.status, DoctorStatus::Error);
        assert!(required.detail.contains("required"));

        let optional = tool_check(spec("pnpm"), "", ToolProbe::NotConfigured);
        assert_eq!(optional.status, DoctorStatus::Warning);
        assert!(optional.detail.contains("not configured"));
    }

    #[test]
    fn configured_tool_invalid_path_and_version_failure_follow_requiredness() {
        let required_missing =
            tool_check(spec("php"), r"C:\missing\php.exe", ToolProbe::MissingPath);
        assert_eq!(required_missing.status, DoctorStatus::Error);

        let optional_missing = tool_check(spec("go"), r"C:\missing\go.exe", ToolProbe::MissingPath);
        assert_eq!(optional_missing.status, DoctorStatus::Warning);

        let required_failure = tool_check(
            spec("composer"),
            r"C:\composer.bat",
            ToolProbe::Tested {
                success: false,
                version: String::new(),
                error: "exit code 1".into(),
            },
        );
        assert_eq!(required_failure.status, DoctorStatus::Error);

        let optional_failure = tool_check(
            spec("nssm"),
            r"C:\nssm.exe",
            ToolProbe::Tested {
                success: false,
                version: String::new(),
                error: "exit code 1".into(),
            },
        );
        assert_eq!(optional_failure.status, DoctorStatus::Warning);

        let ok = tool_check(
            spec("git"),
            r"C:\git.exe",
            ToolProbe::Tested {
                success: true,
                version: "git version 2.45.0".into(),
                error: String::new(),
            },
        );
        assert_eq!(ok.status, DoctorStatus::Ok);
        assert!(ok.detail.contains("2.45.0"));
    }

    #[test]
    fn settings_missing_is_legacy_warning_and_invalid_is_error() {
        assert_eq!(
            settings_file_check(&SettingsFileState::Missing).status,
            DoctorStatus::Warning
        );
        assert_eq!(
            settings_file_check(&SettingsFileState::Valid).status,
            DoctorStatus::Ok
        );
        let invalid = settings_file_check(&SettingsFileState::Invalid("bad JSON".into()));
        assert_eq!(invalid.status, DoctorStatus::Error);
        assert!(invalid.detail.contains("bad JSON"));
    }

    #[test]
    fn apache_settings_validate_root_and_config_presence() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("apache");
        std::fs::create_dir_all(root.join("conf")).unwrap();
        std::fs::write(root.join("conf/httpd.conf"), b"# config").unwrap();

        let mut settings = Settings::default();
        assert_eq!(
            apache_settings_check(&settings).status,
            DoctorStatus::Skipped
        );

        settings.tools.apache.tool.path = root.join("bin/httpd.exe").to_string_lossy().to_string();
        settings.tools.apache.root = root.to_string_lossy().to_string();
        settings.tools.apache.config = "conf/httpd.conf".into();
        assert_eq!(apache_settings_check(&settings).status, DoctorStatus::Ok);

        settings.tools.apache.config = "conf/missing.conf".into();
        assert_eq!(
            apache_settings_check(&settings).status,
            DoctorStatus::Warning
        );

        settings.tools.apache.root.clear();
        assert_eq!(
            apache_settings_check(&settings).status,
            DoctorStatus::Warning
        );
    }

    #[test]
    fn registry_missing_is_warning_and_invalid_is_error() {
        let missing = registry_checks(false, &Ok(UpdaterConfig::default()), Some(&[]));
        assert_eq!(missing[0].status, DoctorStatus::Warning);

        let invalid = registry_checks(true, &Err("Config JSON tidak valid".to_string()), Some(&[]));
        assert_eq!(invalid[0].status, DoctorStatus::Error);
        assert!(invalid[0].detail.contains("invalid"));
    }

    #[test]
    fn registry_warns_for_missing_local_path_and_bad_web_service() {
        let mut config = UpdaterConfig::default();
        config.apps.push(AppConfig {
            id: "demo".into(),
            name: "Demo".into(),
            enabled: true,
            repo_url: "https://example.com/demo.git".into(),
            local_path: r"C:\does-not-exist\demo".into(),
            web_server_service: "MissingApache".into(),
            ..AppConfig::default()
        });

        let checks = registry_checks(true, &Ok(config), Some(&[]));
        assert!(checks
            .iter()
            .any(|check| check.id == "registry.app.demo.path"
                && check.status == DoctorStatus::Warning));
        assert!(checks
            .iter()
            .any(|check| check.id == "registry.app.demo.webServerService"
                && check.status == DoctorStatus::Warning));
    }

    #[test]
    fn registry_accepts_apache_web_service_and_rejects_non_apache() {
        let mut service = ServiceConfig {
            name: "Apache2.4".into(),
            service_type: ServiceType::Apache,
            ..ServiceConfig::default()
        };
        service.normalize();
        let mut config = UpdaterConfig::default();
        config.apps.push(AppConfig {
            id: "demo".into(),
            name: "Demo".into(),
            enabled: false,
            web_server_service: "Apache2.4".into(),
            ..AppConfig::default()
        });
        let checks = registry_checks(true, &Ok(config), Some(&[service.clone()]));
        assert!(checks.iter().any(
            |check| check.id.ends_with(".webServerService") && check.status == DoctorStatus::Ok
        ));

        let mut nssm = ServiceConfig {
            name: "api".into(),
            service_type: ServiceType::Nssm,
            ..ServiceConfig::default()
        };
        nssm.normalize();
        let mut config = UpdaterConfig::default();
        config.apps.push(AppConfig {
            id: "demo".into(),
            web_server_service: "api".into(),
            ..AppConfig::default()
        });
        let checks = registry_checks(true, &Ok(config), Some(&[nssm]));
        assert!(checks
            .iter()
            .any(|check| check.id.ends_with(".webServerService")
                && check.status == DoctorStatus::Warning));
    }

    #[test]
    fn services_config_error_and_apache_presence_warning() {
        let invalid = services_config_check(true, Some("broken"), &[]);
        assert_eq!(invalid.status, DoctorStatus::Error);

        let missing = services_config_check(false, None, &[]);
        assert_eq!(missing.status, DoctorStatus::Warning);

        let no_apache = apache_service_presence_check(true, true, 0);
        assert_eq!(no_apache.status, DoctorStatus::Warning);

        let skipped = apache_service_presence_check(false, true, 0);
        assert_eq!(skipped.status, DoctorStatus::Skipped);
    }

    #[test]
    fn service_state_not_installed_is_warning_and_installed_is_ok() {
        let missing = service_state_check("Apache2.4", Ok(None));
        assert_eq!(missing.status, DoctorStatus::Warning);
        assert!(missing.detail.contains("not installed"));

        let running = service_state_check("Apache2.4", Ok(Some("Running".into())));
        assert_eq!(running.status, DoctorStatus::Ok);
        assert!(running.detail.contains("Running"));

        let error = service_state_check("Apache2.4", Err("access denied".into()));
        assert_eq!(error.status, DoctorStatus::Warning);
    }

    #[test]
    fn task_status_mapping() {
        let not_installed = task_check(
            "ServiceManager-Updater",
            Ok(TaskStatusView {
                installed: false,
                ..TaskStatusView::default()
            }),
        );
        assert_eq!(not_installed.status, DoctorStatus::Warning);
        assert!(not_installed.detail.contains("not installed"));

        let ready = task_check(
            "ServiceManager-Updater",
            Ok(TaskStatusView {
                installed: true,
                state: "Ready".into(),
                last_result: 0,
                ..TaskStatusView::default()
            }),
        );
        assert_eq!(ready.status, DoctorStatus::Ok);

        let not_ready = task_check(
            "ServiceManager-Updater",
            Ok(TaskStatusView {
                installed: true,
                state: "Disabled".into(),
                last_result: 0,
                ..TaskStatusView::default()
            }),
        );
        assert_eq!(not_ready.status, DoctorStatus::Warning);
        assert!(not_ready.detail.contains("Disabled"));

        let nonzero = task_check(
            "ServiceManager-Updater",
            Ok(TaskStatusView {
                installed: true,
                state: "Ready".into(),
                last_result: 1,
                ..TaskStatusView::default()
            }),
        );
        assert_eq!(nonzero.status, DoctorStatus::Warning);
        assert!(nonzero.detail.contains('1'));
    }

    #[test]
    fn pending_change_is_warning_and_absent_is_ok() {
        let pending = pending_check(Ok(Some("saved now; 1 service(s), 1 app(s)".into())));
        assert_eq!(pending.status, DoctorStatus::Warning);
        assert!(pending.detail.contains("pending"));

        assert_eq!(pending_check(Ok(None)).status, DoctorStatus::Ok);
        assert_eq!(
            pending_check(Err("bad json".into())).status,
            DoctorStatus::Warning
        );
    }

    #[test]
    fn ssh_status_mapping() {
        assert_eq!(
            ssh_service_check(SshServiceProbe::Running).status,
            DoctorStatus::Ok
        );
        assert_eq!(
            ssh_service_check(SshServiceProbe::Stopped("Stopped".into())).status,
            DoctorStatus::Warning
        );
        assert_eq!(
            ssh_service_check(SshServiceProbe::NotFound).status,
            DoctorStatus::Warning
        );
        assert_eq!(
            ssh_service_check(SshServiceProbe::Error("denied".into())).status,
            DoctorStatus::Skipped
        );
    }

    #[test]
    fn writability_probe_uses_temp_dirs_and_leaves_no_file() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(probe_writable(temp.path()), WriteProbe::Writable);
        assert_eq!(
            probe_writable(&temp.path().join("missing")),
            WriteProbe::Missing
        );

        let file = temp.path().join("not-a-dir");
        std::fs::write(&file, b"x").unwrap();
        assert!(matches!(probe_writable(&file), WriteProbe::NotWritable(_)));

        let entries: Vec<_> = std::fs::read_dir(temp.path())
            .unwrap()
            .flatten()
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".doctor-probe-")
            })
            .collect();
        assert!(entries.is_empty());
    }

    #[test]
    fn storage_and_disk_statuses() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            storage_check("storage.data", "data", temp.path(), WriteProbe::Writable).status,
            DoctorStatus::Ok
        );
        assert_eq!(
            storage_check(
                "storage.data",
                "data",
                &temp.path().join("missing"),
                WriteProbe::Missing
            )
            .status,
            DoctorStatus::Warning
        );
        assert_eq!(
            storage_check(
                "storage.data",
                "data",
                temp.path(),
                WriteProbe::NotWritable("denied".into())
            )
            .status,
            DoctorStatus::Error
        );

        assert_eq!(
            disk_space_check(Ok(LOW_DISK_BYTES - 1)).status,
            DoctorStatus::Warning
        );
        assert_eq!(
            disk_space_check(Ok(LOW_DISK_BYTES + 1)).status,
            DoctorStatus::Ok
        );
        assert_eq!(
            disk_space_check(Err("unavailable".into())).status,
            DoctorStatus::Skipped
        );
    }

    #[test]
    fn redacts_token_like_details_and_url_userinfo() {
        let checks = sanitize_checks(vec![check(
            "test.secret",
            "test",
            "Secret",
            DoctorStatus::Warning,
            "failed https://oauth2:glpat-abcdefghij@gitlab.com/repo.git token=secretvalue ghp_abcdefghijklmnop12345",
            "retry with password=hunter2",
        )]);
        assert!(!checks[0].detail.contains("glpat-abcdefghij"));
        assert!(!checks[0].detail.contains("secretvalue"));
        assert!(!checks[0].detail.contains("ghp_abcdefghijklmnop12345"));
        assert!(!checks[0].hint.contains("hunter2"));
        assert!(checks[0].detail.contains("[REDACTED]"));
        assert!(checks[0].hint.contains("[REDACTED]"));
    }

    #[test]
    fn remote_check_skips_empty_url_and_warns_on_failure() {
        let empty = AppConfig {
            id: "empty".into(),
            repo_url: String::new(),
            ..AppConfig::default()
        };
        assert_eq!(
            remote_check(&empty, true, None).status,
            DoctorStatus::Skipped
        );

        let app = AppConfig {
            id: "demo".into(),
            name: "Demo".into(),
            repo_url: "https://oauth2:glpat-abcdefghij@example.com/demo.git".into(),
            ..AppConfig::default()
        };
        let failed = remote_check(&app, true, Some(Err("authentication failed".into())));
        assert_eq!(failed.status, DoctorStatus::Warning);
        assert!(!failed.detail.contains("glpat-abcdefghij"));

        let ok = remote_check(&app, true, Some(Ok(RemoteProbe { heads: 3 })));
        assert_eq!(ok.status, DoctorStatus::Ok);
        assert!(ok.detail.contains('3'));
    }

    #[test]
    fn php_scan_finds_hardcoded_paths_but_skips_variables_and_effective_path() {
        let mut config = UpdaterConfig::default();
        config.apps.push(AppConfig {
            id: "demo".into(),
            name: "Demo".into(),
            enabled: true,
            install_command: r#""C:\old-php\php.exe" artisan migrate"#.into(),
            build_command: "npm run build".into(),
            ..AppConfig::default()
        });
        config.apps[0].commands = CustomCommands {
            build: vec![CommandSpec {
                name: "build".into(),
                program: "${PHP}".into(),
                args: vec![r"${PHP_HOME}\artisan".into()],
                ..CommandSpec::default()
            }],
            ..CustomCommands::default()
        };

        let references = scan_hardcoded_php(&config, r"C:\new-php\php.exe");
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].field, "installCommand");

        assert!(scan_hardcoded_php(&config, r"C:\old-php\php.exe").is_empty());

        let variable_only = php_reference_check(Some(&config), "");
        assert_eq!(variable_only.status, DoctorStatus::Skipped);
        let warning = php_reference_check(Some(&config), r"C:\new-php\php.exe");
        assert_eq!(warning.status, DoctorStatus::Warning);
        assert!(warning.detail.contains("1 hardcoded PHP reference"));
    }

    #[test]
    fn php_scan_covers_custom_command_fields() {
        let mut config = UpdaterConfig::default();
        let mut app = AppConfig {
            id: "demo".into(),
            name: "Demo".into(),
            enabled: true,
            preset: Preset::Custom,
            ..AppConfig::default()
        };
        app.commands.pre_update = vec![CommandSpec {
            name: "pre".into(),
            shell: r#""D:\php\php.exe" artisan down"#.into(),
            ..CommandSpec::default()
        }];
        config.apps.push(app);

        let references = scan_hardcoded_php(&config, r"C:\new-php\php.exe");
        assert_eq!(references.len(), 1);
        assert_eq!(references[0].field, "commands.preUpdate[0].shell");
    }
}
