use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::dirty::filter_status;
use crate::log::UpdateLogger;
use crate::runner::{wrap_command, Output, Runner};
use crate::update::UpdateOptions;

const PREFLIGHT_TIMEOUT: Duration = Duration::from_secs(30);
const FETCH_TIMEOUT: Duration = Duration::from_secs(120);
pub(crate) const TOOL_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SyncOutcome {
    pub before: String,
    pub after: String,
    pub changed: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SyncResult {
    UpToDate(String),
    Dirty(String),
    Check(usize),
    BlockedMigration(usize),
    Merged(SyncOutcome),
}

/// Jalankan seluruh tahap Git sampai merge. Hook `before_merge` hanya dipanggil
/// jika ada commit baru, bukan dry-run, dan tidak ada migration yang diblokir.
#[allow(clippy::too_many_arguments)]
pub(crate) fn sync_project(
    path: &Path,
    branch: &str,
    git_path: &Path,
    runner: &dyn Runner,
    logger: &UpdateLogger,
    options: UpdateOptions,
    dirty_ignore: &[String],
    before_merge: impl FnOnce(&[String]) -> Result<(), String>,
) -> Result<SyncResult, String> {
    if !git_path.is_file() {
        return Err(format!("Git not found: {}", git_path.display()));
    }

    let status = git(
        path,
        git_path,
        runner,
        logger,
        &["status", "--porcelain"],
        PREFLIGHT_TIMEOUT,
    )?;
    let scan = filter_status(&status.stdout, dirty_ignore);
    if !scan.ignored.is_empty() {
        let shown = scan
            .ignored
            .iter()
            .take(10)
            .cloned()
            .collect::<Vec<_>>()
            .join("; ");
        let remaining = scan.ignored.len().saturating_sub(10);
        if remaining > 0 {
            logger.log(&format!(
                "INFO: {} change(s) ignored by dirtyIgnore: {shown} (+{remaining} more entries)",
                scan.ignored.len()
            ));
        } else {
            logger.log(&format!(
                "INFO: {} change(s) ignored by dirtyIgnore: {shown}",
                scan.ignored.len()
            ));
        }
    }
    if scan.is_blocking() {
        for line in scan.blocking.iter().take(10) {
            logger.log(line);
        }
        let message = if scan.ignored.is_empty() {
            "Working tree is not clean; project skipped.".to_string()
        } else {
            format!(
                "Working tree is not clean ({} change(s); {} ignored by dirtyIgnore); project skipped.",
                scan.blocking.len(),
                scan.ignored.len()
            )
        };
        return Ok(SyncResult::Dirty(message));
    }

    let branch_output = ensure_git(
        git(
            path,
            git_path,
            runner,
            logger,
            &["rev-parse", "--abbrev-ref", "HEAD"],
            PREFLIGHT_TIMEOUT,
        )?,
        "Failed to read the Git branch",
    )?;
    let active_branch = branch_output.stdout.trim().to_string();
    if !branch.trim().is_empty() && !active_branch.eq_ignore_ascii_case(branch.trim()) {
        return Err(format!(
            "Active branch is '{active_branch}', not '{}'.",
            branch.trim()
        ));
    }
    logger.log(&format!("Branch: {active_branch}"));

    let before_output = ensure_git(
        git(
            path,
            git_path,
            runner,
            logger,
            &["rev-parse", "HEAD"],
            PREFLIGHT_TIMEOUT,
        )?,
        "Failed to read HEAD SHA",
    )?;
    let before = before_output.stdout.trim().to_string();
    logger.log(&format!("SHA before: {before}"));

    let probe = git(
        path,
        git_path,
        runner,
        logger,
        &["ls-remote", "--heads", "origin"],
        PREFLIGHT_TIMEOUT,
    )?;
    if !probe.success() || is_auth_failure(&probe) {
        return Err(format!(
            "Remote access failed. Git credentials are missing or invalid. {}",
            probe.combined()
        ));
    }

    let fetch = run_command_owned(
        runner,
        git_path,
        &["fetch", "--prune", "origin"],
        path,
        FETCH_TIMEOUT,
        logger,
    )?;
    if !fetch.success() || is_auth_failure(&fetch) {
        return Err(format!(
            "git fetch failed. Git credentials are missing or invalid. {}",
            fetch.combined()
        ));
    }

    let origin_ref = format!("origin/{active_branch}");
    let verify = git(
        path,
        git_path,
        runner,
        logger,
        &["rev-parse", "--verify", &origin_ref],
        PREFLIGHT_TIMEOUT,
    )?;
    if !verify.success() {
        return Err(format!("Remote {origin_ref} was not found."));
    }

    let incoming_output = ensure_git(
        git(
            path,
            git_path,
            runner,
            logger,
            &["log", "--oneline", &format!("HEAD..{origin_ref}")],
            PREFLIGHT_TIMEOUT,
        )?,
        "Failed to read incoming commits",
    )?;
    let incoming = split_lines(&incoming_output.stdout);
    if incoming.is_empty() {
        let message = "Already up to date; no new commits.".to_string();
        logger.log(&message);
        return Ok(SyncResult::UpToDate(message));
    }

    logger.log(&format!("New commits ({}):", incoming.len()));
    for line in incoming.iter().take(30) {
        logger.log(line);
    }

    let preview_output = ensure_git(
        git(
            path,
            git_path,
            runner,
            logger,
            &["diff", "--name-only", &format!("HEAD..{origin_ref}")],
            PREFLIGHT_TIMEOUT,
        )?,
        "Failed to read the changed file list",
    )?;
    let preview = split_lines(&preview_output.stdout);
    logger.log("Changed files:");
    for file in preview.iter().take(40) {
        logger.log(file);
    }

    let migration_files: Vec<String> = preview
        .iter()
        .filter(|file| sm_core::update::is_migration_change(file))
        .cloned()
        .collect();
    if !migration_files.is_empty() {
        logger.log("MIGRATION INFO: migration changes detected; the automatic update will stop.");
        for file in &migration_files {
            logger.log(&format!("MIGRATION: {file}"));
        }
    }

    if options.check_only {
        logger.log("Check finished. No files were changed.");
        return Ok(SyncResult::Check(incoming.len()));
    }

    if !migration_files.is_empty() {
        logger.log("CANCELLED: migrations must be run manually. No pull was performed.");
        return Ok(SyncResult::BlockedMigration(migration_files.len()));
    }

    before_merge(&preview)?;

    logger.log(&format!("git merge --ff-only {origin_ref}"));
    let merge = run_command_owned(
        runner,
        git_path,
        &["merge", "--ff-only", &origin_ref],
        path,
        FETCH_TIMEOUT,
        logger,
    )?;
    if !merge.success() {
        return Err(format!("git merge --ff-only failed: {}", merge.combined()));
    }

    let after_output = ensure_git(
        git(
            path,
            git_path,
            runner,
            logger,
            &["rev-parse", "HEAD"],
            PREFLIGHT_TIMEOUT,
        )?,
        "Failed to read HEAD SHA after pull",
    )?;
    let after = after_output.stdout.trim().to_string();
    if before == after {
        logger.log("Already up to date; no changes.");
        return Ok(SyncResult::UpToDate(
            "Already up to date; no changes.".to_string(),
        ));
    }
    logger.log(&format!("New commits: {before}..{after}"));

    let changed_output = ensure_git(
        git(
            path,
            git_path,
            runner,
            logger,
            &["diff", "--name-only", &before, &after],
            PREFLIGHT_TIMEOUT,
        )?,
        "Failed to read changed files",
    )?;
    let changed = split_lines(&changed_output.stdout);

    Ok(SyncResult::Merged(SyncOutcome {
        before,
        after,
        changed,
    }))
}

pub(crate) fn ensure_git(output: Output, context: &str) -> Result<Output, String> {
    if output.success() {
        Ok(output)
    } else {
        Err(format!("{context}: {}", output.combined()))
    }
}

pub(crate) fn is_auth_failure(output: &Output) -> bool {
    let text = output.combined().to_ascii_lowercase();
    text.contains("fatal:")
        || text.contains("could not read username")
        || text.contains("authentication failed")
        || text.contains("terminal prompts disabled")
}

pub(crate) fn git(
    path: &Path,
    git_path: &Path,
    runner: &dyn Runner,
    logger: &UpdateLogger,
    args: &[&str],
    timeout: Duration,
) -> Result<Output, String> {
    let args: Vec<&str> = args.to_vec();
    run_command_owned(runner, git_path, &args, path, timeout, logger)
}

pub(crate) fn run_tool(
    runner: &dyn Runner,
    program: &Path,
    args: &[&str],
    cwd: &Path,
    logger: &UpdateLogger,
) -> Result<Output, String> {
    let output = run_command_owned(runner, program, args, cwd, TOOL_TIMEOUT, logger)?;
    if !output.success() {
        return Err(format!(
            "Command '{}' failed (exit {}): {}",
            program.display(),
            output.exit_code,
            output.combined()
        ));
    }
    Ok(output)
}

pub(crate) fn run_command_owned(
    runner: &dyn Runner,
    program: &Path,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
    logger: &UpdateLogger,
) -> Result<Output, String> {
    let args: Vec<String> = args.iter().map(|value| value.to_string()).collect();
    run_command(runner, program, &args, cwd, timeout, logger)
}

pub(crate) fn run_command(
    runner: &dyn Runner,
    program: &Path,
    args: &[String],
    cwd: &Path,
    timeout: Duration,
    logger: &UpdateLogger,
) -> Result<Output, String> {
    run_command_with_env(
        runner,
        program,
        args,
        cwd,
        timeout,
        &BTreeMap::new(),
        logger,
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_command_with_env(
    runner: &dyn Runner,
    program: &Path,
    args: &[String],
    cwd: &Path,
    timeout: Duration,
    env: &BTreeMap<String, String>,
    logger: &UpdateLogger,
) -> Result<Output, String> {
    logger.log(&format!(
        "$ {} {}",
        program.display(),
        args.iter()
            .map(|arg| if arg.contains(' ') {
                format!("\"{arg}\"")
            } else {
                arg.clone()
            })
            .collect::<Vec<_>>()
            .join(" ")
    ));

    let (program, args) = wrap_command(program, args);
    let output = runner
        .run_with_env(&program, &args, cwd, timeout, env)
        .map_err(|error| error.to_string())?;

    let combined = output.combined();
    if !combined.is_empty() {
        for line in combined.lines().take(60) {
            logger.log(line);
        }
    }
    Ok(output)
}

pub(crate) fn split_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect()
}

pub(crate) fn powershell_path() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join("WindowsPowerShell")
        .join("v1.0")
        .join("powershell.exe")
}

pub(crate) fn shell_command(line: &str) -> (PathBuf, Vec<String>) {
    (
        powershell_path(),
        vec![
            "-NoProfile".to_string(),
            "-ExecutionPolicy".to_string(),
            "Bypass".to_string(),
            "-Command".to_string(),
            line.to_string(),
        ],
    )
}
