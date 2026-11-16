//! Global machine settings (`data/settings.json`).
//!
//! `settings.json` is the authoritative source for tool paths and updater
//! defaults. When the file does not exist, callers can build an effective
//! setting from the deprecated `projects.json` `settings`/`trigger` fields via
//! [`from_legacy`] / [`effective_settings`]. Migration is never written
//! automatically.

use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sm_config::ConfigPaths;

use crate::registry::{TriggerSettings, UpdaterSettings};

pub const SETTINGS_VERSION: u32 = 1;
const MAX_TIMEOUT_SECONDS: u64 = 24 * 60 * 60;
const MAX_CONCURRENT_UPDATES: usize = 32;
const MAX_LOG_RETENTION_DAYS: u32 = 3650;

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("Gagal membaca/menulis settings: {0}")]
    Io(#[from] std::io::Error),
    #[error("Settings JSON tidak valid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}

fn invalid(message: impl Into<String>) -> SettingsError {
    SettingsError::Invalid(message.into())
}

fn default_version() -> u32 {
    SETTINGS_VERSION
}

fn default_true() -> bool {
    true
}

fn default_max_concurrent() -> usize {
    1
}

fn default_log_retention_days() -> u32 {
    30
}

fn default_allowed_ips() -> Vec<String> {
    vec!["127.0.0.1".to_string(), "::1".to_string()]
}

fn default_privileged_mode() -> String {
    "scheduled-task".to_string()
}

fn default_task_name() -> String {
    "ServiceManager-Updater".to_string()
}

fn default_logon_type() -> String {
    "Interactive".to_string()
}

fn normalize_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

fn has_control_characters(value: &str) -> bool {
    value.chars().any(char::is_control)
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    const SENSITIVE: [&str; 9] = [
        "password",
        "passwd",
        "secret",
        "token",
        "apikey",
        "accesskey",
        "privatekey",
        "credential",
        "authtoken",
    ];
    SENSITIVE.iter().any(|needle| {
        normalized == *needle || normalized.starts_with(needle) || normalized.ends_with(needle)
    })
}

fn is_empty_secret(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::String(value) => value.trim().is_empty(),
        Value::Array(values) => values.is_empty(),
        Value::Object(values) => values.is_empty(),
        _ => false,
    }
}

fn find_plaintext_secret(value: &Value, prefix: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                if is_sensitive_key(key) && !is_empty_secret(child) {
                    return Some(path);
                }
                if let Some(found) = find_plaintext_secret(child, &path) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                let path = format!("{prefix}[{index}]");
                if let Some(found) = find_plaintext_secret(child, &path) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

fn reject_plaintext_secrets(value: &Value) -> Result<(), SettingsError> {
    if let Some(path) = find_plaintext_secret(value, "") {
        return Err(invalid(format!(
            "Field rahasia '{path}' tidak boleh memuat password/token plaintext di settings.json."
        )));
    }
    Ok(())
}

/// Path satu tool di `settings.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPath {
    #[serde(default)]
    pub path: String,
    /// Diisi oleh deteksi tool, bukan user.
    #[serde(default)]
    pub version: String,
    /// Timestamp deteksi versi (ISO 8601), diisi oleh deteksi tool.
    #[serde(default)]
    pub detected_at: String,
}

impl ToolPath {
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            ..Self::default()
        }
    }

    pub fn is_configured(&self) -> bool {
        !self.path.trim().is_empty()
    }

    fn normalize(&mut self) {
        self.path = self.path.trim().to_string();
        self.version = self.version.trim().to_string();
        self.detected_at = self.detected_at.trim().to_string();
    }

    fn validate(&self, tool: &str, required: bool) -> Result<(), SettingsError> {
        let path = self.path.trim();
        if required && path.is_empty() {
            return Err(invalid(format!(
                "Path tool '{tool}' wajib diisi di settings.json."
            )));
        }
        if has_control_characters(path) {
            return Err(invalid(format!(
                "Path tool '{tool}' memuat karakter kontrol."
            )));
        }
        if self.version.len() > 128 || has_control_characters(&self.version) {
            return Err(invalid(format!(
                "Versi tool '{tool}' tidak valid (maksimal 128 karakter tanpa karakter kontrol)."
            )));
        }
        if self.detected_at.len() > 64 || has_control_characters(&self.detected_at) {
            return Err(invalid(format!(
                "Timestamp deteksi tool '{tool}' tidak valid."
            )));
        }
        Ok(())
    }
}

/// Apache memakai [`ToolPath`] ditambah root instalasi dan path config.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApacheToolPath {
    #[serde(flatten)]
    pub tool: ToolPath,
    #[serde(default)]
    pub root: String,
    /// Relatif terhadap `root` atau absolut.
    #[serde(default)]
    pub config: String,
}

impl ApacheToolPath {
    pub fn path(&self) -> &str {
        &self.tool.path
    }

    fn normalize(&mut self) {
        self.tool.normalize();
        self.root = self.root.trim().to_string();
        self.config = self.config.trim().to_string();
    }

    fn validate(&self) -> Result<(), SettingsError> {
        self.tool.validate("apache", false)?;
        if has_control_characters(&self.root) || has_control_characters(&self.config) {
            return Err(invalid("Root/config Apache memuat karakter kontrol."));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tools {
    #[serde(default)]
    pub git: ToolPath,
    #[serde(default)]
    pub php: ToolPath,
    #[serde(default)]
    pub composer: ToolPath,
    #[serde(default)]
    pub node: ToolPath,
    #[serde(default)]
    pub npm: ToolPath,
    #[serde(default)]
    pub pnpm: ToolPath,
    #[serde(default)]
    pub yarn: ToolPath,
    #[serde(default)]
    pub bun: ToolPath,
    #[serde(default)]
    pub go: ToolPath,
    #[serde(default)]
    pub apache: ApacheToolPath,
    #[serde(default)]
    pub nssm: ToolPath,
}

impl Default for Tools {
    fn default() -> Self {
        let defaults = UpdaterSettings::default();
        Self {
            git: ToolPath::new(defaults.git_path),
            php: ToolPath::new(defaults.php_path),
            composer: ToolPath::new(defaults.composer_path),
            node: ToolPath::default(),
            npm: ToolPath::new(defaults.npm_path),
            pnpm: ToolPath::new(defaults.pnpm_path),
            yarn: ToolPath::new(defaults.yarn_path),
            bun: ToolPath::new(defaults.bun_path),
            go: ToolPath::new(defaults.go_path),
            apache: ApacheToolPath::default(),
            nssm: ToolPath::default(),
        }
    }
}

impl Tools {
    fn normalize(&mut self) {
        for tool in [
            &mut self.git,
            &mut self.php,
            &mut self.composer,
            &mut self.node,
            &mut self.npm,
            &mut self.pnpm,
            &mut self.yarn,
            &mut self.bun,
            &mut self.go,
            &mut self.nssm,
        ] {
            tool.normalize();
        }
        self.apache.normalize();
    }

    fn validate(&self) -> Result<(), SettingsError> {
        for (name, tool) in [
            ("git", &self.git),
            ("php", &self.php),
            ("composer", &self.composer),
            ("npm", &self.npm),
        ] {
            tool.validate(name, true)?;
        }
        for (name, tool) in [
            ("node", &self.node),
            ("pnpm", &self.pnpm),
            ("yarn", &self.yarn),
            ("bun", &self.bun),
            ("go", &self.go),
            ("nssm", &self.nssm),
        ] {
            tool.validate(name, false)?;
        }
        self.apache.validate()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateTimeouts {
    #[serde(default = "default_preflight_seconds")]
    pub preflight_seconds: u64,
    #[serde(default = "default_fetch_seconds")]
    pub fetch_seconds: u64,
    #[serde(default = "default_tool_seconds")]
    pub tool_seconds: u64,
    #[serde(default = "default_clone_seconds")]
    pub clone_seconds: u64,
}

fn default_preflight_seconds() -> u64 {
    30
}

fn default_fetch_seconds() -> u64 {
    120
}

fn default_tool_seconds() -> u64 {
    300
}

fn default_clone_seconds() -> u64 {
    600
}

impl Default for UpdateTimeouts {
    fn default() -> Self {
        Self {
            preflight_seconds: default_preflight_seconds(),
            fetch_seconds: default_fetch_seconds(),
            tool_seconds: default_tool_seconds(),
            clone_seconds: default_clone_seconds(),
        }
    }
}

impl UpdateTimeouts {
    fn validate(&self) -> Result<(), SettingsError> {
        for (name, seconds) in [
            ("preflightSeconds", self.preflight_seconds),
            ("fetchSeconds", self.fetch_seconds),
            ("toolSeconds", self.tool_seconds),
            ("cloneSeconds", self.clone_seconds),
        ] {
            if seconds == 0 || seconds > MAX_TIMEOUT_SECONDS {
                return Err(invalid(format!(
                    "Timeout '{name}' harus antara 1 dan {MAX_TIMEOUT_SECONDS} detik."
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterDefaults {
    /// Kosong berarti memakai default `logs/` di samping file config.
    #[serde(default)]
    pub log_dir: String,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_updates: usize,
    #[serde(default)]
    pub timeouts: UpdateTimeouts,
    #[serde(default = "default_log_retention_days")]
    pub log_retention_days: u32,
}

impl Default for UpdaterDefaults {
    fn default() -> Self {
        Self {
            log_dir: String::new(),
            max_concurrent_updates: default_max_concurrent(),
            timeouts: UpdateTimeouts::default(),
            log_retention_days: default_log_retention_days(),
        }
    }
}

impl UpdaterDefaults {
    fn validate(&self) -> Result<(), SettingsError> {
        if self.max_concurrent_updates == 0 || self.max_concurrent_updates > MAX_CONCURRENT_UPDATES
        {
            return Err(invalid(format!(
                "maxConcurrentUpdates harus antara 1 dan {MAX_CONCURRENT_UPDATES}."
            )));
        }
        self.timeouts.validate()?;
        if self.log_retention_days > MAX_LOG_RETENTION_DAYS {
            return Err(invalid(format!(
                "logRetentionDays maksimal {MAX_LOG_RETENTION_DAYS} hari."
            )));
        }
        if has_control_characters(&self.log_dir) {
            return Err(invalid("logDir memuat karakter kontrol."));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsTrigger {
    #[serde(
        default = "default_allowed_ips",
        rename = "allowedClientIPs",
        alias = "allowedClientIps"
    )]
    pub allowed_client_ips: Vec<String>,
    #[serde(default = "default_true")]
    pub allow_local: bool,
}

impl Default for SettingsTrigger {
    fn default() -> Self {
        Self {
            allowed_client_ips: default_allowed_ips(),
            allow_local: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivilegedExecution {
    #[serde(default = "default_privileged_mode")]
    pub mode: String,
    #[serde(default = "default_task_name")]
    pub task_name: String,
    /// User Scheduled Task. Bukan password; jangan pernah isi password di sini.
    #[serde(default)]
    pub user_id: String,
    #[serde(default = "default_logon_type")]
    pub logon_type: String,
}

impl Default for PrivilegedExecution {
    fn default() -> Self {
        Self {
            mode: default_privileged_mode(),
            task_name: default_task_name(),
            user_id: String::new(),
            logon_type: default_logon_type(),
        }
    }
}

impl PrivilegedExecution {
    fn normalize(&mut self) {
        self.mode = self.mode.trim().to_string();
        self.task_name = self.task_name.trim().to_string();
        self.user_id = self.user_id.trim().to_string();
        self.logon_type = self.logon_type.trim().to_string();
    }

    fn validate(&self) -> Result<(), SettingsError> {
        if self.mode.is_empty() {
            return Err(invalid("privilegedExecution.mode wajib diisi."));
        }
        if self.mode == "scheduled-task" && self.task_name.is_empty() {
            return Err(invalid(
                "privilegedExecution.taskName wajib diisi untuk mode scheduled-task.",
            ));
        }
        if self.logon_type.is_empty() {
            return Err(invalid("privilegedExecution.logonType wajib diisi."));
        }
        Ok(())
    }
}

/// Isi `data/settings.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub tools: Tools,
    #[serde(default)]
    pub updater: UpdaterDefaults,
    #[serde(default)]
    pub trigger: SettingsTrigger,
    #[serde(default)]
    pub privileged_execution: PrivilegedExecution,
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            tools: Tools::default(),
            updater: UpdaterDefaults::default(),
            trigger: SettingsTrigger::default(),
            privileged_execution: PrivilegedExecution::default(),
            variables: BTreeMap::new(),
        }
    }
}

impl Settings {
    pub fn normalize(&mut self) {
        self.tools.normalize();
        self.updater.log_dir = self.updater.log_dir.trim().to_string();
        self.trigger.allowed_client_ips = normalize_list(&self.trigger.allowed_client_ips);
        self.privileged_execution.normalize();
        self.variables = std::mem::take(&mut self.variables)
            .into_iter()
            .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
            .filter(|(key, _)| !key.is_empty())
            .collect();
    }

    pub fn validate(&self) -> Result<(), SettingsError> {
        if self.version != SETTINGS_VERSION {
            return Err(invalid(format!(
                "Versi settings {} tidak didukung (versi yang didukung: {SETTINGS_VERSION}).",
                self.version
            )));
        }
        self.tools.validate()?;
        self.updater.validate()?;
        self.privileged_execution.validate()?;
        for key in self.variables.keys() {
            if is_sensitive_key(key) {
                return Err(invalid(format!(
                    "Variable '{key}' terlihat menyimpan password/token; settings.json tidak boleh memuat rahasia plaintext."
                )));
            }
        }
        Ok(())
    }

    /// Adapter agar kode updater/toolchain lama tetap memakai [`UpdaterSettings`].
    pub fn to_updater_settings(&self) -> UpdaterSettings {
        UpdaterSettings {
            git_path: self.tools.git.path.clone(),
            php_path: self.tools.php.path.clone(),
            composer_path: self.tools.composer.path.clone(),
            npm_path: self.tools.npm.path.clone(),
            bun_path: self.tools.bun.path.clone(),
            pnpm_path: self.tools.pnpm.path.clone(),
            yarn_path: self.tools.yarn.path.clone(),
            go_path: self.tools.go.path.clone(),
            log_dir: self.updater.log_dir.clone(),
            max_concurrent_updates: self.updater.max_concurrent_updates,
        }
    }
}

/// Default: `<folder config>/data/settings.json`.
pub fn settings_path() -> PathBuf {
    ConfigPaths::detect().data_dir().join("settings.json")
}

/// `settings.json` adalah sibling `projects.json` di folder `data/`.
pub fn settings_path_for(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("settings.json")
}

/// Membaca settings dari path. `Ok(None)` jika file tidak ada/kosong.
pub fn load_settings(path: &Path) -> Result<Option<Settings>, SettingsError> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if raw.trim().is_empty() {
        return Ok(None);
    }

    let value: Value = serde_json::from_str(&raw)?;
    reject_plaintext_secrets(&value)?;
    let mut settings: Settings = serde_json::from_value(value)?;
    settings.normalize();
    settings.validate()?;
    Ok(Some(settings))
}

/// Menyimpan settings secara atomik (temporary file + rename).
pub fn save_settings(path: &Path, settings: &Settings) -> Result<(), SettingsError> {
    let mut settings = settings.clone();
    settings.normalize();
    settings.validate()?;
    let value = serde_json::to_value(&settings)?;
    reject_plaintext_secrets(&value)?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&settings)?;
    let temp_path = path.with_extension(format!("json.tmp-{}", std::process::id()));
    fs::write(&temp_path, json)?;
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    Ok(())
}

pub fn load_default_settings() -> Result<Option<Settings>, SettingsError> {
    load_settings(&settings_path())
}

pub fn save_default_settings(settings: &Settings) -> Result<(), SettingsError> {
    save_settings(&settings_path(), settings)
}

/// Migrasi di memori dari `projects.json` `settings`/`trigger` lama.
///
/// Tidak menulis file apa pun. Field lama yang tidak punya pasangan di
/// `settings.json` (misalnya node/apache/nssm) dibiarkan kosong.
pub fn from_legacy(settings: &UpdaterSettings, trigger: &TriggerSettings) -> Settings {
    let mut value = Settings::default();
    value.tools.git = ToolPath::new(settings.git_path.clone());
    value.tools.php = ToolPath::new(settings.php_path.clone());
    value.tools.composer = ToolPath::new(settings.composer_path.clone());
    value.tools.npm = ToolPath::new(settings.npm_path.clone());
    value.tools.pnpm = ToolPath::new(settings.pnpm_path.clone());
    value.tools.yarn = ToolPath::new(settings.yarn_path.clone());
    value.tools.bun = ToolPath::new(settings.bun_path.clone());
    value.tools.go = ToolPath::new(settings.go_path.clone());
    value.updater.log_dir = settings.log_dir.clone();
    value.updater.max_concurrent_updates = settings.max_concurrent_updates;
    value.trigger.allowed_client_ips = trigger.allowed_client_ips.clone();
    value.trigger.allow_local = trigger.allow_local;
    value.normalize();
    value
}

/// Settings efektif: `settings.json` jika ada, selain itu fallback legacy.
///
/// Fallback tidak divalidasi ketat karena config legacy boleh tidak lengkap.
pub fn effective_settings(
    settings_path: &Path,
    legacy_settings: &UpdaterSettings,
    legacy_trigger: &TriggerSettings,
) -> Result<Settings, SettingsError> {
    match load_settings(settings_path)? {
        Some(settings) => Ok(settings),
        None => Ok(from_legacy(legacy_settings, legacy_trigger)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_settings() -> Settings {
        Settings {
            tools: Tools {
                git: ToolPath::new(r"C:\tools\git.exe"),
                php: ToolPath::new(r"C:\tools\php.exe"),
                composer: ToolPath::new(r"C:\tools\composer.bat"),
                npm: ToolPath::new(r"C:\tools\npm.cmd"),
                ..Tools::default()
            },
            ..Settings::default()
        }
    }

    fn legacy_settings() -> UpdaterSettings {
        UpdaterSettings {
            git_path: r"C:\legacy\git.exe".into(),
            php_path: r"C:\legacy\php.exe".into(),
            composer_path: r"C:\legacy\composer.bat".into(),
            npm_path: r"C:\legacy\npm.cmd".into(),
            bun_path: r"C:\legacy\bun.exe".into(),
            pnpm_path: r"C:\legacy\pnpm.cmd".into(),
            yarn_path: r"C:\legacy\yarn.cmd".into(),
            go_path: r"C:\legacy\go.exe".into(),
            log_dir: r"C:\legacy\logs".into(),
            max_concurrent_updates: 2,
        }
    }

    fn legacy_trigger() -> TriggerSettings {
        TriggerSettings {
            allowed_client_ips: vec!["10.0.0.1".into(), "127.0.0.1".into()],
            allow_local: false,
        }
    }

    #[test]
    fn roundtrip_settings_through_disk() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data/settings.json");
        let mut settings = base_settings();
        settings.tools.php.version = "8.3.16".into();
        settings.tools.php.detected_at = "2026-11-16T16:00:00+07:00".into();
        settings.tools.apache = ApacheToolPath {
            tool: ToolPath::new(r"C:\apache\bin\httpd.exe"),
            root: r"C:\apache".into(),
            config: "conf/httpd.conf".into(),
        };
        settings.updater.log_dir = r"C:\logs\updater".into();
        settings.updater.max_concurrent_updates = 3;
        settings
            .variables
            .insert("PROJECT_ROOT".into(), r"D:\Projects".into());

        save_settings(&path, &settings).unwrap();
        let loaded = load_settings(&path).unwrap().unwrap();

        assert_eq!(loaded, settings);
        assert_eq!(loaded.tools.apache.root, r"C:\apache");
        assert!(!path.with_extension("json.tmp-0").exists());

        let mut updated = loaded;
        updated.updater.log_retention_days = 7;
        save_settings(&path, &updated).unwrap();
        assert_eq!(
            load_settings(&path)
                .unwrap()
                .unwrap()
                .updater
                .log_retention_days,
            7
        );
    }

    #[test]
    fn missing_settings_file_is_none() {
        let temp = tempfile::tempdir().unwrap();
        assert!(load_settings(&temp.path().join("settings.json"))
            .unwrap()
            .is_none());
    }

    #[test]
    fn migrates_legacy_settings_and_trigger_in_memory() {
        let legacy = legacy_settings();
        let trigger = legacy_trigger();
        let settings = from_legacy(&legacy, &trigger);

        assert_eq!(settings.version, SETTINGS_VERSION);
        assert_eq!(settings.tools.git.path, r"C:\legacy\git.exe");
        assert_eq!(settings.tools.php.path, r"C:\legacy\php.exe");
        assert_eq!(settings.tools.composer.path, r"C:\legacy\composer.bat");
        assert_eq!(settings.tools.npm.path, r"C:\legacy\npm.cmd");
        assert_eq!(settings.tools.bun.path, r"C:\legacy\bun.exe");
        assert_eq!(settings.tools.pnpm.path, r"C:\legacy\pnpm.cmd");
        assert_eq!(settings.tools.yarn.path, r"C:\legacy\yarn.cmd");
        assert_eq!(settings.tools.go.path, r"C:\legacy\go.exe");
        assert!(settings.tools.apache.path().is_empty());
        assert_eq!(settings.updater.log_dir, r"C:\legacy\logs");
        assert_eq!(settings.updater.max_concurrent_updates, 2);
        assert_eq!(
            settings.trigger.allowed_client_ips,
            ["10.0.0.1", "127.0.0.1"]
        );
        assert!(!settings.trigger.allow_local);
        assert_eq!(settings.to_updater_settings(), legacy);
    }

    #[test]
    fn effective_settings_prefers_settings_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data/settings.json");
        let mut settings = base_settings();
        settings.tools.git.path = r"C:\settings\git.exe".into();
        settings.updater.log_dir = r"C:\settings\logs".into();
        save_settings(&path, &settings).unwrap();

        let effective = effective_settings(&path, &legacy_settings(), &legacy_trigger()).unwrap();

        assert_eq!(effective.tools.git.path, r"C:\settings\git.exe");
        assert_eq!(effective.updater.log_dir, r"C:\settings\logs");
    }

    #[test]
    fn effective_settings_falls_back_to_legacy() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data/settings.json");
        let effective = effective_settings(&path, &legacy_settings(), &legacy_trigger()).unwrap();

        assert_eq!(effective.tools.git.path, r"C:\legacy\git.exe");
        assert_eq!(effective.updater.log_dir, r"C:\legacy\logs");
        assert!(!path.exists(), "fallback must not write settings.json");
    }

    #[test]
    fn validation_rejects_empty_required_tool_paths() {
        let mut settings = base_settings();
        settings.tools.git.path.clear();
        let error = settings.validate().unwrap_err().to_string();
        assert!(error.contains("git"));

        for tool in [
            &mut settings.tools.php,
            &mut settings.tools.composer,
            &mut settings.tools.npm,
        ] {
            tool.path.clear();
        }
        assert!(settings.validate().is_err());

        settings.tools.php.path = r"C:\tools\php.exe".into();
        settings.tools.composer.path = r"C:\tools\composer.bat".into();
        settings.tools.npm.path = r"C:\tools\npm.cmd".into();
    }

    #[test]
    fn validation_allows_empty_optional_tool_paths() {
        let mut settings = base_settings();
        settings.tools.pnpm.path.clear();
        settings.tools.yarn.path.clear();
        settings.tools.bun.path.clear();
        settings.tools.go.path.clear();
        settings.tools.nssm.path.clear();
        settings.tools.apache.tool.path.clear();
        settings.validate().unwrap();
    }

    #[test]
    fn validation_rejects_bad_version_timeouts_and_concurrency() {
        let mut settings = base_settings();
        settings.version = 0;
        assert!(settings.validate().is_err());
        settings.version = SETTINGS_VERSION;

        settings.updater.timeouts.fetch_seconds = 0;
        assert!(settings.validate().is_err());
        settings.updater.timeouts.fetch_seconds = default_fetch_seconds();

        settings.updater.max_concurrent_updates = 0;
        assert!(settings.validate().is_err());
        settings.updater.max_concurrent_updates = 1;

        settings.tools.php.version = "8.3.16\ninjected".into();
        assert!(settings.validate().is_err());
    }

    #[test]
    fn validation_rejects_sensitive_variables() {
        let mut settings = base_settings();
        settings
            .variables
            .insert("DB_PASSWORD".into(), "hunter2".into());
        let error = settings.validate().unwrap_err().to_string();
        assert!(error.to_lowercase().contains("password"));
    }

    #[test]
    fn loading_rejects_plaintext_password_field() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        fs::write(&path, r#"{"version":1,"password":"hunter2"}"#).unwrap();

        let error = load_settings(&path).unwrap_err().to_string();
        assert!(error.to_ascii_lowercase().contains("password"));
        assert!(error.contains("plaintext"));
    }

    #[test]
    fn saving_rejects_secret_field_before_writing() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("settings.json");
        let mut settings = base_settings();
        settings
            .variables
            .insert("GITHUB_TOKEN".into(), "ghp_example".into());

        assert!(save_settings(&path, &settings).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn to_updater_settings_maps_paths_and_defaults() {
        let mut settings = base_settings();
        settings.tools.bun.path = r"C:\tools\bun.exe".into();
        settings.tools.go.path = r"C:\tools\go.exe".into();
        settings.tools.pnpm.path = r"C:\tools\pnpm.cmd".into();
        settings.tools.yarn.path = r"C:\tools\yarn.cmd".into();
        settings.updater.log_dir = r"C:\logs".into();
        settings.updater.max_concurrent_updates = 4;

        let updater = settings.to_updater_settings();
        assert_eq!(updater.git_path, r"C:\tools\git.exe");
        assert_eq!(updater.php_path, r"C:\tools\php.exe");
        assert_eq!(updater.composer_path, r"C:\tools\composer.bat");
        assert_eq!(updater.npm_path, r"C:\tools\npm.cmd");
        assert_eq!(updater.bun_path, r"C:\tools\bun.exe");
        assert_eq!(updater.pnpm_path, r"C:\tools\pnpm.cmd");
        assert_eq!(updater.yarn_path, r"C:\tools\yarn.cmd");
        assert_eq!(updater.go_path, r"C:\tools\go.exe");
        assert_eq!(updater.log_dir, r"C:\logs");
        assert_eq!(updater.max_concurrent_updates, 4);
    }

    #[test]
    fn example_settings_file_is_valid() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../settings.example.json");
        let settings = load_settings(&path)
            .expect("settings.example.json must parse")
            .expect("settings.example.json must exist");
        assert_eq!(settings.version, SETTINGS_VERSION);
    }
}
