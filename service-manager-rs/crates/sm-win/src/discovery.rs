use std::path::{Path, PathBuf};

use sm_core::{ServiceConfig, ServiceType, StartupMode};
use thiserror::Error;
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::System::Services::{
    EnumServicesStatusExW, QueryServiceConfig2W, QueryServiceConfigW, ENUM_SERVICE_STATUS_PROCESSW,
    QUERY_SERVICE_CONFIGW, SC_ENUM_PROCESS_INFO, SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
    SERVICE_QUERY_CONFIG, SERVICE_STATE_ALL, SERVICE_WIN32,
};

use crate::error::ScmError;
use crate::nssm::Nssm;
use crate::process::CommandRunner;
use crate::scm;

#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error(transparent)]
    Scm(#[from] ScmError),
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RawService {
    pub name: String,
    pub display_name: String,
    pub binary_path: String,
    pub start_type: u32,
    pub delayed: bool,
    pub dependencies: Vec<String>,
    pub service_start_name: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NssmValues {
    pub application: Option<String>,
    pub parameters: Option<String>,
    pub directory: Option<String>,
    pub stdout_log: Option<String>,
    pub stderr_log: Option<String>,
    pub app_exit_default: Option<String>,
    pub restart_delay_ms: Option<String>,
    pub dependencies: Option<String>,
}

pub fn classify_binary_path(binary_path: &str) -> Option<ServiceType> {
    let lower = binary_path.to_ascii_lowercase();
    if lower.contains("nssm.exe") {
        Some(ServiceType::Nssm)
    } else if lower.contains("httpd.exe") {
        Some(ServiceType::Apache)
    } else if lower.contains("pg_ctl.exe") {
        Some(ServiceType::PostgreSql)
    } else {
        None
    }
}

/// Ambil path executable dari `PathName` service, mendukung path berkutip.
pub fn executable_from_path_name(path_name: &str) -> String {
    let text = path_name.trim();
    if text.is_empty() {
        return String::new();
    }
    if let Some(rest) = text.strip_prefix('"') {
        if let Some(end) = rest.find('"') {
            return rest[..end].to_string();
        }
    }

    let lower = text.to_ascii_lowercase();
    if let Some(index) = lower.find(".exe") {
        return text[..index + 4].trim().to_string();
    }

    text.split_whitespace()
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Cari nilai argumen (mis. `-d`, `-f`, `-D`) secara case-insensitive.
pub fn find_argument(command_line: &str, name: &str) -> Option<String> {
    let chars: Vec<char> = command_line.chars().collect();
    let mut index = 0usize;

    while index < chars.len() {
        while index < chars.len() && chars[index].is_whitespace() {
            index += 1;
        }
        if index >= chars.len() {
            break;
        }

        let token = if chars[index] == '"' {
            index += 1;
            let start = index;
            while index < chars.len() && chars[index] != '"' {
                index += 1;
            }
            let token: String = chars[start..index].iter().collect();
            index = (index + 1).min(chars.len());
            token
        } else {
            let start = index;
            while index < chars.len() && !chars[index].is_whitespace() {
                index += 1;
            }
            chars[start..index].iter().collect()
        };

        if token.eq_ignore_ascii_case(name) {
            while index < chars.len() && chars[index].is_whitespace() {
                index += 1;
            }
            if index >= chars.len() {
                return None;
            }
            if chars[index] == '"' {
                index += 1;
                let start = index;
                while index < chars.len() && chars[index] != '"' {
                    index += 1;
                }
                return Some(chars[start..index].iter().collect());
            }
            let start = index;
            while index < chars.len() && !chars[index].is_whitespace() {
                index += 1;
            }
            return Some(chars[start..index].iter().collect());
        }
    }

    None
}

pub fn apache_root_from_exe(executable: &str) -> String {
    if executable.trim().is_empty() {
        return String::new();
    }
    let path = Path::new(executable);
    let Some(parent) = path.parent() else {
        return String::new();
    };
    if parent
        .file_name()
        .map(|name| name.eq_ignore_ascii_case("bin"))
        .unwrap_or(false)
    {
        parent
            .parent()
            .map(|value| value.to_string_lossy().to_string())
            .unwrap_or_default()
    } else {
        parent.to_string_lossy().to_string()
    }
}

pub fn split_dependencies(value: &str) -> Vec<String> {
    value
        .split(|character: char| character.is_whitespace() || character == ',')
        .map(|item| item.trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}

pub fn startup_from_raw(start_type: u32, delayed: bool) -> StartupMode {
    // SERVICE_BOOT_START=0, SYSTEM_START=1, AUTO_START=2, DEMAND_START=3, DISABLED=4
    if delayed {
        StartupMode::Delayed
    } else if start_type >= 3 {
        StartupMode::Manual
    } else {
        StartupMode::Auto
    }
}

pub fn newest_log_file(directory: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(directory).ok()?;
    entries
        .flatten()
        .filter(|entry| {
            entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
        })
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .map(|extension| extension.eq_ignore_ascii_case("log"))
                .unwrap_or(false)
        })
        .max_by_key(|path| {
            std::fs::metadata(path)
                .and_then(|metadata| metadata.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        })
}

pub fn candidate_from_raw(
    raw: &RawService,
    nssm_values: Option<&NssmValues>,
) -> Option<ServiceConfig> {
    let service_type = classify_binary_path(&raw.binary_path)?;
    let executable = executable_from_path_name(&raw.binary_path);
    let startup = startup_from_raw(raw.start_type, raw.delayed);

    let mut config = ServiceConfig {
        name: raw.name.clone(),
        display_name: raw.display_name.clone(),
        service_type,
        executable,
        startup,
        dependencies: raw.dependencies.clone(),
        restart_on_crash: true,
        restart_delay_sec: 5,
        ..ServiceConfig::default()
    };

    match service_type {
        ServiceType::Nssm => {
            let values = nssm_values.cloned().unwrap_or_default();
            config.executable = values.application.unwrap_or_default();
            config.arguments = values.parameters.unwrap_or_default();
            config.working_directory = values.directory.unwrap_or_default();
            config.stdout_log = values.stdout_log.unwrap_or_default();
            config.stderr_log = values.stderr_log.unwrap_or_default();

            if let Some(exit) = values.app_exit_default {
                config.restart_on_crash = !exit.eq_ignore_ascii_case("Exit");
            }
            if let Some(delay) = values.restart_delay_ms {
                if let Ok(milliseconds) = delay.trim().parse::<u32>() {
                    let seconds = ((milliseconds as f64) / 1000.0).round() as u32;
                    config.restart_delay_sec = seconds.max(1);
                }
            }
            if let Some(dependencies) = values.dependencies {
                let parsed = split_dependencies(&dependencies);
                if !parsed.is_empty() {
                    config.dependencies = parsed;
                }
            }
        }
        ServiceType::Apache => {
            let root = find_argument(&raw.binary_path, "-d")
                .unwrap_or_else(|| apache_root_from_exe(&config.executable));
            config.apache_root = root.clone();
            config.apache_config =
                find_argument(&raw.binary_path, "-f").unwrap_or_else(|| "conf/httpd.conf".into());
            config.working_directory = root.clone();
            config.arguments.clear();

            if !root.is_empty() {
                let log_dir = Path::new(&root).join("logs");
                let access_log = log_dir.join("access.log");
                let error_log = log_dir.join("error.log");
                if access_log.is_file() {
                    config.stdout_log = access_log.to_string_lossy().to_string();
                }
                if error_log.is_file() {
                    config.stderr_log = error_log.to_string_lossy().to_string();
                }
            }
        }
        ServiceType::PostgreSql => {
            let pg_data = find_argument(&raw.binary_path, "-D").unwrap_or_default();
            config.pg_data = pg_data.clone();
            config.pg_account = if raw.service_start_name.trim().is_empty() {
                r"NT AUTHORITY\NetworkService".to_string()
            } else {
                raw.service_start_name.clone()
            };
            config.working_directory = pg_data.clone();
            config.arguments.clear();

            if !pg_data.is_empty() {
                if let Some(log) = newest_log_file(&Path::new(&pg_data).join("log")) {
                    let log = log.to_string_lossy().to_string();
                    config.stdout_log = log.clone();
                    config.stderr_log = log;
                }
            }
        }
        ServiceType::Unknown => {}
    }

    config.normalize();
    Some(config)
}

pub fn discover_services(
    nssm_path: &Path,
    runner: &dyn CommandRunner,
) -> Result<Vec<ServiceConfig>, DiscoveryError> {
    let raw_services = enumerate_raw()?;
    let nssm = if nssm_path.is_file() {
        Some(Nssm::new(nssm_path, runner))
    } else {
        None
    };

    let mut result = Vec::new();
    for raw in raw_services {
        let service_type = classify_binary_path(&raw.binary_path);
        let nssm_values = if service_type == Some(ServiceType::Nssm) {
            nssm.as_ref().map(|nssm| NssmValues {
                application: nssm.get_value(&raw.name, "Application").ok().flatten(),
                parameters: nssm.get_value(&raw.name, "AppParameters").ok().flatten(),
                directory: nssm.get_value(&raw.name, "AppDirectory").ok().flatten(),
                stdout_log: nssm.get_value(&raw.name, "AppStdout").ok().flatten(),
                stderr_log: nssm.get_value(&raw.name, "AppStderr").ok().flatten(),
                app_exit_default: nssm
                    .get_value_with_sub(&raw.name, "AppExit", Some("Default"))
                    .ok()
                    .flatten(),
                restart_delay_ms: nssm.get_value(&raw.name, "AppRestartDelay").ok().flatten(),
                dependencies: nssm.get_value(&raw.name, "DependOnService").ok().flatten(),
            })
        } else {
            None
        };

        if let Some(candidate) = candidate_from_raw(&raw, nssm_values.as_ref()) {
            result.push(candidate);
        }
    }

    result.sort_by(|left, right| {
        left.name
            .to_ascii_lowercase()
            .cmp(&right.name.to_ascii_lowercase())
    });
    Ok(result)
}

fn enumerate_raw() -> Result<Vec<RawService>, DiscoveryError> {
    let scm = scm::open_scm()?;
    let mut needed = 0u32;
    let mut returned = 0u32;
    let mut resume = 0u32;

    let _ = unsafe {
        EnumServicesStatusExW(
            scm.0,
            SC_ENUM_PROCESS_INFO,
            SERVICE_WIN32,
            SERVICE_STATE_ALL,
            None,
            &mut needed,
            &mut returned,
            Some(&mut resume),
            PCWSTR::null(),
        )
    };

    if needed == 0 {
        return Ok(Vec::new());
    }

    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    let mut returned = 0u32;
    let mut resume = 0u32;
    let buffer_size = (buffer.len() * 8) as u32;
    let buffer_bytes = unsafe {
        std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<u8>(), buffer_size as usize)
    };

    unsafe {
        EnumServicesStatusExW(
            scm.0,
            SC_ENUM_PROCESS_INFO,
            SERVICE_WIN32,
            SERVICE_STATE_ALL,
            Some(buffer_bytes),
            &mut needed,
            &mut returned,
            Some(&mut resume),
            PCWSTR::null(),
        )
    }
    .map_err(|error| DiscoveryError::Other(format!("Gagal enumerasi service: {error}")))?;

    let mut result = Vec::with_capacity(returned as usize);
    let base = buffer.as_ptr().cast::<ENUM_SERVICE_STATUS_PROCESSW>();
    for index in 0..returned as usize {
        let entry = unsafe { &*base.add(index) };
        let name = unsafe { pwstr_to_string(entry.lpServiceName) };
        let display_name = unsafe { pwstr_to_string(entry.lpDisplayName) };
        if let Ok(raw) = query_raw_config(&scm, &name, &display_name) {
            result.push(raw);
        }
    }

    Ok(result)
}

fn query_raw_config(
    scm: &scm::ScmHandle,
    name: &str,
    enum_display_name: &str,
) -> Result<RawService, ScmError> {
    let service = scm::open_service(scm, name, SERVICE_QUERY_CONFIG)?;

    let mut needed = 0u32;
    let _ = unsafe { QueryServiceConfigW(service.0, None, 0, &mut needed) };
    if needed == 0 {
        return Err(ScmError::Other(format!(
            "Tidak dapat membaca config service '{name}'."
        )));
    }

    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    let buffer_size = (buffer.len() * 8) as u32;
    unsafe {
        QueryServiceConfigW(
            service.0,
            Some(buffer.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>()),
            buffer_size,
            &mut needed,
        )
    }
    .map_err(|error| ScmError::Windows(format!("QueryServiceConfig '{name}': {error}")))?;

    let config = unsafe { &*buffer.as_ptr().cast::<QUERY_SERVICE_CONFIGW>() };
    let binary_path = unsafe { pwstr_to_string(config.lpBinaryPathName) };
    let display_name = unsafe { pwstr_to_string(config.lpDisplayName) }
        .trim()
        .to_string();
    let service_start_name = unsafe { pwstr_to_string(config.lpServiceStartName) };
    let dependencies = unsafe { read_multi_string(config.lpDependencies) };
    let delayed = query_delayed_start(&service);

    Ok(RawService {
        name: name.to_string(),
        display_name: if display_name.is_empty() {
            enum_display_name.to_string()
        } else {
            display_name
        },
        binary_path,
        start_type: config.dwStartType.0,
        delayed,
        dependencies,
        service_start_name,
    })
}

fn query_delayed_start(service: &scm::ServiceHandle) -> bool {
    let mut needed = 0u32;
    let _ = unsafe {
        QueryServiceConfig2W(
            service.0,
            SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
            None,
            &mut needed,
        )
    };
    if needed == 0 {
        return false;
    }

    let mut buffer = vec![0u64; (needed as usize).div_ceil(8)];
    let buffer_size = (buffer.len() * 8) as u32;
    let buffer_bytes = unsafe {
        std::slice::from_raw_parts_mut(buffer.as_mut_ptr().cast::<u8>(), buffer_size as usize)
    };

    if unsafe {
        QueryServiceConfig2W(
            service.0,
            SERVICE_CONFIG_DELAYED_AUTO_START_INFO,
            Some(buffer_bytes),
            &mut needed,
        )
    }
    .is_err()
    {
        return false;
    }

    let info = unsafe {
        &*buffer
            .as_ptr()
            .cast::<windows::Win32::System::Services::SERVICE_DELAYED_AUTO_START_INFO>()
    };
    info.fDelayedAutostart.as_bool()
}

unsafe fn pwstr_to_string(value: PWSTR) -> String {
    if value.0.is_null() {
        String::new()
    } else {
        value.to_string().unwrap_or_default()
    }
}

unsafe fn read_multi_string(value: PWSTR) -> Vec<String> {
    if value.0.is_null() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut current: Vec<u16> = Vec::new();
    let mut index = 0isize;

    loop {
        let unit = *value.0.offset(index);
        index += 1;
        if unit == 0 {
            if current.is_empty() {
                break;
            }
            result.push(String::from_utf16_lossy(&current));
            current.clear();
            if *value.0.offset(index) == 0 {
                break;
            }
        } else {
            current.push(unit);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::SystemRunner;

    fn raw_nssm() -> RawService {
        RawService {
            name: "my-app".into(),
            display_name: "My App".into(),
            binary_path: r#""C:\tools\nssm\nssm.exe" my-app"#.into(),
            start_type: 2,
            delayed: false,
            dependencies: vec!["Tcpip".into()],
            service_start_name: "LocalSystem".into(),
        }
    }

    #[test]
    fn classifies_supported_binaries() {
        assert_eq!(
            classify_binary_path(r"C:\tools\nssm\nssm.exe"),
            Some(ServiceType::Nssm)
        );
        assert_eq!(
            classify_binary_path(r#"C:\Apache24\bin\httpd.exe -d C:\Apache24"#),
            Some(ServiceType::Apache)
        );
        assert_eq!(
            classify_binary_path(r"C:\PostgreSQL\bin\pg_ctl.exe runservice"),
            Some(ServiceType::PostgreSql)
        );
        assert_eq!(classify_binary_path(r"C:\Windows\notepad.exe"), None);
    }

    #[test]
    fn extracts_executable_from_quoted_and_plain_paths() {
        assert_eq!(
            executable_from_path_name(r#""C:\Program Files\App\app.exe" --flag"#),
            r"C:\Program Files\App\app.exe"
        );
        assert_eq!(
            executable_from_path_name(r"C:\Apps\app.exe --flag"),
            r"C:\Apps\app.exe"
        );
    }

    #[test]
    fn finds_command_line_arguments_case_insensitively() {
        let cmd = r#""C:\Apache24\bin\httpd.exe" -d "C:\Apache24" -f conf/httpd.conf"#;
        assert_eq!(find_argument(cmd, "-d"), Some(r"C:\Apache24".into()));
        assert_eq!(find_argument(cmd, "-f"), Some("conf/httpd.conf".into()));
        // Matching bersifat case-insensitive, sama seperti PowerShell.
        assert_eq!(find_argument(cmd, "-D"), Some(r"C:\Apache24".into()));
        assert_eq!(find_argument(cmd, "-x"), None);

        let pg =
            r#""C:\PostgreSQL\bin\pg_ctl.exe" runservice -N "postgresql-16" -D "C:\PG\data" -w"#;
        assert_eq!(find_argument(pg, "-D"), Some(r"C:\PG\data".into()));
    }

    #[test]
    fn derives_apache_root_from_bin_directory() {
        assert_eq!(
            apache_root_from_exe(r"C:\Apache24\bin\httpd.exe"),
            r"C:\Apache24"
        );
        assert_eq!(
            apache_root_from_exe(r"C:\Apache24\httpd.exe"),
            r"C:\Apache24"
        );
    }

    #[test]
    fn splits_dependencies_from_nssm_output() {
        assert_eq!(
            split_dependencies("Tcpip postgresql-16, redis"),
            vec!["Tcpip", "postgresql-16", "redis"]
        );
    }

    #[test]
    fn maps_startup_modes() {
        assert_eq!(startup_from_raw(2, false), StartupMode::Auto);
        assert_eq!(startup_from_raw(2, true), StartupMode::Delayed);
        assert_eq!(startup_from_raw(3, false), StartupMode::Manual);
        assert_eq!(startup_from_raw(4, false), StartupMode::Manual);
    }

    #[test]
    fn builds_nssm_candidate_from_values() {
        let values = NssmValues {
            application: Some(r"C:\apps\demo\node.exe".into()),
            parameters: Some("index.js".into()),
            directory: Some(r"C:\apps\demo".into()),
            stdout_log: Some(r"C:\apps\demo\out.log".into()),
            stderr_log: Some(r"C:\apps\demo\err.log".into()),
            app_exit_default: Some("Exit".into()),
            restart_delay_ms: Some("7000".into()),
            dependencies: Some("Tcpip, redis".into()),
        };
        let config = candidate_from_raw(&raw_nssm(), Some(&values)).unwrap();
        assert_eq!(config.service_type, ServiceType::Nssm);
        assert_eq!(config.executable, r"C:\apps\demo\node.exe");
        assert_eq!(config.arguments, "index.js");
        assert!(!config.restart_on_crash);
        assert_eq!(config.restart_delay_sec, 7);
        assert_eq!(config.dependencies, ["Tcpip", "redis"]);
    }

    #[test]
    fn nssm_candidate_without_application_has_empty_executable() {
        let config = candidate_from_raw(&raw_nssm(), None).unwrap();
        assert_eq!(config.executable, "");
        assert_eq!(config.display_name, "My App");
    }

    #[test]
    fn builds_apache_candidate() {
        let raw = RawService {
            name: "Apache2.4".into(),
            display_name: "Apache 2.4".into(),
            binary_path: r#""C:\Apache24\bin\httpd.exe" -d "C:\Apache24" -f "conf/httpd.conf""#
                .into(),
            start_type: 2,
            delayed: false,
            dependencies: vec!["Tcpip".into()],
            service_start_name: "LocalSystem".into(),
        };
        let config = candidate_from_raw(&raw, None).unwrap();
        assert_eq!(config.service_type, ServiceType::Apache);
        assert_eq!(config.apache_root, r"C:\Apache24");
        assert_eq!(config.apache_config, "conf/httpd.conf");
        assert_eq!(config.working_directory, r"C:\Apache24");
    }

    #[test]
    fn builds_postgres_candidate() {
        let raw = RawService {
            name: "postgresql-16".into(),
            display_name: "PostgreSQL 16".into(),
            binary_path: r#""C:\PostgreSQL\16\bin\pg_ctl.exe" runservice -N "postgresql-16" -D "C:\PG\data" -w"#.into(),
            start_type: 3,
            delayed: false,
            dependencies: vec![],
            service_start_name: r"NT AUTHORITY\NetworkService".into(),
        };
        let config = candidate_from_raw(&raw, None).unwrap();
        assert_eq!(config.service_type, ServiceType::PostgreSql);
        assert_eq!(config.pg_data, r"C:\PG\data");
        assert_eq!(config.pg_account, r"NT AUTHORITY\NetworkService");
        assert_eq!(config.startup, StartupMode::Manual);
    }

    #[test]
    fn newest_log_file_picks_latest() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("old.log"), b"old").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(temp.path().join("new.log"), b"new").unwrap();
        std::fs::write(temp.path().join("ignore.txt"), b"x").unwrap();
        let newest = newest_log_file(temp.path()).unwrap();
        assert_eq!(newest.file_name().unwrap(), "new.log");
    }

    #[test]
    fn read_only_enumeration_does_not_fail() {
        let services = discover_services(Path::new(r"Z:\missing\nssm\nssm.exe"), &SystemRunner)
            .expect("enumerasi service harus berhasil tanpa admin");
        // Tidak menjamin ada service yang dikelola; yang penting tidak error.
        assert!(services
            .iter()
            .all(|service| service.service_type != ServiceType::Unknown));
    }
}
