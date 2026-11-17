//! Explicit Apache propagation from Settings to `services.json` and Windows SCM.
//!
//! Saving settings never changes `services.json` directly. When the Apache
//! path/root/config/version changes, a pending preview is written to
//! `data/apache-pending.json`. The separate `apply_apache_change` command
//! updates the service configs and re-applies Windows services, with backup,
//! rollback, and an audit log.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::{Local, SecondsFormat};
use serde::{Deserialize, Serialize};
use sm_config::ConfigPaths;
use sm_core::{ServiceConfig, ServiceType};
use sm_updater::registry::UpdaterConfig;
use sm_updater::settings::ApacheToolPath;
use sm_win::WaitOptions;

pub(crate) const PENDING_FILE: &str = "apache-pending.json";
pub(crate) const AUDIT_FILE: &str = "apache-apply.log";
const APPLY_TIMEOUT: Duration = Duration::from_secs(60);

fn now_iso() -> String {
    Local::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn has_control_characters(value: &str) -> bool {
    value.chars().any(char::is_control)
}

fn check_text(field: &str, value: &str) -> Result<(), String> {
    if has_control_characters(value) {
        return Err(format!(
            "Apache pending field '{field}' contains control characters."
        ));
    }
    Ok(())
}

/// One affected Apache service and the values before/after propagation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApacheServiceChange {
    pub name: String,
    pub old_executable: String,
    pub new_executable: String,
    pub old_root: String,
    pub new_root: String,
}

/// App linked to an affected Apache service through `webServerService`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApacheAppChange {
    pub id: String,
    pub name: String,
}

/// Persisted preview in `data/apache-pending.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApachePendingChange {
    pub old_path: String,
    pub new_path: String,
    pub old_root: String,
    pub new_root: String,
    pub old_config: String,
    pub new_config: String,
    pub old_version: String,
    pub new_version: String,
    pub services: Vec<ApacheServiceChange>,
    pub apps: Vec<ApacheAppChange>,
    pub created_at: String,
}

impl ApachePendingChange {
    fn normalize(&mut self) {
        self.old_path = self.old_path.trim().to_string();
        self.new_path = self.new_path.trim().to_string();
        self.old_root = self.old_root.trim().to_string();
        self.new_root = self.new_root.trim().to_string();
        self.old_config = self.old_config.trim().to_string();
        self.new_config = self.new_config.trim().to_string();
        self.old_version = self.old_version.trim().to_string();
        self.new_version = self.new_version.trim().to_string();
        self.created_at = self.created_at.trim().to_string();

        for service in &mut self.services {
            service.name = service.name.trim().to_string();
            service.old_executable = service.old_executable.trim().to_string();
            service.new_executable = service.new_executable.trim().to_string();
            service.old_root = service.old_root.trim().to_string();
            service.new_root = service.new_root.trim().to_string();
        }
        let mut seen = BTreeSet::new();
        self.services.retain(|service| {
            let key = service.name.to_ascii_lowercase();
            !key.is_empty() && seen.insert(key)
        });

        for app in &mut self.apps {
            app.id = app.id.trim().to_string();
            app.name = app.name.trim().to_string();
        }
        let mut seen = BTreeSet::new();
        self.apps.retain(|app| {
            let key = app.id.to_ascii_lowercase();
            !key.is_empty() && seen.insert(key)
        });
    }

    fn validate(&self) -> Result<(), String> {
        for (field, value) in [
            ("oldPath", self.old_path.as_str()),
            ("newPath", self.new_path.as_str()),
            ("oldRoot", self.old_root.as_str()),
            ("newRoot", self.new_root.as_str()),
            ("oldConfig", self.old_config.as_str()),
            ("newConfig", self.new_config.as_str()),
            ("oldVersion", self.old_version.as_str()),
            ("newVersion", self.new_version.as_str()),
            ("createdAt", self.created_at.as_str()),
        ] {
            check_text(field, value)?;
        }
        if self.new_path.is_empty() {
            return Err("Apache pending change has an empty newPath.".to_string());
        }
        if self.new_root.is_empty() {
            return Err("Apache pending change has an empty newRoot.".to_string());
        }
        if self.services.is_empty() {
            return Err("Apache pending change has no affected services.".to_string());
        }
        for service in &self.services {
            for (field, value) in [
                ("services[].name", service.name.as_str()),
                ("services[].oldExecutable", service.old_executable.as_str()),
                ("services[].newExecutable", service.new_executable.as_str()),
                ("services[].oldRoot", service.old_root.as_str()),
                ("services[].newRoot", service.new_root.as_str()),
            ] {
                check_text(field, value)?;
            }
            if service.name.is_empty() {
                return Err("Apache pending change has an empty service name.".to_string());
            }
        }
        for app in &self.apps {
            check_text("apps[].id", &app.id)?;
            check_text("apps[].name", &app.name)?;
            if app.id.is_empty() {
                return Err("Apache pending change has an empty app id.".to_string());
            }
        }
        Ok(())
    }

    pub fn view(&self) -> ApacheChangeView {
        ApacheChangeView {
            changed: true,
            old_path: self.old_path.clone(),
            new_path: self.new_path.clone(),
            old_root: self.old_root.clone(),
            new_root: self.new_root.clone(),
            old_config: self.old_config.clone(),
            new_config: self.new_config.clone(),
            old_version: self.old_version.clone(),
            new_version: self.new_version.clone(),
            services: self.services.clone(),
            apps: self.apps.clone(),
            created_at: self.created_at.clone(),
        }
    }
}

/// Serializable preview used by the Settings UI.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApacheChangeView {
    pub changed: bool,
    pub old_path: String,
    pub new_path: String,
    pub old_root: String,
    pub new_root: String,
    pub old_config: String,
    pub new_config: String,
    pub old_version: String,
    pub new_version: String,
    pub services: Vec<ApacheServiceChange>,
    pub apps: Vec<ApacheAppChange>,
    pub created_at: String,
}

/// Result returned after `apply_apache_change`.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApacheApplyResultView {
    pub success: bool,
    pub services: Vec<String>,
    pub errors: Vec<String>,
    pub audit_path: String,
}

pub(crate) fn pending_path_for(data_dir: &Path) -> PathBuf {
    data_dir.join(PENDING_FILE)
}

pub(crate) fn load_pending_at(path: &Path) -> Result<Option<ApachePendingChange>, String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(format!(
                "Failed to read Apache pending change {}: {error}",
                path.display()
            ))
        }
    };
    if raw.trim().is_empty() {
        return Ok(None);
    }

    let mut pending: ApachePendingChange = serde_json::from_str(&raw)
        .map_err(|error| format!("Apache pending JSON is invalid: {error}"))?;
    pending.normalize();
    pending.validate()?;
    Ok(Some(pending))
}

pub(crate) fn save_pending_at(path: &Path, pending: &ApachePendingChange) -> Result<(), String> {
    let mut pending = pending.clone();
    pending.normalize();
    pending.validate()?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let json = serde_json::to_string_pretty(&pending)
        .map_err(|error| format!("Failed to serialize Apache pending change: {error}"))?;
    let temp_path = path.with_extension(format!("json.tmp-{}", std::process::id()));
    fs::write(&temp_path, json).map_err(|error| error.to_string())?;
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(format!(
            "Failed to save Apache pending change {}: {error}",
            path.display()
        ));
    }
    Ok(())
}

pub(crate) fn clear_pending_at(path: &Path) -> Result<(), String> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!(
            "Failed to clear Apache pending change {}: {error}",
            path.display()
        )),
    }
}

/// Compare only the user-editable Apache fields; `detectedAt` alone must not
/// trigger a service update.
pub(crate) fn apache_settings_changed(old: &ApacheToolPath, new: &ApacheToolPath) -> bool {
    old.path().trim() != new.path().trim()
        || old.root.trim() != new.root.trim()
        || old.config.trim() != new.config.trim()
        || old.tool.version.trim() != new.tool.version.trim()
}

fn compare_path(value: &str) -> String {
    value
        .trim()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_ascii_lowercase()
}

fn paths_equal(left: &str, right: &str) -> bool {
    let left = compare_path(left);
    let right = compare_path(right);
    !left.is_empty() && left == right
}

fn path_is_under(path: &str, root: &str) -> bool {
    let path = compare_path(path);
    let root = compare_path(root);
    if path.is_empty() || root.is_empty() {
        return false;
    }
    if path == root {
        return true;
    }
    path.strip_prefix(&root)
        .map(|rest| rest.starts_with('\\'))
        .unwrap_or(false)
}

/// Match an Apache service against the previous effective Apache settings.
///
/// The first configuration (empty old path) matches every Apache service.
pub(crate) fn apache_service_matches(
    service: &ServiceConfig,
    old_path: &str,
    old_root: &str,
) -> bool {
    if service.service_type.normalized() != ServiceType::Apache {
        return false;
    }
    if old_path.trim().is_empty() {
        return true;
    }
    if paths_equal(&service.executable, old_path) {
        return true;
    }
    if !old_root.trim().is_empty() {
        if path_is_under(&service.executable, old_root) {
            return true;
        }
        if paths_equal(&service.apache_root, old_root) {
            return true;
        }
    }
    false
}

fn affected_apps(config: &UpdaterConfig, services: &[ApacheServiceChange]) -> Vec<ApacheAppChange> {
    let names: Vec<&str> = services
        .iter()
        .map(|service| service.name.as_str())
        .collect();
    let mut seen = BTreeSet::new();
    let mut apps = Vec::new();
    for app in &config.apps {
        let web_server = app.web_server_service.trim();
        if web_server.is_empty() {
            continue;
        }
        if !names
            .iter()
            .any(|name| name.eq_ignore_ascii_case(web_server))
        {
            continue;
        }
        let key = app.id.to_ascii_lowercase();
        if !key.is_empty() && seen.insert(key) {
            apps.push(ApacheAppChange {
                id: app.id.clone(),
                name: app.name.clone(),
            });
        }
    }
    apps
}

/// Build a pending change when Apache settings actually affect services.
pub(crate) fn build_pending(
    old: &ApacheToolPath,
    new: &ApacheToolPath,
    services: &[ServiceConfig],
    config: &UpdaterConfig,
) -> Option<ApachePendingChange> {
    let new_path = new.path().trim();
    if new_path.is_empty() {
        // Clearing the optional Apache path must not write empty executables
        // into Apache services.
        return None;
    }
    if new.root.trim().is_empty() {
        // Apache services need a root; do not create a pending change that
        // would apply `-d ""` to SCM.
        return None;
    }

    let new_root = new.root.trim();
    let new_config = new.config.trim();
    let affected: Vec<ApacheServiceChange> = services
        .iter()
        .filter(|service| apache_service_matches(service, old.path(), &old.root))
        .filter(|service| {
            // A version-only change must not leave a pending service update
            // when every matched service already points at the new values.
            !paths_equal(&service.executable, new_path)
                || !paths_equal(&service.apache_root, new_root)
                || (!new_config.is_empty() && !paths_equal(&service.apache_config, new_config))
        })
        .map(|service| ApacheServiceChange {
            name: service.name.clone(),
            old_executable: service.executable.clone(),
            new_executable: new_path.to_string(),
            old_root: service.apache_root.clone(),
            new_root: new.root.trim().to_string(),
        })
        .collect();

    if affected.is_empty() {
        return None;
    }

    let apps = affected_apps(config, &affected);
    Some(ApachePendingChange {
        old_path: old.path().trim().to_string(),
        new_path: new_path.to_string(),
        old_root: old.root.trim().to_string(),
        new_root: new.root.trim().to_string(),
        old_config: old.config.trim().to_string(),
        new_config: new.config.trim().to_string(),
        old_version: old.tool.version.trim().to_string(),
        new_version: new.tool.version.trim().to_string(),
        services: affected,
        apps,
        created_at: now_iso(),
    })
}

/// Update only the services listed in the pending change.
pub(crate) fn update_apache_services(
    services: &[ServiceConfig],
    pending: &ApachePendingChange,
) -> Result<Vec<ServiceConfig>, String> {
    let mut updated = services.to_vec();
    let mut matched = 0usize;

    for change in &pending.services {
        let Some(service) = updated
            .iter_mut()
            .find(|service| service.name.eq_ignore_ascii_case(&change.name))
        else {
            return Err(format!(
                "Service '{}' from the pending Apache change no longer exists in services.json.",
                change.name
            ));
        };
        if service.service_type.normalized() != ServiceType::Apache {
            return Err(format!(
                "Service '{}' is no longer an Apache service.",
                change.name
            ));
        }
        let expected_exe = change.old_executable.trim();
        let expected_root = change.old_root.trim();
        if !expected_exe.is_empty()
            && !paths_equal(&service.executable, expected_exe)
            && !paths_equal(&service.apache_root, expected_root)
        {
            return Err(format!(
                "Service '{}' changed after the Apache preview. Save Settings again to refresh the preview.",
                change.name
            ));
        }

        service.executable = pending.new_path.trim().to_string();
        service.apache_root = pending.new_root.trim().to_string();
        if !pending.new_config.trim().is_empty() {
            service.apache_config = pending.new_config.trim().to_string();
        }
        service.normalize();
        matched += 1;
    }

    if matched == 0 {
        return Err("The pending Apache change has no matching services.".to_string());
    }
    Ok(updated)
}

fn save_updated_services(
    services_path: &Path,
    backup_dir: &Path,
    services: &[ServiceConfig],
    pending: &ApachePendingChange,
) -> Result<Vec<ServiceConfig>, String> {
    let updated = update_apache_services(services, pending)?;
    sm_config::save_services(services_path, backup_dir, &updated)
        .map_err(|error| format!("Failed to save services.json: {error}"))?;
    Ok(updated)
}

fn require_elevation(elevated: bool) -> Result<(), String> {
    if elevated {
        Ok(())
    } else {
        Err(
            "Administrator privileges are required to apply the pending Apache change. The change remains pending; restart Service Manager as Administrator and apply again."
                .to_string(),
        )
    }
}

fn service_running(name: &str) -> Result<bool, String> {
    match sm_win::query_status(name) {
        Ok(status) => Ok(status.state.is_running()),
        Err(sm_win::ScmError::NotFound(_)) => Err(format!(
            "Service '{name}' is not installed. Install it before applying Apache settings."
        )),
        Err(error) => Err(format!("Failed to query service '{name}': {error}")),
    }
}

fn clean_log_value(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn current_user() -> String {
    let user = env::var("USERNAME")
        .or_else(|_| env::var("USER"))
        .unwrap_or_else(|_| "unknown".to_string());
    let user = clean_log_value(&user);
    match env::var("USERDOMAIN") {
        Ok(domain) if !domain.trim().is_empty() => {
            format!("{}\\{}", clean_log_value(&domain), user)
        }
        _ => user,
    }
}

/// Append one line to `logs/apache-apply.log`. Values are control-character
/// sanitized so service names/paths cannot inject extra log lines.
pub(crate) fn append_audit(
    path: &Path,
    pending: &ApachePendingChange,
    result: &str,
    errors: &[String],
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let services = pending
        .services
        .iter()
        .map(|service| service.name.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let apps = pending
        .apps
        .iter()
        .map(|app| app.id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let errors = errors
        .iter()
        .map(|error| clean_log_value(error))
        .collect::<Vec<_>>()
        .join("; ");
    let line = format!(
        "timestamp={} user={} action=apply-apache result={} services={} apps={} oldPath={} newPath={} oldRoot={} newRoot={} oldConfig={} newConfig={} oldVersion={} newVersion={} createdAt={} errors={}",
        now_iso(),
        current_user(),
        clean_log_value(result),
        clean_log_value(&services),
        clean_log_value(&apps),
        clean_log_value(&pending.old_path),
        clean_log_value(&pending.new_path),
        clean_log_value(&pending.old_root),
        clean_log_value(&pending.new_root),
        clean_log_value(&pending.old_config),
        clean_log_value(&pending.new_config),
        clean_log_value(&pending.old_version),
        clean_log_value(&pending.new_version),
        clean_log_value(&pending.created_at),
        errors
    );

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| format!("Failed to open audit log {}: {error}", path.display()))?;
    writeln!(file, "{line}")
        .map_err(|error| format!("Failed to write audit log {}: {error}", path.display()))
}

fn rollback_apache_change(
    services_path: &Path,
    backup_dir: &Path,
    original: &[ServiceConfig],
    pending: &ApachePendingChange,
    nssm_path: &Path,
    applied: &[String],
    running_before: &BTreeMap<String, bool>,
) -> Vec<String> {
    let mut errors = Vec::new();
    if let Err(error) = sm_config::save_services(services_path, backup_dir, original) {
        errors.push(format!(
            "services.json rollback failed; the file may contain the new Apache values: {error}"
        ));
    }

    let runner = sm_win::SystemRunner;
    let options = WaitOptions::with_timeout(APPLY_TIMEOUT);
    for change in &pending.services {
        if applied
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&change.name))
        {
            if let Some(service) = original
                .iter()
                .find(|service| service.name.eq_ignore_ascii_case(&change.name))
            {
                if let Err(error) = sm_win::apply_service(service, nssm_path, &runner) {
                    errors.push(format!(
                        "Failed to re-apply the old config for service '{}': {error}",
                        change.name
                    ));
                }
            }
        }
        if running_before
            .get(&change.name.to_ascii_lowercase())
            .copied()
            .unwrap_or(false)
        {
            if let Err(error) = sm_win::start_service(&change.name, &options) {
                errors.push(format!(
                    "Failed to start service '{}' after rollback: {error}",
                    change.name
                ));
            }
        }
    }
    errors
}

/// Apply a pending Apache change. `elevated` is injectable for tests; the
/// Tauri command always passes a real `sm_win::is_elevated()` result.
pub(crate) fn apply_apache_change_at(
    paths: &ConfigPaths,
    elevated: bool,
) -> Result<ApacheApplyResultView, String> {
    let pending_path = pending_path_for(&paths.data_dir());
    let pending = load_pending_at(&pending_path)?.ok_or_else(|| {
        "No pending Apache change. Change Apache settings and click Save Settings first."
            .to_string()
    })?;
    require_elevation(elevated)?;

    let services_path = paths.services_path();
    let backup_dir = paths.backup_dir();
    let original = sm_config::load_services_strict(&services_path, &backup_dir)
        .map_err(|error| format!("Failed to read services.json: {error}"))?;
    let updated = save_updated_services(&services_path, &backup_dir, &original, &pending)?;

    let nssm_path = paths.nssm_path();
    let runner = sm_win::SystemRunner;
    let options = WaitOptions::with_timeout(APPLY_TIMEOUT);
    let mut running_before = BTreeMap::new();
    let mut applied = Vec::new();
    let mut failure = None;

    for change in &pending.services {
        let Some(target) = updated
            .iter()
            .find(|service| service.name.eq_ignore_ascii_case(&change.name))
        else {
            failure = Some(format!(
                "Service '{}' is missing from the updated services.json.",
                change.name
            ));
            break;
        };

        let was_running = match service_running(&target.name) {
            Ok(value) => value,
            Err(error) => {
                failure = Some(error);
                break;
            }
        };
        running_before.insert(target.name.to_ascii_lowercase(), was_running);

        if was_running {
            if let Err(error) = sm_win::stop_service(&target.name, &options) {
                failure = Some(format!("Failed to stop service '{}': {error}", target.name));
                break;
            }
        }

        if let Err(error) = sm_win::apply_service(target, &nssm_path, &runner) {
            failure = Some(format!(
                "Failed to apply service '{}': {error}",
                target.name
            ));
            break;
        }
        applied.push(target.name.clone());

        if was_running {
            if let Err(error) = sm_win::start_service(&target.name, &options) {
                failure = Some(format!(
                    "Failed to start service '{}' after apply: {error}",
                    target.name
                ));
                break;
            }
        }
    }

    if let Some(message) = failure {
        let mut errors = vec![message];
        errors.extend(rollback_apache_change(
            &services_path,
            &backup_dir,
            &original,
            &pending,
            &nssm_path,
            &applied,
            &running_before,
        ));
        let audit_path = paths.logs_dir().join(AUDIT_FILE);
        if let Err(error) = append_audit(&audit_path, &pending, "failure", &errors) {
            errors.push(format!("Audit log could not be written: {error}"));
        }
        return Err(format!(
            "Apache apply failed. The pending change was kept and services.json was restored.\n{}",
            errors.join("\n")
        ));
    }

    let audit_path = paths.logs_dir().join(AUDIT_FILE);
    let mut warnings = Vec::new();
    if let Err(error) = append_audit(&audit_path, &pending, "success", &[]) {
        warnings.push(format!(
            "Apache services were applied, but the audit log could not be written: {error}"
        ));
    }
    if let Err(error) = clear_pending_at(&pending_path) {
        warnings.push(format!(
            "Apache services were applied, but the pending file could not be cleared: {error}"
        ));
    }

    Ok(ApacheApplyResultView {
        success: true,
        services: pending
            .services
            .iter()
            .map(|service| service.name.clone())
            .collect(),
        errors: warnings,
        audit_path: audit_path.to_string_lossy().to_string(),
    })
}

#[tauri::command]
pub async fn apache_change_status() -> Result<Option<ApacheChangeView>, String> {
    let paths = ConfigPaths::detect();
    tauri::async_runtime::spawn_blocking(move || {
        load_pending_at(&pending_path_for(&paths.data_dir()))
            .map(|pending| pending.map(|p| p.view()))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn apply_apache_change() -> Result<ApacheApplyResultView, String> {
    let paths = ConfigPaths::detect();
    let elevated = sm_win::is_elevated()
        .map_err(|error| format!("Could not check Administrator privileges: {error}"))?;
    tauri::async_runtime::spawn_blocking(move || apply_apache_change_at(&paths, elevated))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use sm_config::ConfigPaths;
    use sm_updater::registry::AppConfig;

    fn apache_service(name: &str, executable: &str, root: &str) -> ServiceConfig {
        let mut service = ServiceConfig {
            name: name.to_string(),
            display_name: name.to_string(),
            service_type: ServiceType::Apache,
            executable: executable.to_string(),
            apache_root: root.to_string(),
            apache_config: "conf/httpd.conf".to_string(),
            ..ServiceConfig::default()
        };
        service.normalize();
        service
    }

    fn nssm_service(name: &str, executable: &str) -> ServiceConfig {
        let mut service = ServiceConfig {
            name: name.to_string(),
            executable: executable.to_string(),
            ..ServiceConfig::default()
        };
        service.normalize();
        service
    }

    fn sample_pending() -> ApachePendingChange {
        ApachePendingChange {
            old_path: r"C:\Apache24\bin\httpd.exe".into(),
            new_path: r"C:\Apache26\bin\httpd.exe".into(),
            old_root: r"C:\Apache24".into(),
            new_root: r"C:\Apache26".into(),
            old_config: "conf/httpd.conf".into(),
            new_config: "conf/httpd.conf".into(),
            old_version: "2.4.62".into(),
            new_version: "2.4.65".into(),
            services: vec![ApacheServiceChange {
                name: "Apache2.4".into(),
                old_executable: r"C:\Apache24\bin\httpd.exe".into(),
                new_executable: r"C:\Apache26\bin\httpd.exe".into(),
                old_root: r"C:\Apache24".into(),
                new_root: r"C:\Apache26".into(),
            }],
            apps: vec![ApacheAppChange {
                id: "kasirin".into(),
                name: "Kasirin".into(),
            }],
            created_at: "2026-11-16T10:00:00.000+07:00".into(),
        }
    }

    #[test]
    fn matches_apache_service_by_executable_or_root_or_apache_root() {
        let by_exe = apache_service("ApacheExe", r"C:\Apache24\bin\httpd.exe", r"C:\Apache24");
        assert!(apache_service_matches(
            &by_exe,
            "c:/apache24/BIN/HTTPD.EXE",
            ""
        ));

        let by_under_root =
            apache_service("ApacheRootExe", r"D:\Apache\bin\httpd.exe", r"E:\Other");
        assert!(apache_service_matches(
            &by_under_root,
            r"C:\Old\bin\httpd.exe",
            r"D:\Apache"
        ));

        let by_apache_root = apache_service("ApacheRoot", r"C:\Anywhere\httpd.exe", r"D:\Apache");
        assert!(apache_service_matches(
            &by_apache_root,
            r"C:\Old\bin\httpd.exe",
            r"D:\Apache"
        ));

        let not_inside = apache_service("NotInside", r"D:\Apache2\bin\httpd.exe", r"D:\Other");
        assert!(!apache_service_matches(
            &not_inside,
            r"C:\Old\bin\httpd.exe",
            r"D:\Apache"
        ));

        assert!(!apache_service_matches(
            &nssm_service("api", r"C:\Apache24\bin\api.exe"),
            r"C:\Apache24\bin\httpd.exe",
            r"C:\Apache24"
        ));
    }

    #[test]
    fn first_configuration_matches_all_apache_services_only() {
        let services = [
            apache_service("ApacheA", r"C:\A\bin\httpd.exe", r"C:\A"),
            apache_service("ApacheB", r"C:\B\bin\httpd.exe", r"C:\B"),
            nssm_service("api", r"C:\A\bin\api.exe"),
        ];
        let matching: Vec<&str> = services
            .iter()
            .filter(|service| apache_service_matches(service, "", ""))
            .map(|service| service.name.as_str())
            .collect();
        assert_eq!(matching, ["ApacheA", "ApacheB"]);
    }

    #[test]
    fn build_pending_filters_apps_by_web_server_service() {
        let services = vec![
            apache_service("Apache2.4", r"C:\Apache24\bin\httpd.exe", r"C:\Apache24"),
            nssm_service("api", r"C:\api\api.exe"),
        ];
        let config = UpdaterConfig {
            apps: vec![
                AppConfig {
                    id: "kasirin".into(),
                    name: "Kasirin".into(),
                    web_server_service: " apache2.4 ".into(),
                    ..AppConfig::default()
                },
                AppConfig {
                    id: "other".into(),
                    name: "Other".into(),
                    web_server_service: "ApacheOther".into(),
                    ..AppConfig::default()
                },
                AppConfig {
                    id: "none".into(),
                    name: "No web server".into(),
                    web_server_service: String::new(),
                    ..AppConfig::default()
                },
            ],
            ..UpdaterConfig::default()
        };
        let old = ApacheToolPath {
            tool: sm_updater::settings::ToolPath::new(r"C:\Apache24\bin\httpd.exe"),
            root: r"C:\Apache24".into(),
            config: "conf/httpd.conf".into(),
        };
        let new = ApacheToolPath {
            tool: sm_updater::settings::ToolPath::new(r"C:\Apache26\bin\httpd.exe"),
            root: r"C:\Apache26".into(),
            config: "conf/httpd.conf".into(),
        };

        let pending = build_pending(&old, &new, &services, &config).expect("pending");
        assert_eq!(pending.services.len(), 1);
        assert_eq!(pending.services[0].name, "Apache2.4");
        assert_eq!(pending.apps.len(), 1);
        assert_eq!(pending.apps[0].id, "kasirin");
    }

    #[test]
    fn pending_save_load_and_clear_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data/apache-pending.json");
        let pending = sample_pending();

        save_pending_at(&path, &pending).unwrap();
        let loaded = load_pending_at(&path).unwrap().unwrap();
        assert_eq!(loaded, pending);

        clear_pending_at(&path).unwrap();
        assert!(load_pending_at(&path).unwrap().is_none());
        clear_pending_at(&path).unwrap();
    }

    #[test]
    fn pending_rejects_control_characters() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data/apache-pending.json");
        let mut pending = sample_pending();
        pending.new_path = "C:\\Apache26\\bin\\httpd.exe\ninjected".into();

        let error = save_pending_at(&path, &pending).unwrap_err();
        assert!(error.contains("control characters"));
        assert!(!path.exists());
    }

    #[test]
    fn build_pending_ignores_services_already_at_new_values() {
        let services = vec![apache_service(
            "Apache2.4",
            r"C:\Apache24\bin\httpd.exe",
            r"C:\Apache24",
        )];
        let config = UpdaterConfig::default();
        let old = ApacheToolPath {
            tool: sm_updater::settings::ToolPath::new(""),
            root: String::new(),
            config: String::new(),
        };
        let mut new = ApacheToolPath {
            tool: sm_updater::settings::ToolPath::new(r"C:\Apache24\bin\httpd.exe"),
            root: r"C:\Apache24".into(),
            config: "conf/httpd.conf".into(),
        };
        new.tool.version = "2.4.63".into();

        assert!(build_pending(&old, &new, &services, &config).is_none());
    }

    #[test]
    fn build_pending_requires_apache_root() {
        let services = vec![apache_service(
            "Apache2.4",
            r"C:\Apache24\bin\httpd.exe",
            r"C:\Apache24",
        )];
        let config = UpdaterConfig::default();
        let old = ApacheToolPath {
            tool: sm_updater::settings::ToolPath::new(r"C:\Apache24\bin\httpd.exe"),
            root: r"C:\Apache24".into(),
            config: "conf/httpd.conf".into(),
        };
        let mut new = old.clone();
        new.tool.path = r"C:\Apache26\bin\httpd.exe".into();
        new.root.clear();
        assert!(build_pending(&old, &new, &services, &config).is_none());
    }

    #[test]
    fn apply_rejects_service_changed_after_preview() {
        let mut changed = apache_service("Apache2.4", r"C:\Apache25\bin\httpd.exe", r"C:\Apache25");
        changed.name = "Apache2.4".into();
        let services = vec![changed];
        let pending = sample_pending();

        let error = update_apache_services(&services, &pending).unwrap_err();
        assert!(error.contains("changed after the Apache preview"));
    }

    #[test]
    fn config_update_only_touches_affected_services() {
        let services = vec![
            apache_service("Apache2.4", r"C:\Apache24\bin\httpd.exe", r"C:\Apache24"),
            apache_service("ApacheLegacy", r"C:\Apache22\bin\httpd.exe", r"C:\Apache22"),
            nssm_service("api", r"C:\api\api.exe"),
        ];
        let mut pending = sample_pending();
        pending.services = vec![ApacheServiceChange {
            name: "Apache2.4".into(),
            old_executable: r"C:\Apache24\bin\httpd.exe".into(),
            new_executable: r"C:\Apache26\bin\httpd.exe".into(),
            old_root: r"C:\Apache24".into(),
            new_root: r"C:\Apache26".into(),
        }];
        pending.new_config.clear();

        let updated = update_apache_services(&services, &pending).unwrap();
        assert_eq!(updated[0].executable, r"C:\Apache26\bin\httpd.exe");
        assert_eq!(updated[0].apache_root, r"C:\Apache26");
        assert_eq!(
            updated[0].apache_config, "conf/httpd.conf",
            "empty newConfig must keep the existing config"
        );
        assert_eq!(updated[1], services[1]);
        assert_eq!(updated[2], services[2]);
    }

    #[test]
    fn config_update_uses_new_config_when_present() {
        let services = vec![apache_service(
            "Apache2.4",
            r"C:\Apache24\bin\httpd.exe",
            r"C:\Apache24",
        )];
        let mut pending = sample_pending();
        pending.new_config = "conf/extra/httpd-custom.conf".into();

        let updated = update_apache_services(&services, &pending).unwrap();
        assert_eq!(updated[0].apache_config, "conf/extra/httpd-custom.conf");
    }

    #[test]
    fn config_update_rejects_missing_service() {
        let services = vec![apache_service(
            "Apache2.4",
            r"C:\Apache24\bin\httpd.exe",
            r"C:\Apache24",
        )];
        let mut pending = sample_pending();
        pending.services[0].name = "Missing".into();

        let error = update_apache_services(&services, &pending).unwrap_err();
        assert!(error.contains("no longer exists"));
    }

    #[test]
    fn apply_without_elevation_is_rejected_and_keeps_pending() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path().join("home"));
        let data = paths.data_dir();
        fs::create_dir_all(&data).unwrap();
        let pending_path = pending_path_for(&data);
        save_pending_at(&pending_path, &sample_pending()).unwrap();

        let error = apply_apache_change_at(&paths, false).unwrap_err();
        assert!(error.contains("Administrator privileges"));
        assert!(load_pending_at(&pending_path).unwrap().is_some());
        assert!(!paths.services_path().exists());
    }

    #[test]
    fn save_updated_services_creates_backup_and_writes_only_new_apache_values() {
        let temp = tempfile::tempdir().unwrap();
        let services_path = temp.path().join("data/services.json");
        let backup_dir = temp.path().join("backup");
        let services = vec![
            apache_service("Apache2.4", r"C:\Apache24\bin\httpd.exe", r"C:\Apache24"),
            apache_service("ApacheLegacy", r"C:\Apache22\bin\httpd.exe", r"C:\Apache22"),
        ];
        sm_config::save_services(&services_path, &backup_dir, &services).unwrap();

        let mut pending = sample_pending();
        pending.services = vec![ApacheServiceChange {
            name: "Apache2.4".into(),
            old_executable: r"C:\Apache24\bin\httpd.exe".into(),
            new_executable: r"C:\Apache26\bin\httpd.exe".into(),
            old_root: r"C:\Apache24".into(),
            new_root: r"C:\Apache26".into(),
        }];
        let updated =
            save_updated_services(&services_path, &backup_dir, &services, &pending).unwrap();
        assert_eq!(updated[0].executable, r"C:\Apache26\bin\httpd.exe");
        assert_eq!(updated[1].executable, r"C:\Apache22\bin\httpd.exe");

        let backups: Vec<_> = fs::read_dir(&backup_dir)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("services-"))
            .collect();
        assert_eq!(backups.len(), 1);

        let stored = sm_config::load_services_strict(&services_path, &backup_dir).unwrap();
        assert_eq!(stored[0].executable, r"C:\Apache26\bin\httpd.exe");
        assert_eq!(stored[1].executable, r"C:\Apache22\bin\httpd.exe");
    }

    #[test]
    fn audit_log_contains_timestamp_user_action_and_old_new_values() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("logs/apache-apply.log");
        let pending = sample_pending();

        append_audit(&path, &pending, "success", &[]).unwrap();
        append_audit(&path, &pending, "failure", &["apply failed".into()]).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("timestamp="));
        assert!(lines[0].contains("user="));
        assert!(lines[0].contains("action=apply-apache"));
        assert!(lines[0].contains("result=success"));
        assert!(lines[0].contains("services=Apache2.4"));
        assert!(lines[0].contains(r"oldPath=C:\Apache24\bin\httpd.exe"));
        assert!(lines[0].contains(r"newPath=C:\Apache26\bin\httpd.exe"));
        assert!(lines[1].contains("result=failure"));
        assert!(lines[1].contains("errors=apply failed"));
    }

    #[test]
    fn elevation_helper_rejects_false_and_accepts_true() {
        assert!(require_elevation(true).is_ok());
        let error = require_elevation(false).unwrap_err();
        assert!(error.contains("Administrator privileges"));
        assert!(error.contains("remains pending"));
    }

    #[test]
    fn apache_settings_change_ignores_detected_at_only() {
        let mut old = ApacheToolPath {
            tool: sm_updater::settings::ToolPath::new(r"C:\Apache24\bin\httpd.exe"),
            root: r"C:\Apache24".into(),
            config: "conf/httpd.conf".into(),
        };
        let mut new = old.clone();
        new.tool.detected_at = "2026-11-16T10:00:00+07:00".into();
        assert!(!apache_settings_changed(&old, &new));

        old.tool.version = "2.4.62".into();
        new.tool.version = "2.4.65".into();
        assert!(apache_settings_changed(&old, &new));
    }
}
