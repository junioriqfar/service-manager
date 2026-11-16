use std::collections::BTreeMap;
use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sm_config::ConfigPaths;
use sm_updater::registry::load_config;
use sm_updater::runner::{wrap_command, Output, Runner, SystemRunner};
use sm_updater::settings::{
    effective_settings, load_settings, save_settings, settings_path, settings_path_for, Settings,
};

const TASK_SCRIPT_INSTALL: &str = "Install-UpdaterTask.ps1";
const TASK_SCRIPT_TEST: &str = "Invoke-UpdaterTask.ps1";
const TASK_QUERY_TIMEOUT: Duration = Duration::from_secs(30);
const TASK_OPERATION_TIMEOUT: Duration = Duration::from_secs(180);
const TOOL_TEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_VERSION_CHARS: usize = 128;
const MAX_TASK_OUTPUT_CHARS: usize = 20_000;

const TOOL_SPECS: &[ToolSpec] = &[
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
        version_args: &["version"],
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
[pscustomobject]@{
    installed = $true
    taskName = $name
    state = [string]$task.State
    runLevel = [string]$task.Principal.RunLevel
    user = [string]$task.Principal.UserId
    logonType = [string]$task.Principal.LogonType
    lastRun = $lastRun
    lastResult = $lastResult
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
    pub diagnostics: SettingsDiagnostics,
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
}

struct ToolSpec {
    name: &'static str,
    label: &'static str,
    binaries: &'static [&'static str],
    version_args: &'static [&'static str],
    required: bool,
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
    let path_text = path.to_string_lossy().to_string();
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

    match SystemRunner.run(&program, &args, cwd, TOOL_TEST_TIMEOUT) {
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

fn settings_tool_path<'a>(settings: &'a Settings, tool: &str) -> &'a str {
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
    task: Option<Result<TaskStatusView, String>>,
) -> SettingsView {
    let diagnostics = build_diagnostics(&settings, legacy_fallback, task);
    SettingsView {
        settings,
        legacy_fallback,
        diagnostics,
    }
}

fn load_settings_at(
    settings_file: &Path,
    projects_file: &Path,
) -> Result<(Settings, bool), String> {
    let config = load_config(projects_file)?;
    let legacy_fallback = load_settings(settings_file)
        .map_err(|error| error.to_string())?
        .is_none();
    let settings = effective_settings(settings_file, &config.settings, &config.trigger)
        .map_err(|error| error.to_string())?;
    Ok((settings, legacy_fallback))
}

fn save_settings_at(settings_file: &Path, mut settings: Settings) -> Result<Settings, String> {
    settings.normalize();
    save_settings(settings_file, &settings).map_err(|error| error.to_string())?;
    Ok(settings)
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
        error: None,
    })
}

fn task_status_blocking(task_name: &str) -> Result<TaskStatusView, String> {
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
        let (settings, legacy_fallback) = load_settings_at(&settings_file, &paths.projects_path())?;
        let task = task_status_blocking(&settings.privileged_execution.task_name);
        Ok(build_view(settings, legacy_fallback, Some(task)))
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub fn save_settings_view(settings: Settings) -> Result<Settings, String> {
    save_settings_at(&settings_path(), settings)
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
    use sm_updater::settings::ToolPath;

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
        let json = r#"{"installed":true,"taskName":"ServiceManager-Updater","state":"Ready","runLevel":"Highest","user":"DOMAIN\\User","logonType":"Interactive","lastRun":"2026-11-16T09:30:00.0000000+07:00","lastResult":0}"#;
        let status = parse_task_status_json("ServiceManager-Updater", json).unwrap();

        assert!(status.installed);
        assert_eq!(status.task_name, "ServiceManager-Updater");
        assert_eq!(status.state, "Ready");
        assert_eq!(status.run_level, "Highest");
        assert_eq!(status.user, r"DOMAIN\User");
        assert_eq!(status.logon_type, "Interactive");
        assert!(status.last_run.contains("2026-11-16"));
        assert_eq!(status.last_result, 0);
        assert!(status.error.is_none());
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
    fn first_line_ignores_blank_lines_and_control_characters() {
        assert_eq!(
            first_line("\r\n  PHP 8.3.16 (cli) \r\nsecond"),
            "PHP 8.3.16 (cli)"
        );
        assert_eq!(first_line("\n\n"), "");
    }
}
