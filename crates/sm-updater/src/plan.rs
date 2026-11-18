//! Read-only preview of an updater run.
//!
//! `plan_app` performs the same Git inspection as an update (including
//! `git fetch --prune`) but never merges, builds, installs, stops/starts
//! services, writes config/state, or writes updater logs. Fetching updates
//! remote-tracking refs in `.git`; that is the only repository mutation and it
//! is documented for the CLI/GUI.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sm_core::redact_secrets;

use crate::dirty::filter_status;
use crate::registry::{AppConfig, ServicePlan, UpdaterSettings};
use crate::runner::{Output, Runner};
use crate::services::{Lifecycle, ServiceControl};
use crate::sync::is_auth_failure;
use crate::toolchain::{self, PlannedCommand, ToolchainPlan};

const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(30);
const LS_REMOTE_TIMEOUT: Duration = Duration::from_secs(20);
const FETCH_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PlanStatus {
    UpToDate,
    Dirty,
    BlockedMigration,
    Ready,
    CloneRequired,
    Error,
}

impl PlanStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UpToDate => "up-to-date",
            Self::Dirty => "dirty",
            Self::BlockedMigration => "blocked-migration",
            Self::Ready => "ready",
            Self::CloneRequired => "clone-required",
            Self::Error => "error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanCommit {
    pub sha: String,
    pub subject: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlanCommandGroup {
    Install,
    Build,
    Artisan,
}

impl PlanCommandGroup {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Install => "install",
            Self::Build => "build",
            Self::Artisan => "artisan",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanCommand {
    pub group: PlanCommandGroup,
    pub label: String,
    pub description: String,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanService {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub running: Option<bool>,
    #[serde(default)]
    pub error: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanServices {
    pub stop_before_pull: Vec<String>,
    pub stop_before_build: Vec<String>,
    pub restart_after: Vec<String>,
    pub start_after: Vec<String>,
    /// `"ok"` when service names exist, otherwise the validation error.
    pub validation: String,
    pub states: Vec<PlanService>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanHealth {
    pub enabled: bool,
    pub url: String,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanNotify {
    pub enabled: bool,
    pub webhook_url: String,
    pub on: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPlan {
    pub app_id: String,
    pub app_name: String,
    pub branch: String,
    pub current_branch: String,
    pub head_before: String,
    pub remote_head: String,
    pub status: PlanStatus,
    pub message: String,
    pub incoming: Vec<PlanCommit>,
    pub changed_files: Vec<String>,
    pub migration_files: Vec<String>,
    pub commands: Vec<PlanCommand>,
    pub services: PlanServices,
    pub health_check: PlanHealth,
    pub notify: PlanNotify,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct GitScan {
    status: PlanStatus,
    message: String,
    current_branch: String,
    head_before: String,
    remote_head: String,
    incoming: Vec<PlanCommit>,
    changed_files: Vec<String>,
    migration_files: Vec<String>,
    warnings: Vec<String>,
}

impl Default for GitScan {
    fn default() -> Self {
        Self {
            status: PlanStatus::UpToDate,
            message: String::new(),
            current_branch: String::new(),
            head_before: String::new(),
            remote_head: String::new(),
            incoming: Vec::new(),
            changed_files: Vec::new(),
            migration_files: Vec::new(),
            warnings: Vec::new(),
        }
    }
}

impl GitScan {
    fn error(message: impl Into<String>) -> Self {
        Self {
            status: PlanStatus::Error,
            message: message.into(),
            ..Self::default()
        }
    }
}

/// Build a dry-run plan. Never panics; Git/service/toolchain failures are
/// reported as warnings or `PlanStatus::Error`.
pub fn plan_app(
    app: &AppConfig,
    settings: &UpdaterSettings,
    project: &Path,
    runner: &dyn Runner,
    services: &dyn ServiceControl,
) -> AppPlan {
    let (health_check, mut warnings) = plan_health(app);
    let (notify, notify_warnings) = plan_notify(app);
    warnings.extend(notify_warnings);

    let planned_services = plan_services(&app.services, services);
    if planned_services.validation != "ok" {
        warnings.push(format!(
            "Service validation: {}",
            planned_services.validation
        ));
    }
    for state in &planned_services.states {
        if !state.error.is_empty() {
            warnings.push(format!(
                "Failed to check service '{}': {}",
                state.name, state.error
            ));
        }
    }
    if !app.enabled {
        warnings.push("App disabled; automatic updates will not process this app.".to_string());
    }

    if app.branch.trim().is_empty() {
        return build_app_plan(
            app,
            GitScan::error(format!("App branch '{}' is not set.", app.id)),
            Vec::new(),
            planned_services,
            health_check,
            notify,
            warnings,
        );
    }

    let mut scan = scan_git(app, settings, project, runner);
    warnings.append(&mut scan.warnings);

    let commands = if scan.status == PlanStatus::Error {
        Vec::new()
    } else {
        let (commands, mut toolchain_warnings) =
            resolve_commands(app, project, &scan.changed_files, settings);
        warnings.append(&mut toolchain_warnings);
        if !commands.is_empty() {
            match scan.status {
                PlanStatus::Dirty => warnings.push(
                    "Working tree is dirty; toolchain commands will not run until it is clean."
                        .to_string(),
                ),
                PlanStatus::UpToDate => {
                    warnings.push("No new commits; toolchain commands will not run.".to_string())
                }
                PlanStatus::BlockedMigration => warnings.push(
                    "Update is blocked by migrations; toolchain commands will not run.".to_string(),
                ),
                _ => {}
            }
        }
        commands
    };

    build_app_plan(
        app,
        scan,
        commands,
        planned_services,
        health_check,
        notify,
        warnings,
    )
}

fn scan_git(
    app: &AppConfig,
    settings: &UpdaterSettings,
    project: &Path,
    runner: &dyn Runner,
) -> GitScan {
    match scan_git_result(app, settings, project, runner) {
        Ok(scan) => scan,
        Err(message) => GitScan::error(message),
    }
}

fn scan_git_result(
    app: &AppConfig,
    settings: &UpdaterSettings,
    project: &Path,
    runner: &dyn Runner,
) -> Result<GitScan, String> {
    let mut warnings = Vec::new();
    let git_path = Path::new(settings.git_path.trim());

    if app.local_path.trim().is_empty() || !project.exists() {
        if !git_path.is_file() {
            warnings.push(format!("Git not found: {}", git_path.display()));
        }
        if app.clone_on_first_run {
            warnings.push(
                "cloneOnFirstRun=true; the update will perform the first clone (plan does not clone)."
                    .to_string(),
            );
        } else {
            warnings.push(
                "cloneOnFirstRun=false; the update will fail until the project folder is created or cloned manually."
                    .to_string(),
            );
        }
        if app.repo_url.trim().is_empty() {
            warnings.push("repoUrl is empty; the first clone cannot be performed.".to_string());
        }
        return Ok(GitScan {
            status: PlanStatus::CloneRequired,
            message:
                "Project folder does not exist yet; a clone is required (plan does not clone)."
                    .to_string(),
            warnings,
            ..GitScan::default()
        });
    }

    if !project.is_dir() {
        return Err(format!("Local path is not a folder: {}", project.display()));
    }
    if !git_path.is_file() {
        return Err(format!("Git not found: {}", git_path.display()));
    }

    let status_output = git_required(
        runner,
        git_path,
        project,
        &["status", "--porcelain"],
        PREFLIGHT_TIMEOUT,
        "Failed to read Git status",
    )?;
    let scan = filter_status(&status_output.stdout, &app.dirty_ignore);
    if !scan.ignored.is_empty() {
        for entry in scan.ignored.iter().take(10) {
            warnings.push(format!("dirtyIgnore: {entry}"));
        }
        if scan.ignored.len() > 10 {
            warnings.push(format!(
                "dirtyIgnore: {} more entries ignored.",
                scan.ignored.len() - 10
            ));
        }
    }
    if scan.is_blocking() {
        for entry in scan.blocking.iter().take(30) {
            warnings.push(format!("Working tree: {entry}"));
        }
        if scan.blocking.len() > 30 {
            warnings.push(format!(
                "Working tree: {} more entries not shown.",
                scan.blocking.len() - 30
            ));
        }

        let mut current_branch = String::new();
        match git(
            runner,
            git_path,
            project,
            &["rev-parse", "--abbrev-ref", "HEAD"],
            PREFLIGHT_TIMEOUT,
        ) {
            Ok(output) if output.success() => {
                current_branch = output.stdout.trim().to_string();
            }
            Ok(output) => warnings.push(format!(
                "Failed to read the active branch: {}",
                output.combined()
            )),
            Err(error) => warnings.push(error),
        }

        let mut head_before = String::new();
        match git(
            runner,
            git_path,
            project,
            &["rev-parse", "HEAD"],
            PREFLIGHT_TIMEOUT,
        ) {
            Ok(output) if output.success() => {
                head_before = output.stdout.trim().to_string();
            }
            Ok(output) => warnings.push(format!("Failed to read HEAD SHA: {}", output.combined())),
            Err(error) => warnings.push(error),
        }

        return Ok(GitScan {
            status: PlanStatus::Dirty,
            message: "Working tree is not clean; the update will be skipped.".to_string(),
            current_branch,
            head_before,
            warnings,
            ..GitScan::default()
        });
    }

    let branch_output = git_required(
        runner,
        git_path,
        project,
        &["rev-parse", "--abbrev-ref", "HEAD"],
        PREFLIGHT_TIMEOUT,
        "Failed to read the Git branch",
    )?;
    let current_branch = branch_output.stdout.trim().to_string();
    if current_branch.is_empty() {
        return Err("Failed to read the active Git branch.".to_string());
    }
    let configured_branch = app.branch.trim();
    if !current_branch.eq_ignore_ascii_case(configured_branch) {
        return Err(format!(
            "Active branch is '{current_branch}', not '{configured_branch}'."
        ));
    }

    let head_before = git_required(
        runner,
        git_path,
        project,
        &["rev-parse", "HEAD"],
        PREFLIGHT_TIMEOUT,
        "Failed to read HEAD SHA",
    )?
    .stdout
    .trim()
    .to_string();
    if head_before.is_empty() {
        return Err("Failed to read HEAD SHA.".to_string());
    }

    let probe = git(
        runner,
        git_path,
        project,
        &["ls-remote", "--heads", "origin"],
        LS_REMOTE_TIMEOUT,
    )?;
    if !probe.success() || is_auth_failure(&probe) {
        return Err(format!(
            "Remote access failed. Git credentials are missing or invalid. {}",
            probe.combined()
        ));
    }

    let fetch = git(
        runner,
        git_path,
        project,
        &["fetch", "--prune", "origin"],
        FETCH_TIMEOUT,
    )?;
    if !fetch.success() || is_auth_failure(&fetch) {
        return Err(format!(
            "git fetch failed. Git credentials are missing or invalid. {}",
            fetch.combined()
        ));
    }

    let origin_ref = format!("origin/{current_branch}");
    let verify = git(
        runner,
        git_path,
        project,
        &["rev-parse", "--verify", &origin_ref],
        PREFLIGHT_TIMEOUT,
    )?;
    if !verify.success() {
        return Err(format!("Remote {origin_ref} was not found."));
    }
    let remote_head = verify.stdout.trim().to_string();

    let range = format!("HEAD..{origin_ref}");
    let count_text = git_required(
        runner,
        git_path,
        project,
        &["rev-list", "--count", &range],
        PREFLIGHT_TIMEOUT,
        "Failed to read the incoming commit count",
    )?
    .stdout
    .trim()
    .to_string();
    let count: usize = count_text
        .parse()
        .map_err(|_| format!("Failed to read the incoming commit count: {count_text}"))?;

    if count == 0 {
        return Ok(GitScan {
            status: PlanStatus::UpToDate,
            message: "Already up to date; no new commits.".to_string(),
            current_branch,
            head_before,
            remote_head,
            warnings,
            ..GitScan::default()
        });
    }

    let log_output = git_required(
        runner,
        git_path,
        project,
        &["log", "--format=%H%x09%s", &range],
        PREFLIGHT_TIMEOUT,
        "Failed to read incoming commits",
    )?;
    let incoming = parse_commits(&log_output.stdout);
    if incoming.is_empty() {
        warnings.push(format!(
            "git log returned no commits even though rev-list counted {count}."
        ));
    }

    let diff_output = git_required(
        runner,
        git_path,
        project,
        &["diff", "--name-only", &range],
        PREFLIGHT_TIMEOUT,
        "Failed to read the changed file list",
    )?;
    let changed_files = split_lines(&diff_output.stdout);
    let migration_files: Vec<String> = changed_files
        .iter()
        .filter(|file| sm_core::update::is_migration_change(file))
        .cloned()
        .collect();

    if !migration_files.is_empty() {
        let policy = if app.migration_policy.trim().is_empty() {
            "block"
        } else {
            app.migration_policy.trim()
        };
        warnings.push(format!(
            "migrationPolicy={policy}: migrations are not run automatically; the update is blocked."
        ));
    }

    let status = if migration_files.is_empty() {
        PlanStatus::Ready
    } else {
        PlanStatus::BlockedMigration
    };
    let message = if migration_files.is_empty() {
        format!("{count} new commit(s) ready to merge.")
    } else {
        format!(
            "Update blocked: {} migration file(s) must be run manually.",
            migration_files.len()
        )
    };

    Ok(GitScan {
        status,
        message,
        current_branch,
        head_before,
        remote_head,
        incoming,
        changed_files,
        migration_files,
        warnings,
    })
}

fn git(
    runner: &dyn Runner,
    git_path: &Path,
    cwd: &Path,
    args: &[&str],
    timeout: Duration,
) -> Result<Output, String> {
    let args: Vec<String> = args.iter().map(|value| value.to_string()).collect();
    let mut env = BTreeMap::new();
    env.insert("GIT_TERMINAL_PROMPT".to_string(), "0".to_string());
    env.insert("GCM_INTERACTIVE".to_string(), "never".to_string());
    runner
        .run_with_env(git_path, &args, cwd, timeout, &env)
        .map_err(|error| format!("Failed to run git: {error}"))
}

fn git_required(
    runner: &dyn Runner,
    git_path: &Path,
    cwd: &Path,
    args: &[&str],
    timeout: Duration,
    context: &str,
) -> Result<Output, String> {
    let output = git(runner, git_path, cwd, args, timeout)?;
    if output.success() {
        Ok(output)
    } else {
        Err(format!("{context}: {}", output.combined()))
    }
}

fn resolve_commands(
    app: &AppConfig,
    project: &Path,
    changed: &[String],
    settings: &UpdaterSettings,
) -> (Vec<PlanCommand>, Vec<String>) {
    match toolchain::resolve_plan(app, project, changed, settings) {
        Ok(plan) => {
            let warnings = plan
                .notes
                .iter()
                .map(|note| format!("Toolchain: {note}"))
                .collect();
            (map_commands(&plan), warnings)
        }
        Err(error) => (
            Vec::new(),
            vec![format!("Toolchain cannot be planned: {error}")],
        ),
    }
}

fn map_commands(plan: &ToolchainPlan) -> Vec<PlanCommand> {
    let mut commands = Vec::new();
    for (group, list) in [
        (PlanCommandGroup::Install, &plan.install),
        (PlanCommandGroup::Build, &plan.build),
        (PlanCommandGroup::Artisan, &plan.artisan),
    ] {
        for command in list {
            commands.push(map_command(group, command));
        }
    }
    commands
}

fn map_command(group: PlanCommandGroup, command: &PlannedCommand) -> PlanCommand {
    PlanCommand {
        group,
        label: redact(&command.label),
        description: redact(&command.description()),
        program: redact(&command.program.to_string_lossy()),
        args: command.args.iter().map(|arg| redact(arg)).collect(),
        cwd: redact(&command.cwd.to_string_lossy()),
    }
}

fn plan_services(plan: &ServicePlan, control: &dyn ServiceControl) -> PlanServices {
    let lifecycle = Lifecycle::new(control, plan.wait_timeout_seconds);
    let validation = match lifecycle.validate_readonly(plan) {
        Ok(()) => "ok".to_string(),
        Err(error) => error,
    };
    let states = service_names(plan)
        .into_iter()
        .map(|name| match control.is_running(&name) {
            Ok(running) => PlanService {
                name,
                running: Some(running),
                error: String::new(),
            },
            Err(error) => PlanService {
                name,
                running: None,
                error,
            },
        })
        .collect();

    PlanServices {
        stop_before_pull: unique_names(&plan.stop_before_pull),
        stop_before_build: unique_names(&plan.stop_before_build),
        restart_after: unique_names(&plan.restart_after),
        start_after: unique_names(&plan.start_after),
        validation,
        states,
    }
}

fn service_names(plan: &ServicePlan) -> Vec<String> {
    let mut names = Vec::new();
    for list in [
        &plan.stop_before_pull,
        &plan.stop_before_build,
        &plan.restart_after,
        &plan.start_after,
    ] {
        for name in unique_names(list) {
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

fn unique_names(values: &[String]) -> Vec<String> {
    let mut names = Vec::new();
    for value in values {
        let value = value.trim().to_string();
        if !value.is_empty() && !names.contains(&value) {
            names.push(value);
        }
    }
    names
}

fn plan_health(app: &AppConfig) -> (PlanHealth, Vec<String>) {
    let url = app.health_check.url.trim();
    let mut warnings = Vec::new();
    if app.health_check.enabled {
        if url.is_empty() {
            warnings.push("Health check is enabled but the URL is empty.".to_string());
        } else if !(url.starts_with("http://") || url.starts_with("https://")) {
            warnings.push(format!("Health check URL is invalid: {url}"));
        }
        if app.health_check.timeout_seconds == 0 {
            warnings.push("Health check timeout is 0; updates use at least 1 second.".to_string());
        }
    }
    (
        PlanHealth {
            enabled: app.health_check.enabled,
            url: redact(url),
            timeout_seconds: app.health_check.timeout_seconds,
        },
        warnings,
    )
}

fn plan_notify(app: &AppConfig) -> (PlanNotify, Vec<String>) {
    let url = app.notify.webhook_url.trim();
    let enabled = !url.is_empty();
    let on: Vec<String> = app
        .notify
        .on
        .iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect();
    let mut warnings = Vec::new();
    if enabled && on.is_empty() {
        warnings.push(
            "Webhook URL is set but notify.on is empty; no notifications will be sent.".to_string(),
        );
    }
    if !enabled && !on.is_empty() {
        warnings.push(
            "notify.on is set but webhookUrl is empty; notifications will not be sent.".to_string(),
        );
    }
    (
        PlanNotify {
            enabled,
            webhook_url: redact(url),
            on,
        },
        warnings,
    )
}

#[allow(clippy::too_many_arguments)]
fn build_app_plan(
    app: &AppConfig,
    scan: GitScan,
    commands: Vec<PlanCommand>,
    services: PlanServices,
    health_check: PlanHealth,
    notify: PlanNotify,
    warnings: Vec<String>,
) -> AppPlan {
    let app_name = if app.name.trim().is_empty() {
        app.id.clone()
    } else {
        app.name.trim().to_string()
    };
    let commands = commands
        .into_iter()
        .map(|command| PlanCommand {
            group: command.group,
            label: redact(&command.label),
            description: redact(&command.description),
            program: redact(&command.program),
            args: command.args.iter().map(|arg| redact(arg)).collect(),
            cwd: redact(&command.cwd),
        })
        .collect();
    let services = PlanServices {
        stop_before_pull: services.stop_before_pull,
        stop_before_build: services.stop_before_build,
        restart_after: services.restart_after,
        start_after: services.start_after,
        validation: redact(&services.validation),
        states: services
            .states
            .into_iter()
            .map(|state| PlanService {
                name: redact(&state.name),
                running: state.running,
                error: redact(&state.error),
            })
            .collect(),
    };

    AppPlan {
        app_id: app.id.clone(),
        app_name,
        branch: app.branch.trim().to_string(),
        current_branch: scan.current_branch,
        head_before: scan.head_before,
        remote_head: scan.remote_head,
        status: scan.status,
        message: redact(&scan.message),
        incoming: scan
            .incoming
            .into_iter()
            .map(|commit| PlanCommit {
                sha: redact(&commit.sha),
                subject: redact(&commit.subject),
            })
            .collect(),
        changed_files: scan.changed_files,
        migration_files: scan.migration_files,
        commands,
        services,
        health_check: PlanHealth {
            enabled: health_check.enabled,
            url: redact(&health_check.url),
            timeout_seconds: health_check.timeout_seconds,
        },
        notify: PlanNotify {
            enabled: notify.enabled,
            webhook_url: redact(&notify.webhook_url),
            on: notify.on,
        },
        warnings: warnings.iter().map(|warning| redact(warning)).collect(),
    }
}

fn parse_commits(text: &str) -> Vec<PlanCommit> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() {
                return None;
            }
            let (sha, subject) = match line.split_once('\t').or_else(|| line.split_once(' ')) {
                Some((sha, subject)) => (sha.trim().to_string(), subject.trim().to_string()),
                None => (line.to_string(), String::new()),
            };
            Some(PlanCommit { sha, subject })
        })
        .collect()
}

fn split_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

fn redact(value: &str) -> String {
    redact_secrets(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{AppConfig, Preset, ServicePlan};
    use crate::runner::{Output, RunnerError};
    use crate::services::ServiceControl;
    use sm_core::ToggleMode;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::time::Duration;

    #[derive(Debug, Clone)]
    struct Call {
        program: String,
        args: Vec<String>,
    }

    struct FakeRunner {
        calls: Mutex<Vec<Call>>,
        replies: Replies,
    }

    #[derive(Clone, Default)]
    struct Replies {
        status: String,
        active_branch: String,
        head: String,
        remote_head: String,
        count: String,
        log: String,
        diff: String,
        fail_ls_remote: bool,
        fail_fetch: bool,
    }

    impl FakeRunner {
        fn new(replies: Replies) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                replies,
            }
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }

        fn has_call(&self, needle: &str) -> bool {
            self.calls().iter().any(|call| {
                call.program.eq_ignore_ascii_case(needle)
                    || call.args.iter().any(|arg| arg == needle)
            })
        }
    }

    impl Runner for FakeRunner {
        fn run(
            &self,
            program: &Path,
            args: &[String],
            _cwd: &Path,
            _timeout: Duration,
        ) -> Result<Output, RunnerError> {
            self.calls.lock().unwrap().push(Call {
                program: program.to_string_lossy().to_string(),
                args: args.to_vec(),
            });
            Ok(self.replies.output(args))
        }
    }

    impl Replies {
        fn output(&self, args: &[String]) -> Output {
            let first = args.first().map(String::as_str).unwrap_or("");
            match first {
                "status" => ok(&self.status),
                "rev-parse" if args.get(1).map(String::as_str) == Some("--abbrev-ref") => {
                    ok(&self.active_branch)
                }
                "rev-parse" if args.get(1).map(String::as_str) == Some("HEAD") => ok(&self.head),
                "rev-parse" if args.get(1).map(String::as_str) == Some("--verify") => {
                    ok(&self.remote_head)
                }
                "ls-remote" if self.fail_ls_remote => {
                    fail("fatal: Authentication failed for 'https://example.test/repo'")
                }
                "ls-remote" => ok("refs/heads/main\n"),
                "fetch" if self.fail_fetch => fail("fatal: could not read Username"),
                "fetch" => ok(""),
                "rev-list" => ok(&self.count),
                "log" => ok(&self.log),
                "diff" => ok(&self.diff),
                _ => ok(""),
            }
        }
    }

    fn ready_replies() -> Replies {
        Replies {
            status: String::new(),
            active_branch: "main".into(),
            head: "aaaa1111aaaa1111".into(),
            remote_head: "bbbb2222bbbb2222".into(),
            count: "2".into(),
            log: "bbbb2222\tfeat: second\naaaa3333\tfix: first".into(),
            diff: "src/app.ts\npackage-lock.json".into(),
            ..Replies::default()
        }
    }

    struct FakeControl {
        known: BTreeSet<String>,
        running: BTreeSet<String>,
        calls: Mutex<Vec<String>>,
    }

    impl FakeControl {
        fn new(known: &[&str], running: &[&str]) -> Self {
            Self {
                known: known.iter().map(|value| value.to_string()).collect(),
                running: running.iter().map(|value| value.to_string()).collect(),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl ServiceControl for FakeControl {
        fn exists(&self, name: &str) -> Result<bool, String> {
            self.calls.lock().unwrap().push(format!("exists:{name}"));
            Ok(self.known.contains(name))
        }

        fn is_running(&self, name: &str) -> Result<bool, String> {
            self.calls
                .lock()
                .unwrap()
                .push(format!("is_running:{name}"));
            Ok(self.running.contains(name))
        }

        fn stop(&self, name: &str, _timeout: Duration) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("stop:{name}"));
            Ok(())
        }

        fn start(&self, name: &str, _timeout: Duration) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("start:{name}"));
            Ok(())
        }

        fn restart(&self, name: &str, _timeout: Duration) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("restart:{name}"));
            Ok(())
        }
    }

    struct Fixture {
        _temp: tempfile::TempDir,
        project: PathBuf,
        app: AppConfig,
        settings: UpdaterSettings,
    }

    fn fixture(project_exists: bool) -> Fixture {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("demo");
        if project_exists {
            std::fs::create_dir_all(project.join(".git")).unwrap();
            std::fs::write(
                project.join("package.json"),
                r#"{"scripts":{"build":"vite build"}}"#,
            )
            .unwrap();
            std::fs::write(project.join("package-lock.json"), "{}").unwrap();
        }

        let tools = temp.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        for name in ["git.exe", "php.exe", "composer.bat", "npm.cmd"] {
            std::fs::write(tools.join(name), b"dummy").unwrap();
        }
        let mut settings = UpdaterSettings {
            git_path: tools.join("git.exe").to_string_lossy().to_string(),
            php_path: tools.join("php.exe").to_string_lossy().to_string(),
            composer_path: tools.join("composer.bat").to_string_lossy().to_string(),
            npm_path: tools.join("npm.cmd").to_string_lossy().to_string(),
            ..UpdaterSettings::default()
        };
        settings.refresh_variables();

        let app = AppConfig {
            id: "demo".into(),
            name: "Demo".into(),
            repo_url: "https://example.test/demo.git".into(),
            local_path: project.to_string_lossy().to_string(),
            branch: "main".into(),
            preset: Preset::Node,
            package_manager: crate::registry::PackageManager::Npm,
            package_install: ToggleMode::Always,
            build: ToggleMode::Always,
            ..AppConfig::default()
        };

        Fixture {
            _temp: temp,
            project,
            app,
            settings,
        }
    }

    fn ok(stdout: &str) -> Output {
        Output {
            exit_code: 0,
            stdout: stdout.to_string(),
            stderr: String::new(),
        }
    }

    fn fail(stderr: &str) -> Output {
        Output {
            exit_code: 1,
            stdout: String::new(),
            stderr: stderr.to_string(),
        }
    }

    fn assert_only_read_only_git_calls(calls: &[Call]) {
        for call in calls {
            assert!(
                call.program.to_ascii_lowercase().ends_with("git.exe"),
                "unexpected program: {call:?}"
            );
            let first = call.args.first().map(String::as_str).unwrap_or("");
            assert!(
                matches!(
                    first,
                    "status" | "rev-parse" | "ls-remote" | "fetch" | "rev-list" | "log" | "diff"
                ),
                "unexpected git command: {call:?}"
            );
        }
    }

    fn assert_no_service_mutation(calls: &[String]) {
        for call in calls {
            assert!(
                call.starts_with("exists:") || call.starts_with("is_running:"),
                "service mutation attempted: {call}"
            );
        }
    }

    #[test]
    fn up_to_date_plan_does_not_merge() {
        let fixture = fixture(true);
        let mut replies = ready_replies();
        replies.count = "0".into();
        replies.log.clear();
        replies.diff.clear();
        let runner = FakeRunner::new(replies);
        let control = FakeControl::new(&[], &[]);

        let plan = plan_app(
            &fixture.app,
            &fixture.settings,
            &fixture.project,
            &runner,
            &control,
        );

        assert_eq!(plan.status, PlanStatus::UpToDate);
        assert!(plan.incoming.is_empty());
        assert!(plan.changed_files.is_empty());
        assert!(plan.migration_files.is_empty());
        assert!(!runner.has_call("merge"));
        assert!(control.calls().is_empty());
        assert_only_read_only_git_calls(&runner.calls());
    }

    #[test]
    fn ready_plan_has_commits_files_commands_and_service_state() {
        let mut fixture = fixture(true);
        fixture.app.services = ServicePlan {
            stop_before_build: vec!["api".into()],
            restart_after: vec!["api".into()],
            wait_timeout_seconds: 5,
            ..ServicePlan::default()
        };
        fixture.app.notify.webhook_url =
            "https://oauth2:supersecret@hooks.example.test/updater".into();
        fixture.app.notify.on = vec!["failed".into()];
        let runner = FakeRunner::new(ready_replies());
        let control = FakeControl::new(&["api"], &["api"]);

        let plan = plan_app(
            &fixture.app,
            &fixture.settings,
            &fixture.project,
            &runner,
            &control,
        );

        assert_eq!(plan.status, PlanStatus::Ready, "{}", plan.message);
        assert_eq!(plan.incoming.len(), 2);
        assert_eq!(plan.incoming[0].sha, "bbbb2222");
        assert_eq!(plan.incoming[0].subject, "feat: second");
        assert_eq!(plan.changed_files, ["src/app.ts", "package-lock.json"]);
        assert!(plan.migration_files.is_empty());
        assert_eq!(plan.services.validation, "ok");
        assert_eq!(plan.services.states.len(), 1);
        assert_eq!(plan.services.states[0].name, "api");
        assert_eq!(plan.services.states[0].running, Some(true));

        let install = plan
            .commands
            .iter()
            .find(|command| command.group == PlanCommandGroup::Install)
            .expect("install command");
        assert_eq!(install.label, "npm install");
        assert!(install.description.contains("npm.cmd"), "{install:?}");
        assert!(install.args.iter().any(|arg| arg == "ci"));
        assert_eq!(install.cwd, fixture.project.to_string_lossy().to_string());
        assert!(plan
            .commands
            .iter()
            .any(|command| command.group == PlanCommandGroup::Build));

        assert!(plan.notify.enabled);
        assert!(!plan.notify.webhook_url.contains("supersecret"));
        assert!(plan.notify.webhook_url.contains("[REDACTED]"));

        assert_no_service_mutation(&control.calls());
        assert!(control.calls().iter().any(|call| call == "exists:api"));
        assert!(control.calls().iter().any(|call| call == "is_running:api"));
        assert_only_read_only_git_calls(&runner.calls());

        let value = serde_json::to_value(&plan).unwrap();
        assert_eq!(value["status"], "ready");
        assert_eq!(value["appId"], "demo");
        assert_eq!(value["commands"][0]["group"], "install");
        assert_eq!(value["services"]["stopBeforeBuild"][0], "api");
        assert_eq!(value["notify"]["webhookUrl"], plan.notify.webhook_url);
    }

    #[test]
    fn dirty_plan_reports_first_entries_and_skips_fetch() {
        let fixture = fixture(true);
        let mut replies = ready_replies();
        replies.status = " M src/app.ts\n?? notes.txt\n".into();
        let runner = FakeRunner::new(replies);
        let control = FakeControl::new(&[], &[]);

        let plan = plan_app(
            &fixture.app,
            &fixture.settings,
            &fixture.project,
            &runner,
            &control,
        );

        assert_eq!(plan.status, PlanStatus::Dirty);
        assert_eq!(plan.current_branch, "main");
        assert!(plan.message.contains("Working tree"));
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.contains("M src/app.ts")));
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.contains("notes.txt")));
        assert!(!runner.has_call("fetch"));
        assert!(!runner.has_call("merge"));
        assert_only_read_only_git_calls(&runner.calls());
        assert_no_service_mutation(&control.calls());
    }

    #[test]
    fn dirty_plan_caps_reported_entries_at_thirty() {
        let fixture = fixture(true);
        let mut replies = ready_replies();
        replies.status = (1..=35)
            .map(|index| format!(" M file{index}.txt"))
            .collect::<Vec<_>>()
            .join("\n");
        let runner = FakeRunner::new(replies);
        let control = FakeControl::new(&[], &[]);

        let plan = plan_app(
            &fixture.app,
            &fixture.settings,
            &fixture.project,
            &runner,
            &control,
        );

        assert_eq!(plan.status, PlanStatus::Dirty);
        let entries = plan
            .warnings
            .iter()
            .filter(|warning| {
                warning.starts_with("Working tree: ") && !warning.contains("more entries")
            })
            .count();
        assert_eq!(entries, 30);
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.contains("5 more entries")));
    }

    #[test]
    fn dirty_ignore_continues_plan_for_generated_artifacts() {
        let mut fixture = fixture(true);
        fixture.app.dirty_ignore = vec!["public/build/**".into()];
        let mut replies = ready_replies();
        replies.status = "?? public/build/app.js\n".into();
        let runner = FakeRunner::new(replies);
        let control = FakeControl::new(&[], &[]);

        let plan = plan_app(
            &fixture.app,
            &fixture.settings,
            &fixture.project,
            &runner,
            &control,
        );

        assert_eq!(plan.status, PlanStatus::Ready, "{}", plan.message);
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.contains("dirtyIgnore") && warning.contains("public/build")));
        assert!(runner.has_call("fetch"));
        assert_only_read_only_git_calls(&runner.calls());
    }

    #[test]
    fn dirty_ignore_keeps_unmatched_entries_blocking() {
        let mut fixture = fixture(true);
        fixture.app.dirty_ignore = vec!["public/build/**".into()];
        let mut replies = ready_replies();
        replies.status = "?? public/build/app.js\n M src/app.ts\n".into();
        let runner = FakeRunner::new(replies);
        let control = FakeControl::new(&[], &[]);

        let plan = plan_app(
            &fixture.app,
            &fixture.settings,
            &fixture.project,
            &runner,
            &control,
        );

        assert_eq!(plan.status, PlanStatus::Dirty);
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.contains("dirtyIgnore") && warning.contains("public/build")));
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.contains("Working tree: M src/app.ts")));
        assert!(!runner.has_call("fetch"));
        assert!(!runner.has_call("merge"));
    }

    #[test]
    fn migration_changes_block_the_plan_but_show_commands() {
        let fixture = fixture(true);
        let mut replies = ready_replies();
        replies.count = "1".into();
        replies.log = "bbbb2222\tfeat: migration".into();
        replies.diff = "database/migrations/2026_01_01_create_users.php\nsrc/app.ts".into();
        let runner = FakeRunner::new(replies);
        let control = FakeControl::new(&[], &[]);

        let plan = plan_app(
            &fixture.app,
            &fixture.settings,
            &fixture.project,
            &runner,
            &control,
        );

        assert_eq!(plan.status, PlanStatus::BlockedMigration);
        assert_eq!(
            plan.migration_files,
            ["database/migrations/2026_01_01_create_users.php"]
        );
        assert!(!plan.commands.is_empty());
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.to_ascii_lowercase().contains("migration")));
        assert!(!runner.has_call("merge"));
        assert_only_read_only_git_calls(&runner.calls());
        assert_no_service_mutation(&control.calls());
    }

    #[test]
    fn missing_project_requires_clone_without_running_git() {
        let fixture = fixture(false);
        let runner = FakeRunner::new(ready_replies());
        let control = FakeControl::new(&[], &[]);

        let plan = plan_app(
            &fixture.app,
            &fixture.settings,
            &fixture.project,
            &runner,
            &control,
        );

        assert_eq!(plan.status, PlanStatus::CloneRequired);
        assert!(plan.message.contains("clone"));
        assert!(plan.warnings.iter().any(|warning| {
            warning.contains("Project folder not found")
                || warning.contains("Toolchain cannot be planned")
        }));
        assert!(plan
            .warnings
            .iter()
            .any(|warning| { warning.contains("cloneOnFirstRun") || warning.contains("clone") }));
        assert!(plan.commands.is_empty());
        assert!(runner.calls().is_empty());
        assert!(control.calls().is_empty());
    }

    #[test]
    fn missing_git_reports_error_without_running_commands() {
        let mut fixture = fixture(true);
        fixture.settings.git_path = fixture
            ._temp
            .path()
            .join("missing-git.exe")
            .to_string_lossy()
            .to_string();
        let runner = FakeRunner::new(ready_replies());
        let control = FakeControl::new(&[], &[]);

        let plan = plan_app(
            &fixture.app,
            &fixture.settings,
            &fixture.project,
            &runner,
            &control,
        );

        assert_eq!(plan.status, PlanStatus::Error);
        assert!(plan.message.contains("Git not found"));
        assert!(plan.commands.is_empty());
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn branch_mismatch_reports_error_before_remote_access() {
        let fixture = fixture(true);
        let mut replies = ready_replies();
        replies.active_branch = "develop".into();
        let runner = FakeRunner::new(replies);
        let control = FakeControl::new(&[], &[]);

        let plan = plan_app(
            &fixture.app,
            &fixture.settings,
            &fixture.project,
            &runner,
            &control,
        );

        assert_eq!(plan.status, PlanStatus::Error);
        assert!(plan.message.contains("Active branch is 'develop'"));
        assert!(plan.message.contains("main"));
        assert!(!runner.has_call("ls-remote"));
        assert!(!runner.has_call("fetch"));
    }

    #[test]
    fn toolchain_resolution_failure_is_a_warning_not_an_error() {
        let mut fixture = fixture(true);
        fixture.app.package_manager = crate::registry::PackageManager::Auto;
        std::fs::write(fixture.project.join("yarn.lock"), "{}").unwrap();
        let runner = FakeRunner::new(ready_replies());
        let control = FakeControl::new(&[], &[]);

        let plan = plan_app(
            &fixture.app,
            &fixture.settings,
            &fixture.project,
            &runner,
            &control,
        );

        assert_eq!(plan.status, PlanStatus::Ready);
        assert!(plan.commands.is_empty());
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.contains("Ambiguous")));
    }

    #[test]
    fn missing_service_is_a_warning_but_does_not_block_plan() {
        let mut fixture = fixture(true);
        fixture.app.services.restart_after = vec!["missing".into()];
        let runner = FakeRunner::new(ready_replies());
        let control = FakeControl::new(&[], &[]);

        let plan = plan_app(
            &fixture.app,
            &fixture.settings,
            &fixture.project,
            &runner,
            &control,
        );

        assert_eq!(plan.status, PlanStatus::Ready);
        assert!(plan.services.validation.contains("missing"));
        assert!(plan
            .warnings
            .iter()
            .any(|warning| warning.contains("Service validation")));
        assert_no_service_mutation(&control.calls());
    }

    #[test]
    fn remote_auth_failure_reports_error_before_fetch() {
        let fixture = fixture(true);
        let mut replies = ready_replies();
        replies.fail_ls_remote = true;
        let runner = FakeRunner::new(replies);
        let control = FakeControl::new(&[], &[]);

        let plan = plan_app(
            &fixture.app,
            &fixture.settings,
            &fixture.project,
            &runner,
            &control,
        );

        assert_eq!(plan.status, PlanStatus::Error);
        assert!(plan.message.contains("Remote access failed"));
        assert!(!runner.has_call("fetch"));
    }

    #[test]
    fn parse_commits_supports_tab_and_oneline_formats() {
        let commits = parse_commits("abc123\tfeat: one\ndef456 fix: two\n");
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].sha, "abc123");
        assert_eq!(commits[0].subject, "feat: one");
        assert_eq!(commits[1].sha, "def456");
        assert_eq!(commits[1].subject, "fix: two");
    }

    #[test]
    fn plan_status_and_command_group_serialize_as_documented() {
        for (status, expected) in [
            (PlanStatus::UpToDate, "\"up-to-date\""),
            (PlanStatus::Dirty, "\"dirty\""),
            (PlanStatus::BlockedMigration, "\"blocked-migration\""),
            (PlanStatus::Ready, "\"ready\""),
            (PlanStatus::CloneRequired, "\"clone-required\""),
            (PlanStatus::Error, "\"error\""),
        ] {
            assert_eq!(serde_json::to_string(&status).unwrap(), expected);
            assert_eq!(status.as_str(), expected.trim_matches('"'));
        }
        for (group, expected) in [
            (PlanCommandGroup::Install, "\"install\""),
            (PlanCommandGroup::Build, "\"build\""),
            (PlanCommandGroup::Artisan, "\"artisan\""),
        ] {
            assert_eq!(serde_json::to_string(&group).unwrap(), expected);
        }
    }
}
