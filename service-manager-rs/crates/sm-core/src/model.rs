use serde::{Deserialize, Serialize};

fn default_true() -> bool {
    true
}

fn default_restart_delay() -> u32 {
    5
}

/// Jenis service yang dikelola aplikasi.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServiceType {
    #[default]
    Nssm,
    Apache,
    #[serde(rename = "postgresql")]
    PostgreSql,
    /// Nilai tidak dikenal dari config lama; dinormalisasi menjadi NSSM.
    #[serde(other)]
    Unknown,
}

impl ServiceType {
    pub fn label(self) -> &'static str {
        match self {
            ServiceType::Nssm => "NSSM",
            ServiceType::Apache => "Apache",
            ServiceType::PostgreSql => "PostgreSQL",
            ServiceType::Unknown => "NSSM",
        }
    }

    pub fn normalized(self) -> Self {
        match self {
            ServiceType::Unknown => ServiceType::Nssm,
            other => other,
        }
    }

    pub fn is_native(self) -> bool {
        matches!(
            self.normalized(),
            ServiceType::Apache | ServiceType::PostgreSql
        )
    }
}

/// Mode startup service.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StartupMode {
    #[default]
    Auto,
    Delayed,
    Manual,
    /// Nilai tidak dikenal dari config lama; dinormalisasi menjadi Auto.
    #[serde(other)]
    Unknown,
}

impl StartupMode {
    pub fn normalized(self) -> Self {
        match self {
            StartupMode::Unknown => StartupMode::Auto,
            other => other,
        }
    }

    pub fn label(self) -> &'static str {
        match self.normalized() {
            StartupMode::Auto => "Automatic",
            StartupMode::Delayed => "Automatic (Delayed)",
            StartupMode::Manual => "Manual",
            StartupMode::Unknown => "Automatic",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToggleMode {
    #[default]
    Auto,
    Always,
    Never,
    #[serde(other)]
    Unknown,
}

impl ToggleMode {
    pub fn normalized(self) -> Self {
        match self {
            ToggleMode::Unknown => ToggleMode::Auto,
            other => other,
        }
    }
}

/// Konfigurasi satu service. Nama field JSON mengikuti `services.json` versi 1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceConfig {
    pub name: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub service_type: ServiceType,
    #[serde(default)]
    pub executable: String,
    #[serde(default)]
    pub arguments: String,
    #[serde(default)]
    pub working_directory: String,
    #[serde(default)]
    pub apache_root: String,
    #[serde(default)]
    pub apache_config: String,
    #[serde(default)]
    pub pg_data: String,
    #[serde(default)]
    pub pg_account: String,
    #[serde(default)]
    pub stdout_log: String,
    #[serde(default)]
    pub stderr_log: String,
    #[serde(default)]
    pub startup: StartupMode,
    #[serde(default = "default_true")]
    pub restart_on_crash: bool,
    #[serde(default = "default_restart_delay")]
    pub restart_delay_sec: u32,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub added_at: String,
    #[serde(default)]
    pub updated_at: String,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            display_name: String::new(),
            service_type: ServiceType::Nssm,
            executable: String::new(),
            arguments: String::new(),
            working_directory: String::new(),
            apache_root: String::new(),
            apache_config: String::new(),
            pg_data: String::new(),
            pg_account: String::new(),
            stdout_log: String::new(),
            stderr_log: String::new(),
            startup: StartupMode::Auto,
            restart_on_crash: true,
            restart_delay_sec: 5,
            dependencies: Vec::new(),
            added_at: String::new(),
            updated_at: String::new(),
        }
    }
}

impl ServiceConfig {
    /// Normalisasi nilai seperti yang dilakukan `ConvertTo-ServiceConfig` di PowerShell.
    pub fn normalize(&mut self) {
        self.name = self.name.trim().to_string();
        self.display_name = self.display_name.trim().to_string();
        self.executable = self.executable.trim().to_string();
        self.arguments = self.arguments.trim().to_string();
        self.working_directory = self.working_directory.trim().to_string();
        self.apache_root = self.apache_root.trim().to_string();
        self.apache_config = self.apache_config.trim().to_string();
        self.pg_data = self.pg_data.trim().to_string();
        self.pg_account = self.pg_account.trim().to_string();
        self.stdout_log = self.stdout_log.trim().to_string();
        self.stderr_log = self.stderr_log.trim().to_string();

        self.service_type = self.service_type.normalized();
        self.startup = self.startup.normalized();

        if self.display_name.is_empty() {
            self.display_name = self.name.clone();
        }

        if self.restart_delay_sec < 1 {
            self.restart_delay_sec = 1;
        }

        self.dependencies = self
            .dependencies
            .iter()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .collect();

        match self.service_type {
            ServiceType::Apache => {
                if self.apache_config.is_empty() {
                    self.apache_config = "conf/httpd.conf".to_string();
                }
                if self.working_directory.is_empty() && !self.apache_root.is_empty() {
                    self.working_directory = self.apache_root.clone();
                }
            }
            ServiceType::PostgreSql => {
                if self.pg_account.is_empty() {
                    self.pg_account = "NT AUTHORITY\\NetworkService".to_string();
                }
                if self.working_directory.is_empty() && !self.pg_data.is_empty() {
                    self.working_directory = self.pg_data.clone();
                }
            }
            ServiceType::Nssm | ServiceType::Unknown => {}
        }
    }

    /// Dependensi yang dapat dipakai untuk pengurutan (trim, tanpa duplikat case-insensitive).
    pub fn normalized_dependencies(&self) -> Vec<String> {
        let mut result: Vec<String> = Vec::new();
        for dep in &self.dependencies {
            let dep = dep.trim();
            if dep.is_empty() {
                continue;
            }
            if result
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(dep))
            {
                continue;
            }
            result.push(dep.to_string());
        }
        result
    }
}

/// Override konfigurasi per-project untuk updater.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectOverride {
    #[serde(default)]
    pub path: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_branch")]
    pub branch: String,
    #[serde(default)]
    pub composer: ToggleMode,
    #[serde(default)]
    pub build: ToggleMode,
    #[serde(default = "default_true")]
    pub queue_restart: bool,
    #[serde(default)]
    pub post_update_command: String,
}

fn default_branch() -> String {
    "main".to_string()
}

impl Default for ProjectOverride {
    fn default() -> Self {
        Self {
            path: String::new(),
            enabled: true,
            branch: "main".to_string(),
            composer: ToggleMode::Auto,
            build: ToggleMode::Auto,
            queue_restart: true,
            post_update_command: String::new(),
        }
    }
}

impl ProjectOverride {
    pub fn normalize(&mut self) {
        self.path = self.path.trim().to_string();
        self.branch = self.branch.trim().to_string();
        self.post_update_command = self.post_update_command.trim().to_string();
        self.composer = self.composer.normalized();
        self.build = self.build.normalized();
        if self.branch.is_empty() {
            self.branch = default_branch();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_service_type_falls_back_to_nssm() {
        let json = r#"{
            "name": "app",
            "serviceType": "tomcat",
            "startup": "weird"
        }"#;
        let mut svc: ServiceConfig = serde_json::from_str(json).unwrap();
        svc.normalize();
        assert_eq!(svc.service_type, ServiceType::Nssm);
        assert_eq!(svc.startup, StartupMode::Auto);
        assert_eq!(svc.display_name, "app");
        assert!(svc.restart_on_crash);
        assert_eq!(svc.restart_delay_sec, 5);
    }

    #[test]
    fn apache_defaults_are_applied() {
        let mut svc = ServiceConfig {
            name: "Apache2.4".into(),
            service_type: ServiceType::Apache,
            apache_root: r"C:\Apache24".into(),
            ..ServiceConfig::default()
        };
        svc.normalize();
        assert_eq!(svc.apache_config, "conf/httpd.conf");
        assert_eq!(svc.working_directory, r"C:\Apache24");
    }

    #[test]
    fn postgres_defaults_are_applied() {
        let mut svc = ServiceConfig {
            name: "postgresql-16".into(),
            service_type: ServiceType::PostgreSql,
            pg_data: r"C:\PostgreSQL\data".into(),
            ..ServiceConfig::default()
        };
        svc.normalize();
        assert_eq!(svc.pg_account, r"NT AUTHORITY\NetworkService");
        assert_eq!(svc.working_directory, r"C:\PostgreSQL\data");
    }

    #[test]
    fn dependencies_are_trimmed_and_deduplicated() {
        let svc = ServiceConfig {
            dependencies: vec![
                " Tcpip ".into(),
                "tcpip".into(),
                "".into(),
                "postgresql-16".into(),
            ],
            ..ServiceConfig::default()
        };
        assert_eq!(
            svc.normalized_dependencies(),
            vec!["Tcpip".to_string(), "postgresql-16".to_string()]
        );
    }

    #[test]
    fn restart_delay_is_clamped() {
        let mut svc = ServiceConfig {
            restart_delay_sec: 0,
            ..ServiceConfig::default()
        };
        svc.normalize();
        assert_eq!(svc.restart_delay_sec, 1);
    }

    #[test]
    fn project_defaults_are_applied() {
        let mut project: ProjectOverride = serde_json::from_str("{}").unwrap();
        project.normalize();
        assert!(project.enabled);
        assert!(project.queue_restart);
        assert_eq!(project.branch, "main");
        assert_eq!(project.composer, ToggleMode::Auto);
        assert_eq!(project.build, ToggleMode::Auto);
    }
}
