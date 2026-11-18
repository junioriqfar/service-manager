use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sm_config::ConfigPaths;
use sm_updater::ip::client_ip_from_env;
use sm_updater::registry::{load_config, TriggerSettings};
use sm_updater::retention::{prune_logs, prune_project_logs, PruneResult, DEFAULT_ACTIVE_GRACE};
use sm_updater::runner::{wrap_command, Output, Runner, SystemRunner};
use sm_updater::settings::{
    effective_settings, load_settings, save_settings, settings_path_for, Settings, SettingsTrigger,
};
use sm_updater::trigger::default_log_dir;
use tauri::AppHandle;

use crate::apache::{self, ApacheChangeView};
use crate::php_paths::{self, PhpPathChangeView};

const TASK_SCRIPT_INSTALL: &str = "Install-UpdaterTask.ps1";
const TASK_SCRIPT_TEST: &str = "Invoke-UpdaterTask.ps1";
const TASK_QUERY_TIMEOUT: Duration = Duration::from_secs(30);
const TASK_OPERATION_TIMEOUT: Duration = Duration::from_secs(180);
const TOOL_TEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_VERSION_CHARS: usize = 128;
const MAX_TASK_OUTPUT_CHARS: usize = 20_000;

pub(crate) const TOOL_SPECS: &[ToolSpec] = &[
    ToolSpec {
        name: "git",
        label: "Git",
        binaries: &["git.exe"],
        version_args: &["--version"],
        required: true,
    },
    ToolSpec {
        name: "php",
        label: "PHP",
        binaries: &["php.exe"],
        version_args: &["-v"],
        required: true,
    },
    ToolSpec {
        name: "composer",
        label: "Composer",
        binaries: &["composer.bat", "composer.exe"],
        version_args: &["--version"],
        required: true,
    },
    ToolSpec {
        name: "node",
        label: "Node.js",
        binaries: &["node.exe"],
        version_args: &["--version"],
        required: false,
    },
    ToolSpec {
        name: "npm",
        label: "npm",
        binaries: &["npm.cmd", "npm.exe"],
        version_args: &["--version"],
        required: true,
    },
    ToolSpec {
        name: "pnpm",
        label: "pnpm",
        binaries: &["pnpm.exe", "pnpm.cmd"],
        version_args: &["--version"],
        required: false,
    },
    ToolSpec {
        name: "yarn",
        label: "Yarn",
        binaries: &["yarn.cmd", "yarn.exe"],
        version_args: &["--version"],
        required: false,
    },
    ToolSpec {
        name: "bun",
        label: "Bun",
        binaries: &["bun.exe"],
        version_args: &["--version"],
        required: false,
    },
    ToolSpec {
        name: "go",
        label: "Go",
        binaries: &["go.exe"],
        version_args: &["version"],
        required: false,
    },
    ToolSpec {
        name: "apache",
        label: "Apache",
        binaries: &["httpd.exe"],
        version_args: &["-v"],
        required: false,
    },
    ToolSpec {
        name: "nssm",
        label: "NSSM",
        binaries: &["nssm.exe"],
        // NSSM has no CLI version flag; `nssm version` opens the GUI and never
        // exits. An empty version args list means "verify the binary only".
        version_args: &[],
        required: false,
    },
];

const TASK_QUERY_COMMAND: &str = r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$ErrorActionPreference = 'Stop'
$name = $env:SM_TASK_NAME
$task = Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
if ($null -eq $task) {
    [pscustomobject]@{ installed = $false; taskName = $name } | ConvertTo-Json -Compress
    exit 0
}
$info = $null
try { $info = Get-ScheduledTaskInfo -TaskName $name -ErrorAction SilentlyContinue } catch { }
$lastRun = ''
if ($null -ne $info -and $null -ne $info.LastRunTime) { $lastRun = $info.LastRunTime.ToString('o') }
$lastResult = 0
if ($null -ne $info -and $null -ne $info.LastTaskResult) { $lastResult = [long]$info.LastTaskResult }
$execute = ''
$arguments = ''
if ($task.Actions.Count -gt 0) {
    $execute = [string]$task.Actions[0].Execute
    $arguments = [string]$task.Actions[0].Arguments
}
$hiddenWscript = ($execute -match '(?i)wscript(\.exe)?$') -and ($arguments -match '(?i)Run-UpdaterTaskHidden\.vbs')
$windowHidden = [bool]$task.Settings.Hidden -and $hiddenWscript
[pscustomobject]@{
    installed = $true
    taskName = $name
    state = [string]$task.State
    runLevel = [string]$task.Principal.RunLevel
    user = [string]$task.Principal.UserId
    logonType = [string]$task.Principal.LogonType
    lastRun = $lastRun
    lastResult = $lastResult
    windowHidden = $windowHidden
} | ConvertTo-Json -Compress
"#;

const TASK_REMOVE_COMMAND: &str = r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$ErrorActionPreference = 'Stop'
$name = $env:SM_TASK_NAME
$task = Get-ScheduledTask -TaskName $name -ErrorAction SilentlyContinue
if ($null -eq $task) {
    [Console]::Error.WriteLine("Scheduled Task '$name' is not installed.")
    exit 1
}
Unregister-ScheduledTask -TaskName $name -Confirm:$false
Write-Output "Scheduled Task '$name' removed."
"#;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPathIssue {
    pub tool: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDiagnostics {
    pub missing_required_tools: Vec<String>,
    pub invalid_paths: Vec<ToolPathIssue>,
    pub task_installed: bool,
    pub task_message: String,
    pub task_error: Option<String>,
    pub legacy_fallback: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsView {
    pub settings: Settings,
    pub legacy_fallback: bool,
    /// `projects.json.trigger`; `None` bila `projects.json` tidak ada.
    pub legacy_trigger: Option<TriggerSettings>,
    /// IP client dari `SSH_CONNECTION`/`SSH_CLIENT`, jika ada.
    pub current_client_ip: Option<String>,
    pub diagnostics: SettingsDiagnostics,
}

/// Status maintenance efektif untuk banner tab Updater.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaintenanceView {
    pub enabled: bool,
    pub message: String,
}

/// Hasil Save Settings. `apachePending` berisi preview service/app yang perlu
/// di-apply terpisah; `phpPathChange` berisi referensi path PHP lama yang perlu
/// ditinjau user. Settings sendiri sudah tersimpan saat nilai ini dikembalikan.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsSaveResult {
    pub settings: Settings,
    pub apache_pending: Option<ApacheChangeView>,
    pub php_path_change: Option<PhpPathChangeView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDetectionView {
    pub tool: String,
    pub found: bool,
    pub success: bool,
    pub path: String,
    pub version: String,
    pub root: String,
    pub config: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolTestView {
    pub tool: String,
    pub path: String,
    pub version: String,
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskStatusView {
    pub task_name: String,
    pub installed: bool,
    pub state: String,
    pub run_level: String,
    pub user: String,
    pub logon_type: String,
    pub last_run: String,
    pub last_result: i64,
    /// True when the task setting `Hidden` is enabled *and* the action uses
    /// the hidden `wscript.exe` + `Run-UpdaterTaskHidden.vbs` launcher.
    pub window_hidden: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskOperationView {
    pub success: bool,
    pub exit_code: i32,
    pub output: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskStatusJson {
    #[serde(default)]
    installed: bool,
    #[serde(default)]
    task_name: String,
    #[serde(default)]
    state: String,
    #[serde(default)]
    run_level: String,
    #[serde(default)]
    user: String,
    #[serde(default)]
    logon_type: String,
    #[serde(default)]
    last_run: String,
    #[serde(default)]
    last_result: i64,
    #[serde(default)]
    window_hidden: bool,
}

pub(crate) struct ToolSpec {
    pub(crate) name: &'static str,
    pub(crate) label: &'static str,
    pub(crate) binaries: &'static [&'static str],
    pub(crate) version_args: &'static [&'static str],
    pub(crate) required: bool,
}

fn tool_spec(name: &str) -> Result<&'static ToolSpec, String> {
    let name = name.trim();
    TOOL_SPECS
        .iter()
        .find(|spec| spec.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| {
            let supported = TOOL_SPECS
                .iter()
                .map(|spec| spec.name)
                .collect::<Vec<_>>()
                .join(", ");
            format!("Unsupported tool '{name}'. Supported tools: {supported}.")
        })
}

fn validate_path_text(path: &str) -> Result<(), String> {
    if path.chars().any(char::is_control) {
        return Err("Path contains control characters.".to_string());
    }
    Ok(())
}

fn validate_task_name(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Err("Scheduled Task name is required.".to_string());
    }
    if value.chars().count() > 200 {
        return Err("Scheduled Task name is too long (maximum 200 characters).".to_string());
    }
    if !value
        .chars()
        .all(|character| character.is_alphanumeric() || matches!(character, ' ' | '.' | '_' | '-'))
    {
        return Err(
            "Scheduled Task name may only contain letters, numbers, spaces, dots, underscores, and hyphens."
                .to_string(),
        );
    }
    Ok(value.to_string())
}

fn validate_user_id(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(String::new());
    }
    if value.chars().count() > 256 {
        return Err("User name is too long (maximum 256 characters).".to_string());
    }
    if !value.chars().all(|character| {
        character.is_alphanumeric() || matches!(character, ' ' | '.' | '_' | '-' | '\\' | '@' | '$')
    }) {
        return Err(
            "User name may only contain letters, numbers, spaces, dots, underscores, hyphens, backslashes, @, and $."
                .to_string(),
        );
    }
    Ok(value.to_string())
}

fn validate_logon_type(value: &str) -> Result<String, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "interactive" => Ok("Interactive".to_string()),
        "s4u" => Ok("S4U".to_string()),
        "password" => Err(
            "Password logon type needs a credential and is not supported from the GUI. Use Interactive or S4U."
                .to_string(),
        ),
        other => Err(format!(
            "Unsupported logon type '{other}'. Use Interactive or S4U."
        )),
    }
}

fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| {
            line.chars()
                .filter(|character| !character.is_control())
                .take(MAX_VERSION_CHARS)
                .collect()
        })
        .unwrap_or_default()
}

fn truncate_output(text: &str) -> String {
    if text.chars().count() <= MAX_TASK_OUTPUT_CHARS {
        return text.to_string();
    }
    let mut truncated: String = text.chars().take(MAX_TASK_OUTPUT_CHARS).collect();
    truncated.push_str("\n...(output truncated)");
    truncated
}

fn env_dir(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn push_program_files(candidates: &mut Vec<PathBuf>, base: &Option<PathBuf>, relative: &str) {
    if let Some(base) = base {
        candidates.push(base.join(relative));
    }
}

fn latest_matching(root: &Path, prefix: &str, executable: &Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    let mut directories: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.is_dir())
        .collect();
    directories.sort_by(|left, right| {
        right
            .to_string_lossy()
            .to_ascii_lowercase()
            .cmp(&left.to_string_lossy().to_ascii_lowercase())
    });
    for directory in directories {
        let name = directory
            .file_name()
            .map(|name| name.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if !prefix.is_empty() && !name.starts_with(&prefix.to_ascii_lowercase()) {
            continue;
        }
        let candidate = directory.join(executable);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn common_candidates(tool: &str) -> Vec<PathBuf> {
    let program_files = env_dir("ProgramFiles");
    let program_files_x86 = env_dir("ProgramFiles(x86)");
    let user_profile = env_dir("USERPROFILE");
    let local_app_data = env_dir("LOCALAPPDATA");
    let app_data = env_dir("APPDATA");
    let program_data = env_dir("ProgramData");
    let mut candidates = Vec::new();

    match tool {
        "git" => {
            push_program_files(&mut candidates, &program_files, r"Git\cmd\git.exe");
            push_program_files(&mut candidates, &program_files, r"Git\bin\git.exe");
            push_program_files(&mut candidates, &program_files_x86, r"Git\cmd\git.exe");
        }
        "php" => {
            if let Some(path) = latest_matching(
                Path::new(r"C:\laragon\bin\php"),
                "php-",
                Path::new("php.exe"),
            ) {
                candidates.push(path);
            }
            candidates.push(PathBuf::from(r"C:\php\php.exe"));
            push_program_files(&mut candidates, &program_files, r"PHP\php.exe");
        }
        "composer" => {
            candidates.push(PathBuf::from(r"C:\laragon\bin\composer\composer.bat"));
            if let Some(profile) = &user_profile {
                candidates.push(profile.join(r"AppData\Roaming\Composer\vendor\bin\composer.bat"));
            }
            push_program_files(
                &mut candidates,
                &program_data,
                r"ComposerSetup\bin\composer.bat",
            );
            push_program_files(
                &mut candidates,
                &app_data,
                r"Composer\vendor\bin\composer.bat",
            );
        }
        "node" => {
            push_program_files(&mut candidates, &program_files, r"nodejs\node.exe");
            push_program_files(&mut candidates, &program_files_x86, r"nodejs\node.exe");
        }
        "npm" => {
            push_program_files(&mut candidates, &program_files, r"nodejs\npm.cmd");
            push_program_files(&mut candidates, &program_files_x86, r"nodejs\npm.cmd");
        }
        "pnpm" => {
            push_program_files(&mut candidates, &program_files, r"nodejs\pnpm.cmd");
            push_program_files(&mut candidates, &local_app_data, r"pnpm\pnpm.exe");
            push_program_files(&mut candidates, &app_data, r"npm\pnpm.cmd");
        }
        "yarn" => {
            push_program_files(&mut candidates, &program_files, r"nodejs\yarn.cmd");
            push_program_files(&mut candidates, &app_data, r"npm\yarn.cmd");
        }
        "bun" => {
            push_program_files(&mut candidates, &user_profile, r".bun\bin\bun.exe");
        }
        "go" => {
            push_program_files(&mut candidates, &program_files, r"Go\bin\go.exe");
            candidates.push(PathBuf::from(r"C:\Go\bin\go.exe"));
        }
        "apache" => {
            if let Some(path) = latest_matching(
                Path::new(r"C:\laragon\bin\apache"),
                "httpd-",
                Path::new(r"bin\httpd.exe"),
            ) {
                candidates.push(path);
            }
            candidates.push(PathBuf::from(r"C:\Apache24\bin\httpd.exe"));
            candidates.push(PathBuf::from(r"C:\xampp\apache\bin\httpd.exe"));
        }
        "nssm" => {
            candidates.push(PathBuf::from(r"C:\tools\nssm\nssm.exe"));
        }
        _ => {}
    }

    candidates
}

fn candidates_from_current(spec: &ToolSpec, current: &str) -> Vec<PathBuf> {
    let current = current.trim();
    if current.is_empty() {
        return Vec::new();
    }
    let path = Path::new(current);
    let mut candidates = Vec::new();
    if path.is_file() {
        candidates.push(path.to_path_buf());
        return candidates;
    }
    if path.is_dir() {
        for binary in spec.binaries {
            let candidate = path.join(binary);
            if candidate.is_file() {
                candidates.push(candidate);
            }
        }
        if spec.name == "apache" {
            let candidate = path.join(r"bin\httpd.exe");
            if candidate.is_file() {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn resolve_tool_path_with(
    spec: &ToolSpec,
    current: Option<&str>,
    path_directories: &[PathBuf],
    common: &[PathBuf],
) -> Option<PathBuf> {
    if let Some(current) = current {
        if let Some(candidate) = candidates_from_current(spec, current).into_iter().next() {
            return Some(candidate);
        }
    }
    for directory in path_directories {
        for binary in spec.binaries {
            let candidate = directory.join(binary);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    common.iter().find(|candidate| candidate.is_file()).cloned()
}

fn resolve_tool_path(spec: &ToolSpec, current: Option<&str>) -> Option<PathBuf> {
    let path_directories: Vec<PathBuf> = env::var_os("PATH")
        .map(|value| env::split_paths(&value).collect())
        .unwrap_or_default();
    let common = common_candidates(spec.name);
    resolve_tool_path_with(spec, current, &path_directories, &common)
}

fn apache_details(spec: &ToolSpec, path: &Path) -> (String, String) {
    if spec.name != "apache" {
        return (String::new(), String::new());
    }
    let root = path
        .parent()
        .and_then(|parent| {
            let is_bin = parent
                .file_name()
                .map(|name| name.eq_ignore_ascii_case("bin"))
                .unwrap_or(false);
            if is_bin {
                parent.parent()
            } else {
                Some(parent)
            }
        })
        .map(Path::to_path_buf);
    let Some(root) = root else {
        return (String::new(), String::new());
    };
    let config = if root.join("conf").is_dir() {
        "conf/httpd.conf".to_string()
    } else {
        String::new()
    };
    (root.to_string_lossy().to_string(), config)
}

fn test_tool_path(spec: &ToolSpec, path: &Path) -> ToolTestView {
    test_tool_path_with_timeout(spec, path, TOOL_TEST_TIMEOUT)
}

pub(crate) fn test_tool_path_with_timeout(
    spec: &ToolSpec,
    path: &Path,
    timeout: Duration,
) -> ToolTestView {
    let path_text = path.to_string_lossy().to_string();
    if spec.version_args.is_empty() {
        // Some tools (NSSM) have no non-interactive version command. Running
        // them just opens a GUI and times out, so only verify the binary.
        return ToolTestView {
            tool: spec.name.to_string(),
            path: path_text,
            version: "installed".to_string(),
            success: true,
            output: format!(
                "{} is installed; this tool has no CLI version check.",
                spec.label
            ),
            error: None,
        };
    }
    let cwd = path
        .parent()
        .filter(|parent| parent.is_dir())
        .unwrap_or_else(|| Path::new("."));
    let args: Vec<String> = spec
        .version_args
        .iter()
        .map(|argument| argument.to_string())
        .collect();
    let (program, args) = wrap_command(path, &args);

    match SystemRunner.run(&program, &args, cwd, timeout) {
        Ok(output) => {
            let combined = output.combined();
            let success = output.success();
            let error = if success {
                None
            } else if combined.trim().is_empty() {
                Some(format!("Exited with code {}.", output.exit_code))
            } else {
                Some(combined.clone())
            };
            ToolTestView {
                tool: spec.name.to_string(),
                path: path_text,
                version: first_line(&combined),
                success,
                output: combined,
                error,
            }
        }
        Err(error) => ToolTestView {
            tool: spec.name.to_string(),
            path: path_text,
            version: String::new(),
            success: false,
            output: String::new(),
            error: Some(error.to_string()),
        },
    }
}

fn detect_tool_at(tool: &str, current: Option<&str>) -> Result<ToolDetectionView, String> {
    let spec = tool_spec(tool)?;
    if let Some(current) = current {
        validate_path_text(current)?;
    }
    let Some(path) = resolve_tool_path(spec, current) else {
        return Ok(ToolDetectionView {
            tool: spec.name.to_string(),
            found: false,
            success: false,
            path: String::new(),
            version: String::new(),
            root: String::new(),
            config: String::new(),
            message: format!(
                "{} was not found. Checked the configured path, PATH, and common install locations.",
                spec.label
            ),
        });
    };

    let test = test_tool_path(spec, &path);
    let (root, config) = apache_details(spec, &path);
    let message = if test.success {
        format!("Detected {} at {}.", spec.label, test.path)
    } else {
        format!(
            "Found {} at {}, but the version check failed: {}",
            spec.label,
            test.path,
            test.error.as_deref().unwrap_or("unknown error")
        )
    };
    Ok(ToolDetectionView {
        tool: spec.name.to_string(),
        found: true,
        success: test.success,
        path: test.path,
        version: test.version,
        root,
        config,
        message,
    })
}

fn test_tool_at(tool: &str, path: &str) -> Result<ToolTestView, String> {
    let spec = tool_spec(tool)?;
    let path = path.trim();
    validate_path_text(path)?;
    if path.is_empty() {
        return Err(format!("{} path is required.", spec.label));
    }
    let candidate = Path::new(path);
    if !candidate.is_file() {
        return Err(format!("{} executable not found: {path}", spec.label));
    }
    Ok(test_tool_path(spec, candidate))
}

pub(crate) fn settings_tool_path<'a>(settings: &'a Settings, tool: &str) -> &'a str {
    match tool {
        "git" => &settings.tools.git.path,
        "php" => &settings.tools.php.path,
        "composer" => &settings.tools.composer.path,
        "node" => &settings.tools.node.path,
        "npm" => &settings.tools.npm.path,
        "pnpm" => &settings.tools.pnpm.path,
        "yarn" => &settings.tools.yarn.path,
        "bun" => &settings.tools.bun.path,
        "go" => &settings.tools.go.path,
        "apache" => settings.tools.apache.path(),
        "nssm" => &settings.tools.nssm.path,
        _ => "",
    }
}

fn build_diagnostics(
    settings: &Settings,
    legacy_fallback: bool,
    task: Option<Result<TaskStatusView, String>>,
) -> SettingsDiagnostics {
    let mut missing_required_tools = Vec::new();
    let mut invalid_paths = Vec::new();

    for spec in TOOL_SPECS {
        let path = settings_tool_path(settings, spec.name).trim();
        if path.is_empty() {
            if spec.required {
                missing_required_tools.push(spec.label.to_string());
            }
            continue;
        }
        if !Path::new(path).is_file() {
            invalid_paths.push(ToolPathIssue {
                tool: spec.name.to_string(),
                path: path.to_string(),
                message: format!("{} path not found: {path}", spec.label),
            });
        }
    }

    let (task_installed, task_message, task_error) = match task {
        Some(Ok(status)) if status.installed => (
            true,
            format!(
                "Scheduled Task '{}' is installed (state: {}).",
                status.task_name, status.state
            ),
            None,
        ),
        Some(Ok(status)) => (
            false,
            format!(
                "Scheduled Task '{}' is not installed. Install it to run privileged updates.",
                status.task_name
            ),
            None,
        ),
        Some(Err(error)) => (
            false,
            "Scheduled Task status is unavailable.".to_string(),
            Some(error),
        ),
        None => (
            false,
            "Scheduled Task status was not checked.".to_string(),
            None,
        ),
    };

    SettingsDiagnostics {
        missing_required_tools,
        invalid_paths,
        task_installed,
        task_message,
        task_error,
        legacy_fallback,
    }
}

fn build_view(
    settings: Settings,
    legacy_fallback: bool,
    legacy_trigger: Option<TriggerSettings>,
    task: Option<Result<TaskStatusView, String>>,
) -> SettingsView {
    let diagnostics = build_diagnostics(&settings, legacy_fallback, task);
    SettingsView {
        settings,
        legacy_fallback,
        legacy_trigger,
        current_client_ip: client_ip_from_env().map(|ip| ip.to_string()),
        diagnostics,
    }
}

fn load_settings_meta_at(
    settings_file: &Path,
    projects_file: &Path,
) -> Result<(Settings, bool, Option<TriggerSettings>), String> {
    let config = load_config(projects_file)?;
    let legacy_fallback = load_settings(settings_file)
        .map_err(|error| error.to_string())?
        .is_none();
    let settings = effective_settings(settings_file, &config.settings, &config.trigger)
        .map_err(|error| error.to_string())?;
    let legacy_trigger = projects_file.is_file().then_some(config.trigger);
    Ok((settings, legacy_fallback, legacy_trigger))
}

fn load_settings_at(
    settings_file: &Path,
    projects_file: &Path,
) -> Result<(Settings, bool), String> {
    let (settings, legacy_fallback, _) = load_settings_meta_at(settings_file, projects_file)?;
    Ok((settings, legacy_fallback))
}

fn save_settings_at(settings_file: &Path, mut settings: Settings) -> Result<Settings, String> {
    settings.normalize();
    save_settings(settings_file, &settings).map_err(|error| error.to_string())?;
    Ok(settings)
}

fn require_json_extension(path: &Path) -> Result<(), String> {
    let is_json = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.eq_ignore_ascii_case("json"))
        .unwrap_or(false);
    if !is_json {
        return Err(format!(
            "Settings file must use a .json extension: {}",
            path.display()
        ));
    }
    Ok(())
}

/// Export effective settings to a user-selected JSON file.
///
/// Only settings values are written: no registry apps, service config, logs,
/// history, or secrets. The destination is written atomically (`save_settings`).
pub(crate) fn export_settings_at(
    settings_file: &Path,
    projects_file: &Path,
    destination: &Path,
) -> Result<(), String> {
    require_json_extension(destination)?;
    if destination.is_dir() {
        return Err(format!(
            "Export destination is a folder, not a file: {}",
            destination.display()
        ));
    }
    let config = load_config(projects_file)?;
    let mut settings = effective_settings(settings_file, &config.settings, &config.trigger)
        .map_err(|error| error.to_string())?;
    settings.normalize();
    save_settings(destination, &settings).map_err(|error| error.to_string())
}

/// Read, parse, normalize, and validate a settings file without writing
/// anything. The caller decides whether to apply it as a draft.
pub(crate) fn load_settings_from_file_at(path: &Path) -> Result<Settings, String> {
    require_json_extension(path)?;
    match load_settings(path).map_err(|error| error.to_string())? {
        Some(settings) => Ok(settings),
        None => Err(format!(
            "Settings file is empty or missing: {}",
            path.display()
        )),
    }
}

fn maintenance_status_at(
    settings_file: &Path,
    projects_file: &Path,
) -> Result<MaintenanceView, String> {
    let config = load_config(projects_file)?;
    let settings = effective_settings(settings_file, &config.settings, &config.trigger)
        .map_err(|error| error.to_string())?;
    Ok(MaintenanceView {
        enabled: settings.maintenance.enabled,
        message: settings.maintenance.message,
    })
}

/// Backup `settings.json` sebelum migrasi trigger.
fn backup_settings_file(settings_file: &Path) -> Result<Option<PathBuf>, String> {
    if !settings_file.is_file() {
        return Ok(None);
    }
    let base = settings_file
        .parent()
        .and_then(|parent| parent.parent())
        .unwrap_or_else(|| Path::new("."));
    let backup_dir = base.join("backup");
    std::fs::create_dir_all(&backup_dir).map_err(|error| error.to_string())?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f");
    let destination = backup_dir.join(format!("settings-{stamp}.json"));
    std::fs::copy(settings_file, &destination).map_err(|error| error.to_string())?;
    Ok(Some(destination))
}

fn save_settings_view_at(
    paths: &ConfigPaths,
    settings: Settings,
) -> Result<SettingsSaveResult, String> {
    let settings_file = settings_path_for(&paths.projects_path());
    // Nilai efektif lama harus dibaca sebelum settings.json ditulis ulang.
    let (old_settings, _) = load_settings_at(&settings_file, &paths.projects_path())?;
    let config = load_config(&paths.projects_path())?;
    let old_php_path = old_settings.tools.php.path.trim().to_string();
    let old_php_root = php_paths::php_home(&old_php_path);

    if settings_file.is_file() {
        backup_settings_file(&settings_file)?;
    }
    let settings = save_settings_at(&settings_file, settings)?;

    let new_php_path = settings.tools.php.path.trim().to_string();
    let new_php_root = php_paths::php_home(&new_php_path);
    let php_path_change = if php_paths::php_path_changed(&old_php_path, &new_php_path) {
        let references = php_paths::scan_php_references(&config, &old_php_path, &old_php_root);
        if references.is_empty() {
            None
        } else {
            Some(PhpPathChangeView {
                old_path: old_php_path,
                old_root: old_php_root,
                new_path: new_php_path,
                new_root: new_php_root,
                references,
            })
        }
    } else {
        None
    };

    let pending_path = apache::pending_path_for(&paths.data_dir());
    let pending =
        if apache::apache_settings_changed(&old_settings.tools.apache, &settings.tools.apache) {
            let loaded =
                sm_config::load_services_lenient(&paths.services_path(), &paths.backup_dir());
            apache::build_pending(
                &old_settings.tools.apache,
                &settings.tools.apache,
                &loaded.services,
                &config,
            )
        } else {
            None
        };

    let apache_pending = match pending {
        Some(pending) => {
            apache::save_pending_at(&pending_path, &pending)?;
            Some(pending.view())
        }
        None => {
            apache::clear_pending_at(&pending_path)?;
            None
        }
    };

    Ok(SettingsSaveResult {
        settings,
        apache_pending,
        php_path_change,
    })
}

/// Migrasi eksplisit `projects.json.trigger` -> `settings.json.trigger`.
///
/// `projects.json` tidak diubah. Trigger dari `projects.json` selalu menang
/// sebagai sumber migrasi agar nilai yang disalin persis seperti legacy.
/// Jika `settings.json` sudah ada, file lama dibackup terlebih dahulu.
fn migrate_trigger_at(settings_file: &Path, projects_file: &Path) -> Result<Settings, String> {
    let config = load_config(projects_file)?;
    let mut settings = effective_settings(settings_file, &config.settings, &config.trigger)
        .map_err(|error| error.to_string())?;
    settings.trigger = SettingsTrigger {
        allowed_client_ips: config.trigger.allowed_client_ips.clone(),
        allow_local: config.trigger.allow_local,
    };
    backup_settings_file(settings_file)?;
    save_settings_at(settings_file, settings)
}

/// Manual retention helper: prune updater logs plus every registered app's
/// Laravel `storage/logs`. `days == 0` disables retention explicitly.
pub(crate) fn prune_logs_at(
    config_path: &Path,
    log_dir: &Path,
    days: u32,
) -> Result<PruneResult, String> {
    if days == 0 {
        return Err("Log retention is disabled (0 days).".to_string());
    }
    let config = load_config(config_path)?;
    let mut result = prune_logs(log_dir, days, DEFAULT_ACTIVE_GRACE);
    result.merge(prune_project_logs(&config.apps, days, DEFAULT_ACTIVE_GRACE));
    Ok(result)
}

fn prune_logs_now_at(config_path: &Path) -> Result<PruneResult, String> {
    let config = load_config(config_path)?;
    let settings = effective_settings(
        &settings_path_for(config_path),
        &config.settings,
        &config.trigger,
    )
    .map_err(|error| error.to_string())?;
    let log_dir = if settings.updater.log_dir.trim().is_empty() {
        default_log_dir(config_path)
    } else {
        PathBuf::from(settings.updater.log_dir.trim())
    };
    prune_logs_at(config_path, &log_dir, settings.updater.log_retention_days)
}

/// Manual log cleanup. No Administrator privileges are required and no
/// automatic prune runs during updates.
#[tauri::command]
pub async fn prune_logs_now() -> Result<PruneResult, String> {
    let config_path = ConfigPaths::detect().projects_path();
    tauri::async_runtime::spawn_blocking(move || prune_logs_now_at(&config_path))
        .await
        .map_err(|error| error.to_string())?
}

fn powershell_exe() -> PathBuf {
    env::var_os("SystemRoot")
        .map(PathBuf::from)
        .map(|root| root.join(r"System32\WindowsPowerShell\v1.0\powershell.exe"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("powershell.exe"))
}

fn find_scripts_dir() -> Result<PathBuf, String> {
    let mut candidates = Vec::new();
    if let Ok(home) = env::var("SERVICE_MANAGER_HOME") {
        if !home.trim().is_empty() {
            candidates.push(Path::new(home.trim()).join("scripts"));
        }
    }
    if let Ok(executable) = env::current_exe() {
        if let Some(parent) = executable.parent() {
            for ancestor in parent.ancestors() {
                candidates.push(ancestor.join("scripts"));
            }
        }
    }
    candidates.push(ConfigPaths::detect().home().join("scripts"));
    candidates.push(PathBuf::from(r"C:\tools\ServiceManager\scripts"));

    candidates
        .into_iter()
        .find(|directory| {
            directory.join(TASK_SCRIPT_INSTALL).is_file()
                || directory.join(TASK_SCRIPT_TEST).is_file()
        })
        .ok_or_else(|| {
            "Scripts folder not found. Expected scripts/Install-UpdaterTask.ps1 next to the application."
                .to_string()
        })
}

fn script_path(name: &str) -> Result<PathBuf, String> {
    let path = find_scripts_dir()?.join(name);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("Script not found: {}", path.display()))
    }
}

fn run_powershell_command(
    command: &str,
    cwd: &Path,
    env: &BTreeMap<String, String>,
    timeout: Duration,
) -> Result<Output, String> {
    let args = vec![
        "-NoProfile".to_string(),
        "-NonInteractive".to_string(),
        "-Command".to_string(),
        command.to_string(),
    ];
    SystemRunner
        .run_with_env(&powershell_exe(), &args, cwd, timeout, env)
        .map_err(|error| format!("Failed to run PowerShell: {error}"))
}

fn run_powershell_script(script: &str, extra_args: &[String]) -> Result<Output, String> {
    let script = script_path(script)?;
    let cwd = script
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let mut args = vec![
        "-NoProfile".to_string(),
        "-NonInteractive".to_string(),
        "-ExecutionPolicy".to_string(),
        "Bypass".to_string(),
        "-File".to_string(),
        script.to_string_lossy().to_string(),
    ];
    args.extend(extra_args.iter().cloned());
    SystemRunner
        .run(&powershell_exe(), &args, &cwd, TASK_OPERATION_TIMEOUT)
        .map_err(|error| format!("Failed to run PowerShell: {error}"))
}

fn working_directory() -> PathBuf {
    env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn task_name_env(task_name: &str) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    env.insert("SM_TASK_NAME".to_string(), task_name.to_string());
    env
}

fn parse_task_status_json(task_name: &str, json: &str) -> Result<TaskStatusView, String> {
    let start = json
        .find('{')
        .ok_or_else(|| format!("Scheduled Task status output is not JSON: {}", json.trim()))?;
    let end = json
        .rfind('}')
        .filter(|end| *end >= start)
        .ok_or_else(|| format!("Scheduled Task status output is not JSON: {}", json.trim()))?;
    let raw: TaskStatusJson = serde_json::from_str(&json[start..=end])
        .map_err(|error| format!("Failed to parse Scheduled Task status JSON: {error}"))?;

    let installed = raw.installed;
    let fallback_task_name = if task_name.trim().is_empty() {
        raw.task_name.trim().to_string()
    } else {
        task_name.trim().to_string()
    };
    let state = if raw.state.trim().is_empty() {
        if installed {
            "Unknown".to_string()
        } else {
            "NotInstalled".to_string()
        }
    } else {
        raw.state.trim().to_string()
    };
    let last_run = raw.last_run.trim().to_string();
    let last_run = if last_run.starts_with("0001-01-01") {
        String::new()
    } else {
        last_run
    };

    Ok(TaskStatusView {
        task_name: fallback_task_name,
        installed,
        state,
        run_level: raw.run_level.trim().to_string(),
        user: raw.user.trim().to_string(),
        logon_type: raw.logon_type.trim().to_string(),
        last_run,
        last_result: raw.last_result,
        window_hidden: raw.window_hidden,
        error: None,
    })
}

pub(crate) fn task_status_blocking(task_name: &str) -> Result<TaskStatusView, String> {
    let task_name = validate_task_name(task_name)?;
    let output = run_powershell_command(
        TASK_QUERY_COMMAND,
        &working_directory(),
        &task_name_env(&task_name),
        TASK_QUERY_TIMEOUT,
    )?;
    if !output.success() {
        let text = output.combined();
        return Err(if text.trim().is_empty() {
            format!("PowerShell exited with code {}.", output.exit_code)
        } else {
            text
        });
    }
    parse_task_status_json(&task_name, &output.combined())
}

fn task_operation(output: Output) -> TaskOperationView {
    let text = output.combined();
    TaskOperationView {
        success: output.success(),
        exit_code: output.exit_code,
        output: if text.trim().is_empty() {
            "(no output)".to_string()
        } else {
            truncate_output(&text)
        },
    }
}

fn require_elevation(action: &str) -> Result<(), String> {
    match sm_win::is_elevated() {
        Ok(true) => Ok(()),
        Ok(false) => Err(format!(
            "Administrator privileges are required to {action}. Restart Service Manager as Administrator."
        )),
        Err(error) => Err(format!("Could not check Administrator privileges: {error}")),
    }
}

#[tauri::command]
pub async fn load_settings_view() -> Result<SettingsView, String> {
    let paths = ConfigPaths::detect();
    tauri::async_runtime::spawn_blocking(move || {
        let settings_file = settings_path_for(&paths.projects_path());
        let (settings, legacy_fallback, legacy_trigger) =
            load_settings_meta_at(&settings_file, &paths.projects_path())?;
        let task = task_status_blocking(&settings.privileged_execution.task_name);
        Ok(build_view(
            settings,
            legacy_fallback,
            legacy_trigger,
            Some(task),
        ))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Read-only maintenance status for the Updater banner.
#[tauri::command]
pub async fn load_maintenance_status() -> Result<MaintenanceView, String> {
    let paths = ConfigPaths::detect();
    tauri::async_runtime::spawn_blocking(move || {
        maintenance_status_at(
            &settings_path_for(&paths.projects_path()),
            &paths.projects_path(),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Save a copy of the effective settings. The dialog picks the destination;
/// the helper writes pretty JSON atomically and never exports registry apps.
#[tauri::command]
pub async fn export_settings_file(app: AppHandle) -> Result<Option<String>, String> {
    let default_name = format!(
        "settings-export-{}.json",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let selected = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .set_file_name(&default_name)
            .save_file();
        let _ = sender.send(selected);
    })
    .map_err(|error| error.to_string())?;
    let Some(destination) = receiver.await.map_err(|error| error.to_string())? else {
        return Ok(None);
    };

    let paths = ConfigPaths::detect();
    let settings_file = settings_path_for(&paths.projects_path());
    let projects_file = paths.projects_path();
    tauri::async_runtime::spawn_blocking(move || {
        export_settings_at(&settings_file, &projects_file, &destination)?;
        Ok(Some(destination.to_string_lossy().to_string()))
    })
    .await
    .map_err(|error| error.to_string())?
}

/// Load a settings file into a draft. Nothing is saved and no service or Apache
/// change is applied.
#[tauri::command]
pub async fn load_settings_file(app: AppHandle) -> Result<Option<Settings>, String> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        let selected = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .pick_file();
        let _ = sender.send(selected);
    })
    .map_err(|error| error.to_string())?;
    let Some(path) = receiver.await.map_err(|error| error.to_string())? else {
        return Ok(None);
    };
    tauri::async_runtime::spawn_blocking(move || load_settings_from_file_at(&path).map(Some))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn save_settings_view(settings: Settings) -> Result<SettingsSaveResult, String> {
    let paths = ConfigPaths::detect();
    save_settings_view_at(&paths, settings)
}

#[tauri::command]
pub async fn migrate_trigger_to_settings() -> Result<Settings, String> {
    let paths = ConfigPaths::detect();
    tauri::async_runtime::spawn_blocking(move || {
        let settings_file = settings_path_for(&paths.projects_path());
        migrate_trigger_at(&settings_file, &paths.projects_path())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn detect_tool(
    tool: String,
    current_path: Option<String>,
) -> Result<ToolDetectionView, String> {
    tauri::async_runtime::spawn_blocking(move || detect_tool_at(&tool, current_path.as_deref()))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn test_tool(tool: String, path: String) -> Result<ToolTestView, String> {
    tauri::async_runtime::spawn_blocking(move || test_tool_at(&tool, &path))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn task_status(task_name: String) -> Result<TaskStatusView, String> {
    tauri::async_runtime::spawn_blocking(move || task_status_blocking(&task_name))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn install_task(
    task_name: String,
    user_id: String,
    logon_type: String,
) -> Result<TaskOperationView, String> {
    let task_name = validate_task_name(&task_name)?;
    let user_id = validate_user_id(&user_id)?;
    let logon_type = validate_logon_type(&logon_type)?;
    require_elevation("install the updater Scheduled Task")?;

    tauri::async_runtime::spawn_blocking(move || {
        let mut args = vec![
            "-TaskName".to_string(),
            task_name,
            "-LogonType".to_string(),
            logon_type,
        ];
        if !user_id.is_empty() {
            args.push("-UserId".to_string());
            args.push(user_id);
        }
        let output = run_powershell_script(TASK_SCRIPT_INSTALL, &args)?;
        Ok(task_operation(output))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn remove_task(task_name: String) -> Result<TaskOperationView, String> {
    let task_name = validate_task_name(&task_name)?;
    require_elevation("remove the updater Scheduled Task")?;

    tauri::async_runtime::spawn_blocking(move || {
        let output = run_powershell_command(
            TASK_REMOVE_COMMAND,
            &working_directory(),
            &task_name_env(&task_name),
            TASK_OPERATION_TIMEOUT,
        )?;
        Ok(task_operation(output))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn test_task(task_name: Option<String>) -> Result<TaskOperationView, String> {
    let paths = ConfigPaths::detect();
    tauri::async_runtime::spawn_blocking(move || {
        let task_name = match task_name
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        {
            Some(value) => validate_task_name(&value)?,
            None => {
                let settings_file = settings_path_for(&paths.projects_path());
                let (settings, _) = load_settings_at(&settings_file, &paths.projects_path())?;
                validate_task_name(&settings.privileged_execution.task_name)?
            }
        };
        let args = vec![
            "-TaskName".to_string(),
            task_name,
            "-Check".to_string(),
            "-TimeoutSeconds".to_string(),
            "120".to_string(),
        ];
        let output = run_powershell_script(TASK_SCRIPT_TEST, &args)?;
        Ok(task_operation(output))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use sm_updater::settings::{ApacheToolPath, ToolPath};

    fn write_projects(path: &Path) {
        std::fs::write(
            path,
            r#"{
  "version": 2,
  "settings": {
    "gitPath": "C:\\legacy\\git.exe",
    "phpPath": "C:\\legacy\\php.exe",
    "composerPath": "C:\\legacy\\composer.bat",
    "npmPath": "C:\\legacy\\npm.cmd",
    "logDir": "C:\\legacy\\logs",
    "maxConcurrentUpdates": 2
  },
  "trigger": {
    "allowedClientIPs": ["10.0.0.5", "127.0.0.1"],
    "allowLocal": false
  },
  "apps": []
}"#,
        )
        .unwrap();
    }

    #[test]
    fn parses_installed_task_status_json() {
        let json = r#"{"installed":true,"taskName":"ServiceManager-Updater","state":"Ready","runLevel":"Highest","user":"DOMAIN\\User","logonType":"Interactive","lastRun":"2026-11-16T09:30:00.0000000+07:00","lastResult":0,"windowHidden":true}"#;
        let status = parse_task_status_json("ServiceManager-Updater", json).unwrap();

        assert!(status.installed);
        assert_eq!(status.task_name, "ServiceManager-Updater");
        assert_eq!(status.state, "Ready");
        assert_eq!(status.run_level, "Highest");
        assert_eq!(status.user, r"DOMAIN\User");
        assert_eq!(status.logon_type, "Interactive");
        assert!(status.last_run.contains("2026-11-16"));
        assert_eq!(status.last_result, 0);
        assert!(status.window_hidden);
        assert!(status.error.is_none());
    }

    #[test]
    fn task_query_recognizes_hidden_wscript_launcher() {
        assert!(TASK_QUERY_COMMAND.contains("Run-UpdaterTaskHidden"));
        assert!(TASK_QUERY_COMMAND.contains("wscript"));
        assert!(!TASK_QUERY_COMMAND.contains("-WindowStyle"));
    }

    #[test]
    fn parses_task_status_without_hidden_action_flag() {
        let json = r#"{"installed":true,"taskName":"ServiceManager-Updater","state":"Ready","lastResult":0}"#;
        let status = parse_task_status_json("ServiceManager-Updater", json).unwrap();
        assert!(status.installed);
        assert!(!status.window_hidden);
    }

    #[test]
    fn parses_not_installed_task_status_json_with_noise() {
        let json = "WARNING: task lookup\n{\"installed\":false,\"taskName\":\"Custom\"}\n";
        let status = parse_task_status_json("Custom", json).unwrap();

        assert!(!status.installed);
        assert_eq!(status.state, "NotInstalled");
        assert!(status.last_run.is_empty());
        assert_eq!(status.last_result, 0);
    }

    #[test]
    fn rejects_invalid_task_status_json() {
        let error = parse_task_status_json("Task", "not-json").unwrap_err();
        assert!(error.contains("JSON"));
    }

    #[test]
    fn validates_tool_task_user_and_logon_inputs() {
        assert_eq!(tool_spec("Git").unwrap().name, "git");
        assert!(tool_spec("powershell").is_err());
        assert_eq!(
            validate_task_name("ServiceManager-Updater").unwrap(),
            "ServiceManager-Updater"
        );
        assert!(validate_task_name(r"Folder\ServiceManager-Updater").is_err());
        assert!(validate_task_name("Task'; Unregister-ScheduledTask -TaskName x").is_err());
        assert!(validate_task_name("Task\nName").is_err());
        assert!(validate_task_name("   ").is_err());

        assert!(validate_user_id(r"DOMAIN\User").is_ok());
        assert!(validate_user_id("user@example.test").is_ok());
        assert!(validate_user_id("user'; Remove-Item C:\\").is_err());
        assert!(validate_user_id("user\nname").is_err());

        assert_eq!(validate_logon_type("interactive").unwrap(), "Interactive");
        assert_eq!(validate_logon_type("S4U").unwrap(), "S4U");
        assert!(validate_logon_type("password").is_err());
        assert!(validate_logon_type("unknown").is_err());
    }

    #[test]
    fn effective_settings_falls_back_to_legacy_projects() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let projects = data.join("projects.json");
        let settings_file = data.join("settings.json");
        write_projects(&projects);

        let (settings, legacy_fallback) = load_settings_at(&settings_file, &projects).unwrap();

        assert!(legacy_fallback);
        assert_eq!(settings.tools.git.path, r"C:\legacy\git.exe");
        assert_eq!(settings.tools.php.path, r"C:\legacy\php.exe");
        assert_eq!(settings.tools.composer.path, r"C:\legacy\composer.bat");
        assert_eq!(settings.tools.npm.path, r"C:\legacy\npm.cmd");
        assert_eq!(settings.updater.log_dir, r"C:\legacy\logs");
        assert_eq!(settings.updater.max_concurrent_updates, 2);
        assert_eq!(
            settings.trigger.allowed_client_ips,
            ["10.0.0.5", "127.0.0.1"]
        );
        assert!(!settings.trigger.allow_local);
        assert!(
            !settings_file.exists(),
            "fallback must not write settings.json"
        );
    }

    #[test]
    fn save_and_load_helpers_use_settings_file() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let settings_file = data.join("settings.json");
        let projects = data.join("projects.json");

        let mut settings = Settings::default();
        settings.updater.log_retention_days = 14;
        settings.tools.apache = sm_updater::settings::ApacheToolPath {
            tool: ToolPath::new(r"C:\apache\bin\httpd.exe"),
            root: r"C:\apache".to_string(),
            config: "conf/httpd.conf".to_string(),
        };

        let saved = save_settings_at(&settings_file, settings).unwrap();
        assert_eq!(saved.updater.log_retention_days, 14);
        assert!(settings_file.is_file());

        let (loaded, legacy_fallback) = load_settings_at(&settings_file, &projects).unwrap();
        assert!(!legacy_fallback);
        assert_eq!(loaded.tools.apache.root, r"C:\apache");
        assert_eq!(loaded.updater.log_retention_days, 14);
    }

    #[test]
    fn save_and_load_preserve_maintenance_settings() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        let settings_file = data.join("settings.json");
        let projects = data.join("projects.json");

        let mut settings = Settings::default();
        settings.maintenance.enabled = true;
        settings.maintenance.message = "  Deploy window  ".into();

        let saved = save_settings_at(&settings_file, settings).unwrap();
        assert!(saved.maintenance.enabled);
        assert_eq!(saved.maintenance.message, "Deploy window");

        let (loaded, legacy_fallback) = load_settings_at(&settings_file, &projects).unwrap();
        assert!(!legacy_fallback);
        assert!(loaded.maintenance.enabled);
        assert_eq!(loaded.maintenance.message, "Deploy window");
    }

    #[test]
    fn maintenance_status_reports_effective_settings() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path());
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        let settings_file = settings_path_for(&paths.projects_path());

        let initial = maintenance_status_at(&settings_file, &paths.projects_path()).unwrap();
        assert!(!initial.enabled);
        assert!(initial.message.is_empty());

        let mut settings = Settings::default();
        settings.maintenance.enabled = true;
        settings.maintenance.message = "Deploy window".into();
        save_settings_at(&settings_file, settings).unwrap();

        let view = maintenance_status_at(&settings_file, &paths.projects_path()).unwrap();
        assert!(view.enabled);
        assert_eq!(view.message, "Deploy window");
    }

    #[test]
    fn export_and_load_settings_roundtrip_without_runtime_writes() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path());
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        let settings_file = settings_path_for(&paths.projects_path());
        let mut settings = Settings::default();
        settings.maintenance.enabled = true;
        settings.maintenance.message = "Deploy window".into();
        settings.updater.log_retention_days = 14;
        save_settings_at(&settings_file, settings).unwrap();
        write_projects(&paths.projects_path());

        let settings_before = std::fs::read_to_string(&settings_file).unwrap();
        let projects_before = std::fs::read_to_string(paths.projects_path()).unwrap();
        let mut data_before: Vec<String> = std::fs::read_dir(paths.data_dir())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        data_before.sort();

        let destination = temp.path().join("exports/settings-export-20261117.json");
        export_settings_at(&settings_file, &paths.projects_path(), &destination).unwrap();

        assert!(destination.is_file());
        let exported = std::fs::read_to_string(&destination).unwrap();
        assert!(exported.contains("\"maintenance\""));
        assert!(exported.contains("\"message\": \"Deploy window\""));
        assert!(!exported.to_ascii_lowercase().contains("password"));
        assert!(!exported.to_ascii_lowercase().contains("token"));

        assert_eq!(
            std::fs::read_to_string(&settings_file).unwrap(),
            settings_before,
            "export must not modify settings.json"
        );
        assert_eq!(
            std::fs::read_to_string(paths.projects_path()).unwrap(),
            projects_before,
            "export must not modify projects.json"
        );
        let mut data_after: Vec<String> = std::fs::read_dir(paths.data_dir())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        data_after.sort();
        assert_eq!(data_after, data_before, "export wrote into data/");

        let loaded = load_settings_from_file_at(&destination).unwrap();
        assert!(loaded.maintenance.enabled);
        assert_eq!(loaded.maintenance.message, "Deploy window");
        assert_eq!(loaded.updater.log_retention_days, 14);
        assert_eq!(
            std::fs::read_to_string(&settings_file).unwrap(),
            settings_before,
            "load must not modify the live settings.json"
        );
    }

    #[test]
    fn export_uses_legacy_fallback_without_creating_settings_file() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path());
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        write_projects(&paths.projects_path());
        let settings_file = settings_path_for(&paths.projects_path());
        assert!(!settings_file.exists());

        let destination = temp.path().join("settings-export.json");
        export_settings_at(&settings_file, &paths.projects_path(), &destination).unwrap();

        assert!(destination.is_file());
        assert!(
            !settings_file.exists(),
            "export must not create the live settings.json"
        );
        let loaded = load_settings_from_file_at(&destination).unwrap();
        assert_eq!(loaded.tools.git.path, r"C:\legacy\git.exe");
        assert!(!loaded.maintenance.enabled);
    }

    #[test]
    fn load_settings_file_rejects_invalid_json_and_invalid_settings() {
        let temp = tempfile::tempdir().unwrap();
        let invalid_json = temp.path().join("invalid.json");
        std::fs::write(&invalid_json, "{not-json").unwrap();
        let error = load_settings_from_file_at(&invalid_json).unwrap_err();
        assert!(error.contains("JSON"), "{error}");

        let invalid_settings = temp.path().join("invalid-settings.json");
        let mut settings = Settings::default();
        settings.updater.max_concurrent_updates = 0;
        std::fs::write(
            &invalid_settings,
            serde_json::to_string_pretty(&settings).unwrap(),
        )
        .unwrap();
        let error = load_settings_from_file_at(&invalid_settings).unwrap_err();
        assert!(error.contains("maxConcurrentUpdates"), "{error}");

        let empty = temp.path().join("empty.json");
        std::fs::write(&empty, "   ").unwrap();
        let error = load_settings_from_file_at(&empty).unwrap_err();
        assert!(error.contains("empty"), "{error}");
    }

    #[test]
    fn load_settings_file_rejects_control_characters_in_maintenance_message() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("control.json");
        let mut settings = Settings::default();
        settings.maintenance.enabled = true;
        settings.maintenance.message = "line\nbreak".into();
        std::fs::write(&path, serde_json::to_string_pretty(&settings).unwrap()).unwrap();

        let error = load_settings_from_file_at(&path).unwrap_err();
        assert!(error.contains("control"), "{error}");
    }

    #[test]
    fn settings_file_helpers_require_json_extension() {
        let temp = tempfile::tempdir().unwrap();
        let text_file = temp.path().join("settings.txt");
        std::fs::write(
            &text_file,
            serde_json::to_string(&Settings::default()).unwrap(),
        )
        .unwrap();

        let error = load_settings_from_file_at(&text_file).unwrap_err();
        assert!(error.contains(".json"), "{error}");

        let destination = temp.path().join("export.txt");
        let error = export_settings_at(
            &temp.path().join("data/settings.json"),
            &temp.path().join("data/projects.json"),
            &destination,
        )
        .unwrap_err();
        assert!(error.contains(".json"), "{error}");
        assert!(!destination.exists());
    }

    #[test]
    fn load_meta_exposes_legacy_trigger_for_migration() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let projects = data.join("projects.json");
        let settings_file = data.join("settings.json");
        write_projects(&projects);

        let (settings, legacy_fallback, legacy_trigger) =
            load_settings_meta_at(&settings_file, &projects).unwrap();
        let legacy_trigger = legacy_trigger.expect("projects.json trigger");

        assert!(legacy_fallback);
        assert_eq!(
            settings.trigger.allowed_client_ips,
            legacy_trigger.allowed_client_ips
        );
        assert!(!legacy_trigger.allow_local);
    }

    #[test]
    fn meta_has_no_legacy_trigger_without_projects_file() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let projects = data.join("projects.json");
        let settings_file = data.join("settings.json");

        let (_, legacy_fallback, legacy_trigger) =
            load_settings_meta_at(&settings_file, &projects).unwrap();

        assert!(legacy_fallback);
        assert!(legacy_trigger.is_none());
    }

    #[test]
    fn migrates_trigger_to_settings_without_touching_projects() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let projects = data.join("projects.json");
        let settings_file = data.join("settings.json");
        write_projects(&projects);
        let projects_before = std::fs::read_to_string(&projects).unwrap();

        let migrated = migrate_trigger_at(&settings_file, &projects).unwrap();

        assert_eq!(
            migrated.trigger.allowed_client_ips,
            ["10.0.0.5", "127.0.0.1"]
        );
        assert!(!migrated.trigger.allow_local);
        assert!(settings_file.is_file());
        assert_eq!(
            std::fs::read_to_string(&projects).unwrap(),
            projects_before,
            "projects.json must not be modified"
        );

        let (loaded, legacy_fallback) = load_settings_at(&settings_file, &projects).unwrap();
        assert!(!legacy_fallback);
        assert_eq!(loaded.trigger.allowed_client_ips, ["10.0.0.5", "127.0.0.1"]);
        assert!(!loaded.trigger.allow_local);
    }

    #[test]
    fn migration_backs_up_existing_settings_file() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let projects = data.join("projects.json");
        let settings_file = data.join("settings.json");
        write_projects(&projects);

        let mut settings = Settings::default();
        settings.tools.git.path = r"C:\old\git.exe".into();
        settings.tools.php.path = r"C:\old\php.exe".into();
        settings.tools.composer.path = r"C:\old\composer.bat".into();
        settings.tools.npm.path = r"C:\old\npm.cmd".into();
        save_settings_at(&settings_file, settings).unwrap();

        migrate_trigger_at(&settings_file, &projects).unwrap();

        let backup_dir = temp.path().join("backup");
        let backups: Vec<_> = std::fs::read_dir(&backup_dir)
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .collect();
        assert_eq!(backups.len(), 1);
        assert!(backups[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("settings-"));
    }

    #[test]
    fn migration_rejects_invalid_legacy_trigger() {
        let temp = tempfile::tempdir().unwrap();
        let data = temp.path().join("data");
        std::fs::create_dir_all(&data).unwrap();
        let projects = data.join("projects.json");
        let settings_file = data.join("settings.json");
        std::fs::write(
            &projects,
            r#"{"version":2,"trigger":{"allowedClientIPs":["not-an-ip"]},"apps":[]}"#,
        )
        .unwrap();

        let error = migrate_trigger_at(&settings_file, &projects).unwrap_err();
        assert!(error.contains("invalid"));
        assert!(!settings_file.exists());
    }

    #[test]
    fn diagnostics_report_missing_invalid_and_task_state() {
        let mut settings = Settings::default();
        settings.tools.php.path.clear();
        settings.tools.git.path = r"C:\definitely-missing\git.exe".to_string();
        let task = TaskStatusView {
            task_name: "ServiceManager-Updater".to_string(),
            installed: false,
            state: "NotInstalled".to_string(),
            ..TaskStatusView::default()
        };

        let diagnostics = build_diagnostics(&settings, true, Some(Ok(task)));
        assert!(diagnostics
            .missing_required_tools
            .contains(&"PHP".to_string()));
        assert!(diagnostics
            .invalid_paths
            .iter()
            .any(|issue| issue.tool == "git"));
        assert!(!diagnostics.task_installed);
        assert!(diagnostics.task_message.contains("not installed"));
        assert!(diagnostics.legacy_fallback);
        assert!(diagnostics.task_error.is_none());
    }

    #[test]
    fn diagnostics_report_task_query_error() {
        let settings = Settings::default();
        let diagnostics = build_diagnostics(
            &settings,
            false,
            Some(Err("PowerShell unavailable".to_string())),
        );

        assert!(!diagnostics.task_installed);
        assert_eq!(
            diagnostics.task_error.as_deref(),
            Some("PowerShell unavailable")
        );
        assert!(!diagnostics.legacy_fallback);
    }

    #[test]
    fn resolves_tool_path_from_current_discovered_and_common_locations() {
        let temp = tempfile::tempdir().unwrap();
        let current = temp.path().join("current");
        std::fs::create_dir_all(&current).unwrap();
        let current_git = current.join("git.exe");
        std::fs::write(&current_git, b"stub").unwrap();
        let discovered = temp.path().join("discovered");
        std::fs::create_dir_all(&discovered).unwrap();
        let discovered_git = discovered.join("git.exe");
        std::fs::write(&discovered_git, b"stub").unwrap();
        let common = temp.path().join("common");
        std::fs::create_dir_all(&common).unwrap();
        let common_git = common.join("git.exe");
        std::fs::write(&common_git, b"stub").unwrap();

        let spec = tool_spec("git").unwrap();
        assert_eq!(
            resolve_tool_path_with(spec, Some(&current.to_string_lossy()), &[], &[]),
            Some(current_git)
        );
        assert_eq!(
            resolve_tool_path_with(spec, None, &[discovered], &[]),
            Some(discovered_git)
        );
        assert_eq!(
            resolve_tool_path_with(spec, None, &[], std::slice::from_ref(&common_git)),
            Some(common_git)
        );
        assert_eq!(
            resolve_tool_path_with(spec, None, &[temp.path().join("missing")], &[]),
            None
        );
    }

    #[test]
    fn apache_detection_infers_root_and_config() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("apache");
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(root.join("conf")).unwrap();
        let httpd = bin.join("httpd.exe");
        std::fs::write(&httpd, b"stub").unwrap();

        let spec = tool_spec("apache").unwrap();
        let (detected_root, config) = apache_details(spec, &httpd);
        assert_eq!(detected_root, root.to_string_lossy());
        assert_eq!(config, "conf/httpd.conf");

        let (git_root, git_config) = apache_details(tool_spec("git").unwrap(), &httpd);
        assert!(git_root.is_empty());
        assert!(git_config.is_empty());
    }

    #[test]
    fn rejects_missing_tool_executable_on_test() {
        let error = test_tool_at("git", r"C:\definitely-missing\git.exe").unwrap_err();
        assert!(error.contains("not found"));
        assert!(test_tool_at("unknown", "x").is_err());
    }

    #[test]
    fn test_tool_wraps_batch_files_and_reads_version_line() {
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("fake-git.bat");
        std::fs::write(&script, "@echo off\r\necho git version 2.45.0\r\n").unwrap();

        let result = test_tool_at("git", &script.to_string_lossy()).unwrap();

        assert!(result.success);
        assert_eq!(result.tool, "git");
        assert_eq!(result.version, "git version 2.45.0");
        assert!(result.error.is_none());
    }

    #[test]
    fn nssm_uses_existence_check_without_running_gui() {
        let temp = tempfile::tempdir().unwrap();
        let script = temp.path().join("nssm.exe");
        std::fs::write(&script, b"stub").unwrap();

        let result = test_tool_at("nssm", &script.to_string_lossy()).unwrap();

        assert!(result.success);
        assert_eq!(result.version, "installed");
        assert!(result.output.contains("no CLI version check"));
        assert!(result.error.is_none());
    }

    #[test]
    fn first_line_ignores_blank_lines_and_control_characters() {
        assert_eq!(
            first_line("\r\n  PHP 8.3.16 (cli) \r\nsecond"),
            "PHP 8.3.16 (cli)"
        );
        assert_eq!(first_line("\n\n"), "");
    }

    fn apache_tool(path: &str, root: &str) -> ApacheToolPath {
        ApacheToolPath {
            tool: ToolPath::new(path),
            root: root.to_string(),
            config: "conf/httpd.conf".to_string(),
        }
    }

    fn write_apache_services(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            path,
            r#"{"version":1,"services":[
                {"name":"Apache2.4","serviceType":"apache","executable":"C:\\Apache24\\bin\\httpd.exe","apacheRoot":"C:\\Apache24","apacheConfig":"conf/httpd.conf"},
                {"name":"ApacheLegacy","serviceType":"apache","executable":"C:\\Apache22\\bin\\httpd.exe","apacheRoot":"C:\\Apache22","apacheConfig":"conf/httpd.conf"},
                {"name":"api","serviceType":"nssm","executable":"C:\\api\\api.exe"}
            ]}"#,
        )
        .unwrap();
    }

    fn write_apache_apps(path: &Path) {
        std::fs::write(
            path,
            r#"{"version":2,"apps":[
                {"id":"kasirin","name":"Kasirin","repoUrl":"https://example.com/kasirin.git","localPath":"C:\\project","webServerService":"Apache2.4"}
            ]}"#,
        )
        .unwrap();
    }

    #[test]
    fn first_settings_save_creates_pending_for_all_apache_services() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path().join("home"));
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        write_apache_services(&paths.services_path());
        write_apache_apps(&paths.projects_path());

        let mut settings = Settings::default();
        settings.tools.apache = apache_tool(r"C:\Apache26\bin\httpd.exe", r"C:\Apache26");

        let result = save_settings_view_at(&paths, settings).unwrap();
        let pending = result.apache_pending.expect("pending Apache change");

        assert!(pending.changed);
        assert!(pending.old_path.is_empty());
        assert_eq!(pending.new_path, r"C:\Apache26\bin\httpd.exe");
        assert_eq!(pending.services.len(), 2);
        assert!(pending
            .services
            .iter()
            .any(|service| service.name == "Apache2.4"));
        assert!(pending
            .services
            .iter()
            .any(|service| service.name == "ApacheLegacy"));
        assert_eq!(pending.apps.len(), 1);
        assert_eq!(pending.apps[0].id, "kasirin");
        assert!(apache::pending_path_for(&paths.data_dir()).is_file());

        // The first save creates settings.json, so there is no old file to back up.
        if paths.backup_dir().is_dir() {
            let settings_backups = std::fs::read_dir(paths.backup_dir())
                .unwrap()
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().starts_with("settings-"))
                .count();
            assert_eq!(settings_backups, 0);
        }
    }

    #[test]
    fn apache_change_save_creates_pending_and_settings_backup() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path().join("home"));
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        write_apache_services(&paths.services_path());
        write_apache_apps(&paths.projects_path());
        let settings_file = settings_path_for(&paths.projects_path());

        let mut old = Settings::default();
        old.tools.apache = apache_tool(r"C:\Apache24\bin\httpd.exe", r"C:\Apache24");
        save_settings_at(&settings_file, old.clone()).unwrap();

        let mut next = old.clone();
        next.tools.apache = apache_tool(r"C:\Apache26\bin\httpd.exe", r"C:\Apache26");
        next.tools.apache.tool.version = "2.4.65".into();

        let result = save_settings_view_at(&paths, next).unwrap();
        let pending = result.apache_pending.expect("pending Apache change");

        assert_eq!(pending.old_path, r"C:\Apache24\bin\httpd.exe");
        assert_eq!(pending.new_path, r"C:\Apache26\bin\httpd.exe");
        assert_eq!(pending.old_root, r"C:\Apache24");
        assert_eq!(pending.new_root, r"C:\Apache26");
        assert_eq!(pending.old_version, "");
        assert_eq!(pending.new_version, "2.4.65");
        assert_eq!(pending.services.len(), 1);
        assert_eq!(pending.services[0].name, "Apache2.4");
        assert_eq!(
            pending.services[0].old_executable,
            old.tools.apache.tool.path
        );
        assert_eq!(pending.apps.len(), 1);
        assert_eq!(pending.apps[0].id, "kasirin");
        assert!(apache::pending_path_for(&paths.data_dir()).is_file());

        let settings_backups = std::fs::read_dir(paths.backup_dir())
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("settings-"))
            .count();
        assert_eq!(settings_backups, 1);

        // Saving settings must not update services.json directly.
        let stored =
            sm_config::load_services_strict(&paths.services_path(), &paths.backup_dir()).unwrap();
        assert_eq!(stored[0].executable, r"C:\Apache24\bin\httpd.exe");
        assert_eq!(stored[0].apache_root, r"C:\Apache24");

        let second = save_settings_view_at(&paths, result.settings).unwrap();
        assert!(second.apache_pending.is_none());
        assert!(!apache::pending_path_for(&paths.data_dir()).exists());
    }

    fn php_config(path: &Path, install_command: &str) {
        use sm_updater::registry::{save_config, AppConfig, CommandSpec, UpdaterConfig};

        let mut config = UpdaterConfig::default();
        config.apps.push(AppConfig {
            id: "demo".into(),
            name: "Demo".into(),
            repo_url: "https://example.com/demo.git".into(),
            local_path: r"C:\project\demo".into(),
            preset: sm_updater::registry::Preset::Custom,
            install_command: install_command.into(),
            commands: sm_updater::registry::CustomCommands {
                build: vec![CommandSpec {
                    name: "build".into(),
                    program: r"C:\old-php\php.exe".into(),
                    args: vec![r"C:\old-php\artisan".into()],
                    shell: String::new(),
                    ..CommandSpec::default()
                }],
                ..sm_updater::registry::CustomCommands::default()
            },
            ..AppConfig::default()
        });
        save_config(path, &config).unwrap();
    }

    #[test]
    fn php_path_change_reports_references_and_is_absent_on_second_save() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path());
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        php_config(
            &paths.projects_path(),
            r#""C:\old-php\php.exe" artisan migrate"#,
        );

        let settings_file = settings_path_for(&paths.projects_path());
        let mut old = Settings::default();
        old.tools.php.path = r"C:\old-php\php.exe".into();
        save_settings_at(&settings_file, old.clone()).unwrap();

        let mut next = old.clone();
        next.tools.php.path = r"C:\new-php\php.exe".into();
        let result = save_settings_view_at(&paths, next).unwrap();
        let change = result.php_path_change.expect("PHP path change preview");

        assert_eq!(change.old_path, r"C:\old-php\php.exe");
        assert_eq!(change.old_root, r"C:\old-php");
        assert_eq!(change.new_path, r"C:\new-php\php.exe");
        assert_eq!(change.new_root, r"C:\new-php");
        assert_eq!(change.references.len(), 3);
        assert!(change
            .references
            .iter()
            .any(|reference| reference.field == "installCommand"));
        assert!(change
            .references
            .iter()
            .any(|reference| reference.field == "commands.build[0].program"));
        assert!(change
            .references
            .iter()
            .any(|reference| reference.field == "commands.build[0].args[0]"));

        let second = save_settings_view_at(&paths, result.settings).unwrap();
        assert!(second.php_path_change.is_none());
    }

    #[test]
    fn php_change_without_references_is_absent() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path());
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        php_config(&paths.projects_path(), "composer install --no-dev");
        let mut config = load_config(&paths.projects_path()).unwrap();
        config.apps[0].commands = sm_updater::registry::CustomCommands::default();
        sm_updater::registry::save_config(&paths.projects_path(), &config).unwrap();

        let settings_file = settings_path_for(&paths.projects_path());
        let mut old = Settings::default();
        old.tools.php.path = r"C:\old-php\php.exe".into();
        save_settings_at(&settings_file, old.clone()).unwrap();

        let mut next = old.clone();
        next.tools.php.path = r"C:\new-php\php.exe".into();
        next.updater.log_retention_days = 14;
        let result = save_settings_view_at(&paths, next).unwrap();

        assert!(result.php_path_change.is_none());
    }

    fn write_old_log(path: &Path, age: std::time::Duration, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
        let modified = std::time::SystemTime::now()
            .checked_sub(age)
            .unwrap_or(std::time::UNIX_EPOCH);
        let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
    }

    #[test]
    fn prune_logs_at_deletes_old_updater_and_project_logs() {
        use sm_updater::registry::{save_config, AppConfig, UpdaterConfig};

        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path());
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        let project = temp.path().join("project");
        let project_log = project.join("storage/logs/project-update-old.log");
        write_old_log(
            &project_log,
            std::time::Duration::from_secs(10 * 86_400),
            "project-old",
        );
        let recent_project_log = project.join("storage/logs/project-update-new.log");
        write_old_log(
            &recent_project_log,
            std::time::Duration::from_secs(86_400),
            "project-new",
        );

        let mut config = UpdaterConfig::default();
        config.apps.push(AppConfig {
            id: "demo".into(),
            name: "Demo".into(),
            repo_url: "https://example.com/demo.git".into(),
            local_path: project.to_string_lossy().to_string(),
            ..AppConfig::default()
        });
        save_config(&paths.projects_path(), &config).unwrap();

        let log_dir = temp.path().join("logs");
        let global_log = log_dir.join("updater-task-old.log");
        write_old_log(
            &global_log,
            std::time::Duration::from_secs(10 * 86_400),
            "global-old",
        );

        let result = prune_logs_at(&paths.projects_path(), &log_dir, 7).unwrap();

        assert_eq!(result.deleted_files, 2);
        assert_eq!(
            result.deleted_bytes,
            ("project-old".len() + "global-old".len()) as u64
        );
        assert!(result.errors.is_empty());
        assert!(!project_log.exists());
        assert!(recent_project_log.exists());
        assert!(!global_log.exists());
    }

    #[test]
    fn prune_logs_at_rejects_zero_days() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path());
        let log_dir = temp.path().join("logs");
        let log = log_dir.join("project-update-old.log");
        write_old_log(&log, std::time::Duration::from_secs(10 * 86_400), "old");

        let error = prune_logs_at(&paths.projects_path(), &log_dir, 0).unwrap_err();

        assert_eq!(error, "Log retention is disabled (0 days).");
        assert!(log.exists());
    }

    #[test]
    fn prune_logs_now_uses_effective_settings_log_dir() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path());
        std::fs::create_dir_all(paths.data_dir()).unwrap();
        let custom_logs = temp.path().join("custom-logs");
        let old_log = custom_logs.join("project-update-old.log");
        write_old_log(&old_log, std::time::Duration::from_secs(10 * 86_400), "old");

        let settings_file = settings_path_for(&paths.projects_path());
        let mut settings = Settings::default();
        settings.updater.log_dir = custom_logs.to_string_lossy().to_string();
        settings.updater.log_retention_days = 7;
        save_settings_at(&settings_file, settings).unwrap();

        let result = prune_logs_now_at(&paths.projects_path()).unwrap();

        assert_eq!(result.deleted_files, 1);
        assert!(!old_log.exists());
    }
}
