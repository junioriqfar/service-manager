use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sm_config::ProjectsFile;
use sm_core::{ProjectOverride, ToggleMode};

fn default_version() -> u32 {
    2
}

fn default_branch() -> String {
    "main".to_string()
}

fn default_wait_timeout() -> u64 {
    60
}

fn default_command_timeout() -> u64 {
    300
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Preset {
    #[default]
    Auto,
    Laravel,
    Node,
    Bun,
    Go,
    Static,
    Custom,
    #[serde(other)]
    Unknown,
}

impl Preset {
    pub fn normalized(self) -> Self {
        match self {
            Self::Unknown => Self::Auto,
            other => other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageManager {
    #[default]
    Auto,
    Npm,
    Pnpm,
    Yarn,
    Bun,
    None,
    #[serde(other)]
    Unknown,
}

impl PackageManager {
    pub fn normalized(self) -> Self {
        match self {
            Self::Unknown => Self::Auto,
            other => other,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicePlan {
    #[serde(default)]
    pub stop_before_pull: Vec<String>,
    #[serde(default)]
    pub stop_before_build: Vec<String>,
    #[serde(default)]
    pub restart_after: Vec<String>,
    #[serde(default)]
    pub start_after: Vec<String>,
    #[serde(default = "default_wait_timeout")]
    pub wait_timeout_seconds: u64,
}

impl ServicePlan {
    pub fn normalize(&mut self) {
        self.stop_before_pull = normalize_list(&self.stop_before_pull);
        self.stop_before_build = normalize_list(&self.stop_before_build);
        self.restart_after = normalize_list(&self.restart_after);
        self.start_after = normalize_list(&self.start_after);
        if self.wait_timeout_seconds == 0 {
            self.wait_timeout_seconds = default_wait_timeout();
        }
    }
}

fn normalize_list(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandSpec {
    #[serde(default)]
    pub name: String,
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub cwd: String,
    #[serde(default = "default_command_timeout")]
    pub timeout_seconds: u64,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub continue_on_error: bool,
    #[serde(default)]
    pub shell: String,
}

impl CommandSpec {
    pub fn normalize(&mut self) {
        self.name = self.name.trim().to_string();
        self.program = self.program.trim().to_string();
        self.cwd = self.cwd.trim().to_string();
        self.shell = self.shell.trim().to_string();
        self.args = self.args.iter().map(|value| value.to_string()).collect();
        if self.timeout_seconds == 0 {
            self.timeout_seconds = default_command_timeout();
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomCommands {
    #[serde(default)]
    pub install: Vec<CommandSpec>,
    #[serde(default)]
    pub build: Vec<CommandSpec>,
    #[serde(default)]
    pub pre_update: Vec<CommandSpec>,
    #[serde(default)]
    pub post_update: Vec<CommandSpec>,
}

impl CustomCommands {
    pub fn normalize(&mut self) {
        for group in [
            &mut self.install,
            &mut self.build,
            &mut self.pre_update,
            &mut self.post_update,
        ] {
            for command in group.iter_mut() {
                command.normalize();
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthCheck {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub url: String,
    #[serde(default = "default_wait_timeout")]
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotifyConfig {
    #[serde(default)]
    pub webhook_url: String,
    #[serde(default)]
    pub on: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub repo_url: String,
    #[serde(default)]
    pub local_path: String,
    #[serde(default = "default_branch")]
    pub branch: String,
    #[serde(default)]
    pub credential_profile: String,
    #[serde(default, rename = "allowedClientIPs", alias = "allowedClientIps")]
    pub allowed_client_ips: Vec<String>,
    /// Pola repo-relative untuk artifact build yang boleh diabaikan saat
    /// memeriksa working tree. Entri lain tetap memblokir update.
    #[serde(default)]
    pub dirty_ignore: Vec<String>,
    /// Nama service Apache di `services.json` yang menyajikan app ini.
    /// Kosong berarti tidak terhubung eksplisit ke web server.
    #[serde(default)]
    pub web_server_service: String,
    #[serde(default)]
    pub clone_on_first_run: bool,
    #[serde(default)]
    pub preset: Preset,
    #[serde(default)]
    pub composer: ToggleMode,
    #[serde(default)]
    pub artisan: ToggleMode,
    #[serde(default)]
    pub package_manager: PackageManager,
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
    pub migration_policy: String,
    #[serde(default)]
    pub pre_update_command: String,
    #[serde(default)]
    pub post_update_command: String,
    #[serde(default)]
    pub services: ServicePlan,
    #[serde(default)]
    pub commands: CustomCommands,
    #[serde(default)]
    pub health_check: HealthCheck,
    #[serde(default)]
    pub notify: NotifyConfig,
}

fn default_true() -> bool {
    true
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            enabled: true,
            repo_url: String::new(),
            local_path: String::new(),
            branch: default_branch(),
            credential_profile: String::new(),
            allowed_client_ips: Vec::new(),
            dirty_ignore: Vec::new(),
            web_server_service: String::new(),
            clone_on_first_run: false,
            preset: Preset::Auto,
            composer: ToggleMode::Auto,
            artisan: ToggleMode::Auto,
            package_manager: PackageManager::Auto,
            package_install: ToggleMode::Auto,
            build: ToggleMode::Auto,
            install_command: String::new(),
            build_command: String::new(),
            queue_restart: true,
            migration_policy: "block".to_string(),
            pre_update_command: String::new(),
            post_update_command: String::new(),
            services: ServicePlan::default(),
            commands: CustomCommands::default(),
            health_check: HealthCheck::default(),
            notify: NotifyConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn normalize(&mut self) {
        self.id = self.id.trim().to_string();
        self.name = self.name.trim().to_string();
        self.repo_url = self.repo_url.trim().to_string();
        self.local_path = self.local_path.trim().to_string();
        self.branch = self.branch.trim().to_string();
        self.credential_profile = self.credential_profile.trim().to_string();
        self.allowed_client_ips = normalize_list(&self.allowed_client_ips);
        self.dirty_ignore = normalize_list(&self.dirty_ignore);
        self.web_server_service = self.web_server_service.trim().to_string();
        self.install_command = self.install_command.trim().to_string();
        self.build_command = self.build_command.trim().to_string();
        self.pre_update_command = self.pre_update_command.trim().to_string();
        self.post_update_command = self.post_update_command.trim().to_string();
        self.migration_policy = self.migration_policy.trim().to_lowercase();
        if self.name.is_empty() {
            self.name = self.id.clone();
        }
        if self.branch.is_empty() {
            self.branch = default_branch();
        }
        if self.migration_policy.is_empty() {
            self.migration_policy = "block".to_string();
        }
        self.preset = self.preset.normalized();
        self.package_manager = self.package_manager.normalized();
        self.services.normalize();
        self.commands.normalize();
        if self.health_check.timeout_seconds == 0 {
            self.health_check.timeout_seconds = default_wait_timeout();
        }
    }

    pub fn to_project_override(&self) -> ProjectOverride {
        ProjectOverride {
            path: self.local_path.clone(),
            enabled: self.enabled,
            branch: self.branch.clone(),
            composer: self.composer,
            build: self.build,
            queue_restart: self.queue_restart,
            post_update_command: self.post_update_command.clone(),
        }
    }

    pub fn to_project_info(&self) -> crate::scan::ProjectInfo {
        crate::scan::ProjectInfo {
            name: self.name.clone(),
            path: PathBuf::from(&self.local_path),
            config: self.to_project_override(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterSettings {
    #[serde(default)]
    pub git_path: String,
    #[serde(default)]
    pub php_path: String,
    #[serde(default)]
    pub composer_path: String,
    #[serde(default)]
    pub npm_path: String,
    #[serde(default)]
    pub bun_path: String,
    #[serde(default)]
    pub pnpm_path: String,
    #[serde(default)]
    pub yarn_path: String,
    #[serde(default)]
    pub go_path: String,
    #[serde(default)]
    pub log_dir: String,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_updates: usize,
    /// Built-in tool variables (`PHP`, `PHP_HOME`, `GIT`, ...) plus admin
    /// variables from `settings.json.variables`. Used by command expansion.
    #[serde(default)]
    pub variables: BTreeMap<String, String>,
}

fn default_max_concurrent() -> usize {
    1
}

/// Parent folder of a tool executable. Empty path or bare file name yields
/// an empty value rather than the current directory.
pub fn tool_home(path: &str) -> String {
    let path = path.trim();
    if path.is_empty() {
        return String::new();
    }
    Path::new(path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(|parent| parent.to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Build the variable table shared by `settings.json`, the legacy fallback,
/// and `UpdaterSettings::default()`.
///
/// Built-in names win over admin variables with the same name so `${PHP}`
/// always points at the configured PHP executable.
#[allow(clippy::too_many_arguments)]
pub fn build_tool_variables(
    git_path: &str,
    php_path: &str,
    composer_path: &str,
    npm_path: &str,
    bun_path: &str,
    go_path: &str,
    apache_path: &str,
    apache_root: &str,
    apache_config: &str,
    custom: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut variables = custom.clone();
    variables.insert("PHP".to_string(), php_path.trim().to_string());
    variables.insert("PHP_HOME".to_string(), tool_home(php_path));
    variables.insert("GIT".to_string(), git_path.trim().to_string());
    variables.insert("COMPOSER".to_string(), composer_path.trim().to_string());
    variables.insert("NPM".to_string(), npm_path.trim().to_string());
    variables.insert("BUN".to_string(), bun_path.trim().to_string());
    variables.insert("GO".to_string(), go_path.trim().to_string());
    variables.insert("APACHE".to_string(), apache_path.trim().to_string());
    variables.insert("APACHE_HOME".to_string(), apache_root.trim().to_string());
    variables.insert(
        "APACHE_CONFIG".to_string(),
        apache_config.trim().to_string(),
    );
    variables
}

impl UpdaterSettings {
    /// Recompute built-in variables after paths changed programmatically.
    /// Admin variables are preserved.
    pub fn refresh_variables(&mut self) {
        let custom = self.variables.clone();
        self.variables = build_tool_variables(
            &self.git_path,
            &self.php_path,
            &self.composer_path,
            &self.npm_path,
            &self.bun_path,
            &self.go_path,
            "",
            "",
            "",
            &custom,
        );
    }
}

impl Default for UpdaterSettings {
    fn default() -> Self {
        let base = ProjectsFile::default();
        let bun_path = default_bun_path();
        let go_path = default_go_path();
        let variables = build_tool_variables(
            &base.git_path,
            &base.php_path,
            &base.composer_path,
            &base.npm_path,
            &bun_path,
            &go_path,
            "",
            "",
            "",
            &BTreeMap::new(),
        );
        Self {
            git_path: base.git_path,
            php_path: base.php_path,
            composer_path: base.composer_path,
            npm_path: base.npm_path,
            bun_path,
            pnpm_path: String::new(),
            yarn_path: String::new(),
            go_path,
            log_dir: base.log_dir,
            max_concurrent_updates: 1,
            variables,
        }
    }
}

fn default_bun_path() -> String {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .map(|profile| profile.join(".bun").join("bin").join("bun.exe"))
        .filter(|path| path.is_file())
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_default()
}

fn default_go_path() -> String {
    let path = PathBuf::from(r"C:\Program Files\Go\bin\go.exe");
    if path.is_file() {
        path.to_string_lossy().to_string()
    } else {
        String::new()
    }
}

impl UpdaterSettings {
    pub fn to_projects_file(&self, allowed_client_ips: Vec<String>) -> ProjectsFile {
        ProjectsFile {
            version: 1,
            scan_roots: Vec::new(),
            allowed_client_ips,
            git_path: self.git_path.clone(),
            php_path: self.php_path.clone(),
            composer_path: self.composer_path.clone(),
            npm_path: self.npm_path.clone(),
            log_dir: self.log_dir.clone(),
            projects: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TriggerSettings {
    #[serde(
        default = "default_allowed_ips",
        rename = "allowedClientIPs",
        alias = "allowedClientIps"
    )]
    pub allowed_client_ips: Vec<String>,
    #[serde(default = "default_true")]
    pub allow_local: bool,
}

fn default_allowed_ips() -> Vec<String> {
    vec!["127.0.0.1".to_string(), "::1".to_string()]
}

impl Default for TriggerSettings {
    fn default() -> Self {
        Self {
            allowed_client_ips: default_allowed_ips(),
            allow_local: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_interval")]
    pub check_interval_minutes: u64,
}

fn default_interval() -> u64 {
    5
}

impl Default for SchedulerSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            check_interval_minutes: 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdaterConfig {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub settings: UpdaterSettings,
    #[serde(default)]
    pub trigger: TriggerSettings,
    #[serde(default)]
    pub apps: Vec<AppConfig>,
    #[serde(default)]
    pub scheduler: SchedulerSettings,
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        Self {
            version: 2,
            settings: UpdaterSettings::default(),
            trigger: TriggerSettings::default(),
            apps: Vec::new(),
            scheduler: SchedulerSettings::default(),
        }
    }
}

impl UpdaterConfig {
    pub fn normalize(&mut self) {
        self.settings.log_dir = self.settings.log_dir.trim().to_string();
        self.trigger.allowed_client_ips = normalize_list(&self.trigger.allowed_client_ips);
        self.apps.retain(|app| !app.id.trim().is_empty());
        for app in self.apps.iter_mut() {
            app.normalize();
        }
        if self.settings.max_concurrent_updates == 0 {
            self.settings.max_concurrent_updates = 1;
        }
        self.settings.refresh_variables();
    }

    pub fn find(&self, id: &str) -> Option<&AppConfig> {
        self.apps.iter().find(|app| app.id.eq_ignore_ascii_case(id))
    }

    pub fn find_mut(&mut self, id: &str) -> Option<&mut AppConfig> {
        self.apps
            .iter_mut()
            .find(|app| app.id.eq_ignore_ascii_case(id))
    }

    pub fn validate_app(&self, app: &AppConfig) -> Result<(), String> {
        if app.id.trim().is_empty() {
            return Err("App id is required.".to_string());
        }
        if !app
            .id
            .chars()
            .all(|value| value.is_ascii_alphanumeric() || matches!(value, '-' | '_' | '.'))
        {
            return Err(
                "App id may only contain letters, numbers, dots, underscores, or hyphens."
                    .to_string(),
            );
        }
        if app.repo_url.trim().is_empty() {
            return Err(format!("Repo URL for app '{}' is required.", app.id));
        }
        if app.local_path.trim().is_empty() {
            return Err(format!("Local path for app '{}' is required.", app.id));
        }
        if app.branch.trim().is_empty() {
            return Err(format!("Branch for app '{}' is required.", app.id));
        }
        crate::dirty::validate_patterns(&app.dirty_ignore)
            .map_err(|error| format!("dirtyIgnore app '{}': {error}", app.id))?;
        if self.find(&app.id).is_some() {
            // Caller can use this for add; editing should bypass this check.
        }
        Ok(())
    }
}

pub fn load_config(path: &Path) -> Result<UpdaterConfig, String> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(UpdaterConfig::default())
        }
        Err(error) => return Err(format!("Failed to read config: {error}")),
    };
    if raw.trim().is_empty() {
        return Ok(UpdaterConfig::default());
    }

    let value: serde_json::Value =
        serde_json::from_str(&raw).map_err(|error| format!("Config JSON is invalid: {error}"))?;
    let version = value
        .get("version")
        .and_then(|value| value.as_u64())
        .unwrap_or(1);

    if version >= 2 || value.get("apps").is_some() {
        let mut config: UpdaterConfig = serde_json::from_value(value)
            .map_err(|error| format!("Registry config is invalid: {error}"))?;
        config.normalize();
        Ok(config)
    } else {
        let legacy: ProjectsFile = serde_json::from_value(value)
            .map_err(|error| format!("Legacy config is invalid: {error}"))?;
        let mut config = from_legacy(legacy);
        config.normalize();
        Ok(config)
    }
}

fn from_legacy(legacy: ProjectsFile) -> UpdaterConfig {
    let mut settings = UpdaterSettings {
        git_path: legacy.git_path.clone(),
        php_path: legacy.php_path.clone(),
        composer_path: legacy.composer_path.clone(),
        npm_path: legacy.npm_path.clone(),
        log_dir: legacy.log_dir.clone(),
        ..UpdaterSettings::default()
    };
    settings.refresh_variables();
    let apps = legacy
        .projects
        .iter()
        .map(|(name, project)| {
            let mut app = AppConfig {
                id: name.clone(),
                name: name.clone(),
                enabled: project.enabled,
                local_path: project.path.clone(),
                branch: project.branch.clone(),
                composer: project.composer,
                build: project.build,
                queue_restart: project.queue_restart,
                post_update_command: project.post_update_command.clone(),
                ..AppConfig::default()
            };
            app.normalize();
            app
        })
        .collect();

    UpdaterConfig {
        version: 2,
        settings,
        trigger: TriggerSettings {
            allowed_client_ips: legacy.allowed_client_ips,
            allow_local: true,
        },
        apps,
        scheduler: SchedulerSettings::default(),
    }
}

pub fn save_config(path: &Path, config: &UpdaterConfig) -> Result<(), String> {
    let mut config = config.clone();
    config.normalize();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let json = serde_json::to_string_pretty(&config).map_err(|error| error.to_string())?;
    let temp = path.with_extension(format!("json.tmp-{}", std::process::id()));
    fs::write(&temp, json).map_err(|error| error.to_string())?;
    if let Err(error) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error.to_string());
    }
    Ok(())
}

pub fn project_settings(config: &UpdaterConfig) -> ProjectsFile {
    config
        .settings
        .to_projects_file(config.trigger.allowed_client_ips.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_v2_registry() {
        let raw = r#"{
            "version": 2,
            "trigger": { "allowedClientIPs": ["10.255.240.143", "10.255.240.0/24"] },
            "apps": [{
                "id": "kasirin",
                "name": "Kasirin",
                "repoUrl": "https://example.com/kasirin.git",
                "localPath": "D:\\Projects\\PHPProjects\\kasirin",
                "branch": "main",
                "preset": "laravel",
                "packageManager": "npm",
                "webServerService": " Apache2.4 ",
                "dirtyIgnore": [" public/build/** ", ""]
            }]
        }"#;
        let config: UpdaterConfig = serde_json::from_str(raw).unwrap();
        assert_eq!(config.version, 2);
        assert_eq!(config.apps.len(), 1);
        assert_eq!(config.apps[0].preset, Preset::Laravel);
        assert_eq!(config.apps[0].package_manager, PackageManager::Npm);
        assert_eq!(config.apps[0].web_server_service, " Apache2.4 ");
        assert_eq!(
            config.apps[0].dirty_ignore,
            [" public/build/** ".to_string(), String::new()]
        );
        assert_eq!(
            config.trigger.allowed_client_ips,
            ["10.255.240.143", "10.255.240.0/24"]
        );

        let mut config = config;
        config.normalize();
        assert_eq!(config.apps[0].web_server_service, "Apache2.4");
        assert_eq!(config.apps[0].dirty_ignore, ["public/build/**"]);
    }

    #[test]
    fn converts_v1_config_to_registry() {
        let raw = r#"{
            "version": 1,
            "scanRoots": ["D:\\Projects\\PHPProjects"],
            "allowedClientIPs": ["10.255.240.143"],
            "gitPath": "C:\\git.exe",
            "phpPath": "C:\\php.exe",
            "composerPath": "C:\\composer.bat",
            "npmPath": "C:\\npm.cmd",
            "projects": {
                "kasirin": {
                    "path": "D:\\Projects\\PHPProjects\\kasirin",
                    "enabled": true,
                    "branch": "main",
                    "composer": "auto",
                    "build": "auto",
                    "queueRestart": true
                }
            }
        }"#;
        let value: ProjectsFile = serde_json::from_str(raw).unwrap();
        let config = from_legacy(value);
        assert_eq!(config.version, 2);
        assert_eq!(config.apps.len(), 1);
        assert_eq!(config.apps[0].id, "kasirin");
        assert_eq!(
            config.apps[0].local_path,
            r"D:\Projects\PHPProjects\kasirin"
        );
        assert_eq!(config.settings.git_path, r"C:\git.exe");
        assert_eq!(config.trigger.allowed_client_ips, ["10.255.240.143"]);
    }

    #[test]
    fn saves_and_loads_v2() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data/projects.json");
        let mut config = UpdaterConfig::default();
        config.apps.push(AppConfig {
            id: "demo".into(),
            name: "Demo".into(),
            repo_url: "https://example.com/demo.git".into(),
            local_path: temp.path().to_string_lossy().to_string(),
            branch: "main".into(),
            ..AppConfig::default()
        });
        save_config(&path, &config).unwrap();
        let loaded = load_config(&path).unwrap();
        assert_eq!(loaded.version, 2);
        assert_eq!(loaded.apps[0].id, "demo");
        assert_eq!(loaded.apps[0].repo_url, "https://example.com/demo.git");
    }

    #[test]
    fn refresh_variables_preserves_admin_variables_and_overrides_builtins() {
        let mut settings = UpdaterSettings {
            php_path: r"C:\custom\php.exe".into(),
            ..UpdaterSettings::default()
        };
        settings
            .variables
            .insert("PROJECT_ROOT".into(), r"D:\Projects".into());
        settings
            .variables
            .insert("PHP".into(), r"C:\bogus\php.exe".into());

        settings.refresh_variables();

        assert_eq!(settings.variables["PROJECT_ROOT"], r"D:\Projects");
        assert_eq!(settings.variables["PHP"], r"C:\custom\php.exe");
        assert_eq!(settings.variables["PHP_HOME"], r"C:\custom");
    }

    #[test]
    fn validate_app_rejects_invalid_dirty_ignore_patterns() {
        let config = UpdaterConfig::default();
        let app = AppConfig {
            id: "demo".into(),
            repo_url: "https://example.com/demo.git".into(),
            local_path: r"C:\demo".into(),
            branch: "main".into(),
            dirty_ignore: vec!["../outside".into()],
            ..AppConfig::default()
        };

        let error = config.validate_app(&app).unwrap_err();
        assert!(error.contains("dirtyIgnore app 'demo'"), "{error}");
        assert!(error.contains(".."), "{error}");
    }

    #[test]
    fn normalizes_values() {
        let mut app = AppConfig {
            id: " demo ".into(),
            repo_url: " https://example.com ".into(),
            local_path: " C:\\demo ".into(),
            ..AppConfig::default()
        };
        app.normalize();
        assert_eq!(app.id, "demo");
        assert_eq!(app.repo_url, "https://example.com");
        assert_eq!(app.local_path, r"C:\demo");
        assert_eq!(app.name, "demo");
    }
}
