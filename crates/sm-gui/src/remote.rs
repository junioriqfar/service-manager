use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use sm_config::ConfigPaths;
use sm_trigger::{Args as TriggerArgs, DEFAULT_REMOTE_BIN};
use sm_win::{dpapi_protect, dpapi_unprotect, CommandNoWindow};
use tauri::{AppHandle, Emitter};

const DEFAULT_WRAPPER: &str = r"C:\tools\ServiceManager\scripts\Invoke-UpdaterTask.ps1";
const DPAPI_ENTROPY: &[u8] = b"ServiceManager-RemoteTrigger";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTriggerConfig {
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub user: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub identity: String,
    #[serde(default = "default_auth_mode")]
    pub auth_mode: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub password: String,
    #[serde(default)]
    pub remember_password: bool,
    #[serde(default)]
    pub password_protected: String,
    #[serde(default)]
    pub wrapper: String,
    #[serde(default = "default_remote_bin")]
    pub remote_bin: String,
    #[serde(default)]
    pub remote_config: String,
    #[serde(default)]
    pub app: String,
}

fn default_port() -> u16 {
    22
}

fn default_remote_bin() -> String {
    DEFAULT_REMOTE_BIN.to_string()
}

fn default_auth_mode() -> String {
    "key".to_string()
}

impl Default for RemoteTriggerConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            user: sm_trigger::default_user().unwrap_or_default(),
            port: 22,
            identity: String::new(),
            auth_mode: "key".to_string(),
            password: String::new(),
            remember_password: false,
            password_protected: String::new(),
            wrapper: DEFAULT_WRAPPER.to_string(),
            remote_bin: DEFAULT_REMOTE_BIN.to_string(),
            remote_config: String::new(),
            app: String::new(),
        }
    }
}

impl RemoteTriggerConfig {
    fn normalize(&mut self) {
        self.host = self.host.trim().to_string();
        self.user = self.user.trim().to_string();
        self.identity = self.identity.trim().to_string();
        self.wrapper = self.wrapper.trim().to_string();
        self.remote_bin = self.remote_bin.trim().to_string();
        self.remote_config = self.remote_config.trim().to_string();
        self.app = self.app.trim().to_string();
        self.auth_mode = self.auth_mode.trim().to_ascii_lowercase();
        if self.auth_mode != "password" {
            self.auth_mode = "key".to_string();
        }
        if self.port == 0 {
            self.port = 22;
        }
        if self.user.is_empty() {
            self.user = sm_trigger::default_user().unwrap_or_default();
        }
        if self.remote_bin.is_empty() {
            self.remote_bin = DEFAULT_REMOTE_BIN.to_string();
        }
    }

    fn validate(&self) -> Result<(), String> {
        if self.host.is_empty() {
            return Err("Host is required.".to_string());
        }
        if self.user.is_empty() {
            return Err("User is required.".to_string());
        }
        Ok(())
    }

    fn validate_for_run(&self) -> Result<(), String> {
        self.validate()?;
        if self.auth_mode == "password" && self.password.is_empty() {
            return Err("Password is required for password authentication.".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTriggerOutput {
    pub stream: String,
    pub line: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteTriggerResult {
    pub command: String,
    pub exit_code: i32,
    pub output: String,
}

fn config_path() -> PathBuf {
    ConfigPaths::detect().data_dir().join("trigger-client.json")
}

fn load_at(path: &Path) -> Result<RemoteTriggerConfig, String> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RemoteTriggerConfig::default())
        }
        Err(error) => return Err(error.to_string()),
    };
    if raw.trim().is_empty() {
        return Ok(RemoteTriggerConfig::default());
    }
    let mut config: RemoteTriggerConfig =
        serde_json::from_str(&raw).map_err(|error| error.to_string())?;
    config.normalize();
    if !config.password_protected.is_empty() {
        match unprotect_password(&config.password_protected) {
            Some(password) => config.password = password,
            None => {
                config.password.clear();
                config.password_protected.clear();
            }
        }
    }
    Ok(config)
}

fn save_at(path: &Path, config: &RemoteTriggerConfig) -> Result<(), String> {
    let mut persisted = config.clone();
    persisted.normalize();
    persisted.validate()?;
    if persisted.auth_mode == "password"
        && persisted.remember_password
        && !persisted.password.is_empty()
    {
        persisted.password_protected = protect_password(&persisted.password)?;
    } else {
        persisted.password_protected.clear();
    }
    persisted.password.clear();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let json = serde_json::to_string_pretty(&persisted).map_err(|error| error.to_string())?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json).map_err(|error| error.to_string())?;
    std::fs::rename(&temp, path).map_err(|error| error.to_string())
}

fn with_options(
    config: &RemoteTriggerConfig,
    app: Option<String>,
    check: bool,
) -> Result<TriggerArgs, String> {
    let mut config = config.clone();
    config.normalize();
    let mut ssh_options = vec![
        "ConnectTimeout=10".to_string(),
        "StrictHostKeyChecking=accept-new".to_string(),
    ];
    if config.auth_mode == "password" {
        ssh_options.push("PreferredAuthentications=password,keyboard-interactive".to_string());
        ssh_options.push("NumberOfPasswordPrompts=1".to_string());
    } else {
        ssh_options.push("BatchMode=yes".to_string());
    }
    Ok(TriggerArgs {
        host: config.host,
        user: config.user,
        port: config.port,
        identity: non_empty(config.identity),
        ssh_options,
        remote_bin: config.remote_bin,
        wrapper: non_empty(config.wrapper),
        config: non_empty(config.remote_config),
        app,
        check,
        dry_run: false,
        help: false,
    })
}

fn ssh_env(config: &RemoteTriggerConfig) -> Result<Vec<(String, String)>, String> {
    if config.auth_mode != "password" {
        return Ok(Vec::new());
    }
    let helper = askpass_path()?;
    Ok(vec![
        ("SM_ASKPASS".to_string(), "1".to_string()),
        ("SM_ASKPASS_PASSWORD".to_string(), config.password.clone()),
        (
            "SSH_ASKPASS".to_string(),
            helper.to_string_lossy().to_string(),
        ),
        ("SSH_ASKPASS_REQUIRE".to_string(), "force".to_string()),
        ("DISPLAY".to_string(), "localhost:0".to_string()),
    ])
}

fn askpass_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let candidate = exe
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("sm-trigger.exe");
    if candidate.is_file() {
        Ok(candidate)
    } else {
        Err(format!(
            "sm-trigger.exe was not found in the application folder ({}). It is required for SSH password authentication.",
            candidate.display()
        ))
    }
}

fn protect_password(password: &str) -> Result<String, String> {
    let protected = dpapi_protect(password.as_bytes(), Some(DPAPI_ENTROPY))
        .map_err(|error| format!("Failed to encrypt the password with DPAPI: {error}"))?;
    Ok(hex_encode(&protected))
}

fn unprotect_password(value: &str) -> Option<String> {
    let bytes = hex_decode(value)?;
    let plain = dpapi_unprotect(&bytes, Some(DPAPI_ENTROPY)).ok()?;
    String::from_utf8(plain).ok()
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write;
        let _ = write!(text, "{byte:02x}");
    }
    text
}

fn hex_decode(value: &str) -> Option<Vec<u8>> {
    let value = value.trim();
    if !value.len().is_multiple_of(2) {
        return None;
    }
    let mut bytes = Vec::with_capacity(value.len() / 2);
    let chars: Vec<char> = value.chars().collect();
    let mut index = 0usize;
    while index < chars.len() {
        let high = chars[index].to_digit(16)?;
        let low = chars[index + 1].to_digit(16)?;
        bytes.push(((high << 4) | low) as u8);
        index += 2;
    }
    Some(bytes)
}

fn non_empty(value: String) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value)
    }
}

#[tauri::command]
pub fn load_remote_trigger() -> Result<RemoteTriggerConfig, String> {
    load_at(&config_path())
}

#[tauri::command]
pub fn save_remote_trigger(mut config: RemoteTriggerConfig) -> Result<RemoteTriggerConfig, String> {
    config.normalize();
    config.validate()?;
    save_at(&config_path(), &config)?;
    Ok(config)
}

#[tauri::command]
pub fn preview_remote_trigger(
    config: RemoteTriggerConfig,
    app: Option<String>,
    check: bool,
) -> Result<String, String> {
    let mut config = config;
    config.normalize();
    config.validate()?;
    Ok(sm_trigger::preview(&with_options(&config, app, check)?))
}

#[tauri::command]
pub async fn test_remote_ssh(config: RemoteTriggerConfig) -> Result<RemoteTriggerResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut config = config;
        config.normalize();
        config.validate_for_run()?;
        let args = with_options(&config, None, false)?;
        let env = ssh_env(&config)?;
        let command = sm_trigger::preview_with(&args, "echo sm-trigger-ok");
        let (program, ssh_args) = sm_trigger::ssh_program_args_with(&args, "echo sm-trigger-ok");
        run_capture(&program, &ssh_args, &env, command)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn run_remote_trigger(
    app: AppHandle,
    config: RemoteTriggerConfig,
    app_id: Option<String>,
    check: bool,
) -> Result<RemoteTriggerResult, String> {
    let mut config = config;
    config.normalize();
    config.validate_for_run()?;
    let args = with_options(&config, app_id, check)?;
    let env = ssh_env(&config)?;
    let command = sm_trigger::preview(&args);
    let (program, ssh_args) = sm_trigger::ssh_program_args(&args);

    tauri::async_runtime::spawn_blocking(move || {
        run_streaming(&app, &program, &ssh_args, &env, command)
    })
    .await
    .map_err(|error| error.to_string())?
}

fn run_capture(
    program: &str,
    args: &[String],
    env: &[(String, String)],
    command: String,
) -> Result<RemoteTriggerResult, String> {
    let output = Command::new(program)
        .args(args)
        .envs(
            env.iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        )
        .stdin(Stdio::null())
        .no_window()
        .output()
        .map_err(|error| format!("Failed to run {program}: {error}"))?;
    let mut text = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    if !stderr.trim().is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    Ok(RemoteTriggerResult {
        command,
        exit_code: output.status.code().unwrap_or(-1),
        output: text,
    })
}

fn run_streaming(
    app: &AppHandle,
    program: &str,
    args: &[String],
    env: &[(String, String)],
    command: String,
) -> Result<RemoteTriggerResult, String> {
    let mut child = Command::new(program)
        .args(args)
        .envs(
            env.iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .no_window()
        .spawn()
        .map_err(|error| format!("Failed to run {program}: {error}"))?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture ssh stdout.".to_string())?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to capture ssh stderr.".to_string())?;
    let lines = Arc::new(Mutex::new(Vec::<String>::new()));

    let out_handle = app.clone();
    let out_lines = Arc::clone(&lines);
    let out_thread = thread::spawn(move || {
        read_stream(stdout, "stdout", &out_handle, &out_lines);
    });
    let err_handle = app.clone();
    let err_lines = Arc::clone(&lines);
    let err_thread = thread::spawn(move || {
        read_stream(stderr, "stderr", &err_handle, &err_lines);
    });

    let status = child.wait().map_err(|error| error.to_string())?;
    let _ = out_thread.join();
    let _ = err_thread.join();
    let output = lines
        .lock()
        .map(|lines| lines.join("\n"))
        .unwrap_or_default();

    Ok(RemoteTriggerResult {
        command,
        exit_code: status.code().unwrap_or(-1),
        output,
    })
}

fn read_stream<R: Read>(reader: R, stream: &str, app: &AppHandle, lines: &Arc<Mutex<Vec<String>>>) {
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        match reader.read_until(b'\n', &mut buffer) {
            Ok(0) => break,
            Ok(_) => {
                let line = String::from_utf8_lossy(&buffer)
                    .trim_end_matches(['\r', '\n'])
                    .to_string();
                if let Ok(mut output) = lines.lock() {
                    output.push(if stream == "stderr" {
                        format!("[stderr] {line}")
                    } else {
                        line.clone()
                    });
                }
                let _ = app.emit(
                    "remote-trigger-output",
                    RemoteTriggerOutput {
                        stream: stream.to_string(),
                        line,
                    },
                );
            }
            Err(_) => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> RemoteTriggerConfig {
        RemoteTriggerConfig {
            host: "10.255.240.160".into(),
            user: "EGOV22".into(),
            wrapper: DEFAULT_WRAPPER.into(),
            ..RemoteTriggerConfig::default()
        }
    }

    #[test]
    fn normalizes_and_validates_config() {
        let mut config = RemoteTriggerConfig {
            host: "  10.255.240.160  ".into(),
            user: "  ".into(),
            port: 0,
            ..RemoteTriggerConfig::default()
        };
        config.normalize();
        assert_eq!(config.host, "10.255.240.160");
        assert!(!config.user.is_empty());
        assert_eq!(config.port, 22);
        assert!(config.validate().is_ok());
    }

    #[test]
    fn saves_and_loads_config() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data/trigger-client.json");
        let config = config();
        save_at(&path, &config).unwrap();
        let loaded = load_at(&path).unwrap();
        assert_eq!(loaded.host, "10.255.240.160");
        assert_eq!(loaded.user, "EGOV22");
        assert_eq!(loaded.wrapper, DEFAULT_WRAPPER);
    }

    #[test]
    fn builds_wrapper_preview_with_batch_mode() {
        let config = config();
        let args = with_options(&config, Some("opencode-quota".into()), true).unwrap();
        let preview = sm_trigger::preview(&args);
        assert!(preview.contains("BatchMode=yes"));
        assert!(preview.contains("ConnectTimeout=10"));
        assert!(preview.contains("Invoke-UpdaterTask.ps1"));
        assert!(preview.contains("-App opencode-quota"));
        assert!(preview.contains("-Check"));
    }

    #[test]
    fn builds_direct_preview_without_wrapper() {
        let mut config = config();
        config.wrapper.clear();
        let args = with_options(&config, None, false).unwrap();
        let preview = sm_trigger::preview(&args);
        assert!(preview.contains("sm-updater.exe run --all"));
        assert!(!preview.contains("Invoke-UpdaterTask.ps1"));
    }

    #[test]
    fn password_mode_uses_password_auth_options() {
        let mut config = config();
        config.auth_mode = "password".into();
        config.password = "secret".into();
        let args = with_options(&config, None, false).unwrap();
        let preview = sm_trigger::preview(&args);
        assert!(preview.contains("PreferredAuthentications=password,keyboard-interactive"));
        assert!(preview.contains("NumberOfPasswordPrompts=1"));
        assert!(!preview.contains("BatchMode=yes"));
        assert!(config.validate_for_run().is_ok());
    }

    #[test]
    fn password_is_encrypted_with_dpapi_and_loaded_back() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data/trigger-client.json");
        let mut config = config();
        config.auth_mode = "password".into();
        config.password = "super-secret".into();
        config.remember_password = true;
        save_at(&path, &config).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("super-secret"));
        assert!(!raw.contains("\"password\":"));
        let loaded = load_at(&path).unwrap();
        assert_eq!(loaded.password, "super-secret");
        assert!(loaded.password_protected.len() > 20);
    }

    #[test]
    fn password_not_remembered_is_not_persisted() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("data/trigger-client.json");
        let mut config = config();
        config.auth_mode = "password".into();
        config.password = "super-secret".into();
        config.remember_password = false;
        save_at(&path, &config).unwrap();
        let loaded = load_at(&path).unwrap();
        assert!(loaded.password.is_empty());
        assert!(loaded.password_protected.is_empty());
    }

    #[test]
    fn password_mode_requires_password_for_run() {
        let mut config = config();
        config.auth_mode = "password".into();
        config.password.clear();
        assert!(config.validate_for_run().is_err());
    }

    #[test]
    fn missing_host_is_rejected() {
        let mut config = config();
        config.host.clear();
        assert!(config.validate().is_err());
    }

    #[test]
    fn empty_config_uses_default_remote_bin() {
        let config = RemoteTriggerConfig::default();
        assert_eq!(config.remote_bin, DEFAULT_REMOTE_BIN);
        assert_eq!(config.port, 22);
    }
}
