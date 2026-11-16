use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use sm_config::ConfigPaths;
use sm_trigger::{Args as TriggerArgs, DEFAULT_REMOTE_BIN};
use tauri::{AppHandle, Emitter};

const DEFAULT_WRAPPER: &str = r"C:\tools\ServiceManager\scripts\Invoke-UpdaterTask.ps1";

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

impl Default for RemoteTriggerConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            user: sm_trigger::default_user().unwrap_or_default(),
            port: 22,
            identity: String::new(),
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
    Ok(config)
}

fn save_at(path: &Path, config: &RemoteTriggerConfig) -> Result<(), String> {
    let mut config = config.clone();
    config.normalize();
    config.validate()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let json = serde_json::to_string_pretty(&config).map_err(|error| error.to_string())?;
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, json).map_err(|error| error.to_string())?;
    std::fs::rename(&temp, path).map_err(|error| error.to_string())
}

fn with_options(config: &RemoteTriggerConfig, app: Option<String>, check: bool) -> TriggerArgs {
    let mut config = config.clone();
    config.normalize();
    TriggerArgs {
        host: config.host,
        user: config.user,
        port: config.port,
        identity: non_empty(config.identity),
        ssh_options: vec!["BatchMode=yes".to_string(), "ConnectTimeout=10".to_string()],
        remote_bin: config.remote_bin,
        wrapper: non_empty(config.wrapper),
        config: non_empty(config.remote_config),
        app,
        check,
        dry_run: false,
        help: false,
    }
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
    Ok(sm_trigger::preview(&with_options(&config, app, check)))
}

#[tauri::command]
pub async fn test_remote_ssh(config: RemoteTriggerConfig) -> Result<RemoteTriggerResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut config = config;
        config.normalize();
        config.validate()?;
        let args = with_options(&config, None, false);
        let command = sm_trigger::preview_with(&args, "echo sm-trigger-ok");
        let (program, ssh_args) = sm_trigger::ssh_program_args_with(&args, "echo sm-trigger-ok");
        run_capture(&program, &ssh_args, command)
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
    config.validate()?;
    let args = with_options(&config, app_id, check);
    let command = sm_trigger::preview(&args);
    let (program, ssh_args) = sm_trigger::ssh_program_args(&args);

    tauri::async_runtime::spawn_blocking(move || run_streaming(&app, &program, &ssh_args, command))
        .await
        .map_err(|error| error.to_string())?
}

fn run_capture(
    program: &str,
    args: &[String],
    command: String,
) -> Result<RemoteTriggerResult, String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
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
    command: String,
) -> Result<RemoteTriggerResult, String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
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
        let args = with_options(&config, Some("opencode-quota".into()), true);
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
        let args = with_options(&config, None, false);
        let preview = sm_trigger::preview(&args);
        assert!(preview.contains("sm-updater.exe run --all"));
        assert!(!preview.contains("Invoke-UpdaterTask.ps1"));
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
