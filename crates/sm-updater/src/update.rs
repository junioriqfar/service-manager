use std::path::{Path, PathBuf};

use sm_config::ProjectsFile;
use sm_core::plan_update;

use crate::log::UpdateLogger;
use crate::runner::Runner;
use crate::scan::{is_laravel_project, ProjectInfo};
use crate::sync::{powershell_path, run_tool, sync_project, SyncResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateStatus {
    UpToDate,
    Check,
    Updated,
    Dirty,
    BlockedMigration,
    Degraded,
    Failed,
}

impl UpdateStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UpToDate => "up-to-date",
            Self::Check => "check",
            Self::Updated => "updated",
            Self::Dirty => "dirty",
            Self::BlockedMigration => "blocked-migration",
            Self::Degraded => "degraded",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UpdateOptions {
    pub check_only: bool,
}

#[derive(Debug, Clone)]
pub struct UpdateOutcome {
    pub project: String,
    pub status: UpdateStatus,
    pub message: String,
    pub log_path: Option<PathBuf>,
}

pub fn update_project(
    project: &ProjectInfo,
    settings: &ProjectsFile,
    log_dir: &Path,
    runner: &dyn Runner,
    options: UpdateOptions,
) -> UpdateOutcome {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let logger = UpdateLogger::new(log_dir, &project.path, &stamp);
    logger.log(&format!(
        "===== {}: {} =====",
        project.name,
        if options.check_only { "CHECK" } else { "AUTO" }
    ));

    let result = execute(project, settings, runner, &logger, options);
    let (status, message) = match result {
        Ok(value) => value,
        Err(message) => {
            logger.log(&format!("GAGAL: {message}"));
            (UpdateStatus::Failed, message)
        }
    };

    if status == UpdateStatus::Updated {
        logger.log("DEPLOY SELESAI");
    }

    UpdateOutcome {
        project: project.name.clone(),
        status,
        message,
        log_path: Some(logger.run_log_path().to_path_buf()),
    }
}

fn execute(
    project: &ProjectInfo,
    settings: &ProjectsFile,
    runner: &dyn Runner,
    logger: &UpdateLogger,
    options: UpdateOptions,
) -> Result<(UpdateStatus, String), String> {
    if !is_laravel_project(&project.path) {
        return Err(format!(
            "Project bukan Laravel yang valid: {}",
            project.path.display()
        ));
    }

    let git_path = Path::new(settings.git_path.trim());
    let result = sync_project(
        &project.path,
        &project.config.branch,
        git_path,
        runner,
        logger,
        options,
        |_| Ok(()),
    )?;

    let outcome = match result {
        SyncResult::UpToDate(message) => return Ok((UpdateStatus::UpToDate, message)),
        SyncResult::Dirty(message) => return Ok((UpdateStatus::Dirty, message)),
        SyncResult::Check(count) => {
            return Ok((
                UpdateStatus::Check,
                format!("{count} commit baru terdeteksi (dry-run)."),
            ))
        }
        SyncResult::BlockedMigration(count) => {
            return Ok((
                UpdateStatus::BlockedMigration,
                format!("Update diblokir: {count} file migration harus dijalankan manual."),
            ))
        }
        SyncResult::Merged(outcome) => outcome,
    };

    let has_package_lock = project.path.join("package-lock.json").is_file();
    let cache_files_exist = [
        "bootstrap/cache/config.php",
        "bootstrap/cache/routes.php",
        "bootstrap/cache/routes-v7.php",
    ]
    .iter()
    .any(|file| project.path.join(file).is_file());

    let plan = plan_update(
        &outcome.changed,
        &project.config,
        has_package_lock,
        cache_files_exist,
    );

    if plan.composer_install {
        let composer = Path::new(settings.composer_path.trim());
        if !composer.is_file() {
            return Err(format!("Composer tidak ditemukan: {}", composer.display()));
        }
        run_tool(
            runner,
            composer,
            &[
                "install",
                "--no-dev",
                "--optimize-autoloader",
                "--no-interaction",
            ],
            &project.path,
            logger,
        )?;
    }

    if plan.npm_build {
        let npm = Path::new(settings.npm_path.trim());
        if !npm.is_file() {
            return Err(format!("npm tidak ditemukan: {}", npm.display()));
        }
        if plan.npm_ci {
            run_tool(runner, npm, &["ci"], &project.path, logger)?;
        } else if plan.npm_install {
            run_tool(runner, npm, &["install"], &project.path, logger)?;
        }
        run_tool(runner, npm, &["run", "build"], &project.path, logger)?;
    }

    if plan.optimize_clear || plan.queue_restart {
        let php = Path::new(settings.php_path.trim());
        if !php.is_file() {
            return Err(format!("PHP tidak ditemukan: {}", php.display()));
        }
        if plan.optimize_clear {
            run_tool(
                runner,
                php,
                &["artisan", "optimize:clear"],
                &project.path,
                logger,
            )?;
        }
        if plan.queue_restart {
            run_tool(
                runner,
                php,
                &["artisan", "queue:restart"],
                &project.path,
                logger,
            )?;
        }
    }

    if !project.config.post_update_command.trim().is_empty() {
        logger.log("Post-update command");
        let powershell = powershell_path();
        run_tool(
            runner,
            &powershell,
            &[
                "-NoProfile",
                "-ExecutionPolicy",
                "Bypass",
                "-Command",
                project.config.post_update_command.trim(),
            ],
            &project.path,
            logger,
        )?;
    }

    logger.log(&format!(
        "Rollback kode: git reset --hard {}",
        outcome.before
    ));
    Ok((UpdateStatus::Updated, "Update berhasil.".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{Output, RunnerError};
    use crate::scan::ProjectInfo;
    use sm_core::ProjectOverride;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    #[derive(Debug, Clone)]
    struct Call {
        program: String,
        args: Vec<String>,
    }

    type Handler = Box<dyn Fn(&str, &[String]) -> Output + Send + Sync>;

    struct FakeRunner {
        calls: Mutex<Vec<Call>>,
        handler: Handler,
    }

    impl FakeRunner {
        fn new(handler: impl Fn(&str, &[String]) -> Output + Send + Sync + 'static) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                handler: Box::new(handler),
            }
        }

        fn calls(&self) -> Vec<Call> {
            self.calls.lock().unwrap().clone()
        }

        fn has_call(&self, needle: &str) -> bool {
            self.calls().iter().any(|call| {
                call.program.to_ascii_lowercase().contains(needle)
                    || call
                        .args
                        .iter()
                        .any(|arg| arg.to_ascii_lowercase().contains(needle))
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
            Ok((self.handler)(&program.to_string_lossy(), args))
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

    struct Fixture {
        _temp: tempfile::TempDir,
        project: ProjectInfo,
        settings: ProjectsFile,
        log_dir: PathBuf,
    }

    fn fixture() -> Fixture {
        let temp = tempfile::tempdir().unwrap();
        let project_path = temp.path().join("kasirin");
        std::fs::create_dir_all(project_path.join(".git")).unwrap();
        std::fs::write(project_path.join("artisan"), b"php").unwrap();
        std::fs::write(project_path.join("composer.json"), b"{}").unwrap();
        std::fs::write(project_path.join("package-lock.json"), b"{}").unwrap();
        std::fs::create_dir_all(project_path.join("bootstrap/cache")).unwrap();
        std::fs::write(project_path.join("bootstrap/cache/config.php"), b"cache").unwrap();

        let tools = temp.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        for name in ["git.exe", "php.exe", "composer.bat", "npm.cmd"] {
            std::fs::write(tools.join(name), b"dummy").unwrap();
        }

        let settings = ProjectsFile {
            scan_roots: vec![temp.path().to_string_lossy().to_string()],
            git_path: tools.join("git.exe").to_string_lossy().to_string(),
            php_path: tools.join("php.exe").to_string_lossy().to_string(),
            composer_path: tools.join("composer.bat").to_string_lossy().to_string(),
            npm_path: tools.join("npm.cmd").to_string_lossy().to_string(),
            log_dir: temp.path().join("logs").to_string_lossy().to_string(),
            ..ProjectsFile::default()
        };

        let project = ProjectInfo {
            name: "kasirin".into(),
            path: project_path,
            config: ProjectOverride {
                branch: "main".into(),
                ..ProjectOverride::default()
            },
        };

        Fixture {
            log_dir: temp.path().join("logs"),
            _temp: temp,
            project,
            settings,
        }
    }

    fn standard_handler(
        incoming: &'static str,
        preview: &'static str,
        changed: &'static str,
    ) -> impl Fn(&str, &[String]) -> Output + Send + Sync {
        let head_reads = Arc::new(AtomicUsize::new(0));
        move |_program, args| {
            let first = args.first().map(String::as_str).unwrap_or("");
            match first {
                "status" => ok(""),
                "rev-parse" if args.get(1).map(String::as_str) == Some("--abbrev-ref") => {
                    ok("main\n")
                }
                "rev-parse" if args.get(1).map(String::as_str) == Some("HEAD") => {
                    let count = head_reads.fetch_add(1, Ordering::SeqCst);
                    if count == 0 {
                        ok("aaaa1111\n")
                    } else {
                        ok("bbbb2222\n")
                    }
                }
                "rev-parse" => ok("ok\n"),
                "ls-remote" => ok("refs/heads/main\n"),
                "fetch" => ok(""),
                "log" => ok(incoming),
                "diff" if args.iter().any(|arg| arg == "HEAD..origin/main") => ok(preview),
                "diff" => ok(changed),
                "merge" => ok("Fast-forward\n"),
                _ => ok(""),
            }
        }
    }

    #[test]
    fn successful_update_runs_conditional_steps() {
        let fixture = fixture();
        let runner = FakeRunner::new(standard_handler(
            "abc1234 fix: something\n",
            "app/Http/Controllers/Foo.php\nresources/js/app.ts\n",
            "composer.lock\npackage-lock.json\nresources/js/app.ts\napp/Http/Controllers/Foo.php\n",
        ));

        let outcome = update_project(
            &fixture.project,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            UpdateOptions::default(),
        );

        assert_eq!(outcome.status, UpdateStatus::Updated, "{}", outcome.message);
        assert!(runner.has_call("composer.bat"));
        assert!(runner.has_call("ci"));
        assert!(runner.has_call("run"));
        assert!(runner.has_call("optimize:clear"));
        assert!(runner.has_call("queue:restart"));
        assert!(outcome.log_path.unwrap().is_file());
    }

    #[test]
    fn up_to_date_when_no_incoming_commits() {
        let fixture = fixture();
        let runner = FakeRunner::new(standard_handler("", "", ""));
        let outcome = update_project(
            &fixture.project,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            UpdateOptions::default(),
        );
        assert_eq!(outcome.status, UpdateStatus::UpToDate);
        assert!(!runner.has_call("merge"));
    }

    #[test]
    fn check_mode_does_not_merge() {
        let fixture = fixture();
        let runner = FakeRunner::new(standard_handler("abc1234 fix\n", "app/Foo.php\n", ""));
        let outcome = update_project(
            &fixture.project,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            UpdateOptions { check_only: true },
        );
        assert_eq!(outcome.status, UpdateStatus::Check);
        assert!(!runner.has_call("merge"));
    }

    #[test]
    fn dirty_working_tree_is_skipped() {
        let fixture = fixture();
        let runner = FakeRunner::new(|_program, args| {
            if args.first().map(String::as_str) == Some("status") {
                ok(" M app/Foo.php\n")
            } else {
                ok("")
            }
        });
        let outcome = update_project(
            &fixture.project,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            UpdateOptions::default(),
        );
        assert_eq!(outcome.status, UpdateStatus::Dirty);
        assert!(!runner.has_call("fetch"));
    }

    #[test]
    fn branch_mismatch_fails() {
        let fixture = fixture();
        let runner = FakeRunner::new(|_program, args| {
            if args.first().map(String::as_str) == Some("status") {
                ok("")
            } else if args.first().map(String::as_str) == Some("rev-parse")
                && args.get(1).map(String::as_str) == Some("--abbrev-ref")
            {
                ok("develop\n")
            } else {
                ok("")
            }
        });
        let outcome = update_project(
            &fixture.project,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            UpdateOptions::default(),
        );
        assert_eq!(outcome.status, UpdateStatus::Failed);
        assert!(outcome.message.contains("Branch aktif"));
    }

    #[test]
    fn migration_change_blocks_update() {
        let fixture = fixture();
        let runner = FakeRunner::new(standard_handler(
            "abc1234 feat: migration\n",
            "database/migrations/2026_01_01_create_users.php\n",
            "",
        ));
        let outcome = update_project(
            &fixture.project,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            UpdateOptions::default(),
        );
        assert_eq!(outcome.status, UpdateStatus::BlockedMigration);
        assert!(!runner.has_call("merge"));
    }

    #[test]
    fn auth_failure_stops_before_fetch() {
        let fixture = fixture();
        let runner = FakeRunner::new(|_program, args| {
            if args.first().map(String::as_str) == Some("status") {
                ok("")
            } else if args.first().map(String::as_str) == Some("rev-parse")
                && args.get(1).map(String::as_str) == Some("--abbrev-ref")
            {
                ok("main\n")
            } else if args.first().map(String::as_str) == Some("rev-parse") {
                ok("aaaa\n")
            } else if args.first().map(String::as_str) == Some("ls-remote") {
                fail("fatal: Authentication failed for 'https://example/repo'")
            } else {
                ok("")
            }
        });
        let outcome = update_project(
            &fixture.project,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            UpdateOptions::default(),
        );
        assert_eq!(outcome.status, UpdateStatus::Failed);
        assert!(!runner.has_call("fetch"));
    }

    #[test]
    fn composer_failure_marks_update_failed() {
        let fixture = fixture();
        let inner = standard_handler("abc1234 fix\n", "app/Foo.php\n", "composer.lock\n");
        let runner = FakeRunner::new(move |program, args| {
            if args.iter().any(|arg| arg.contains("composer.bat")) {
                return fail("composer failed");
            }
            inner(program, args)
        });
        let outcome = update_project(
            &fixture.project,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            UpdateOptions::default(),
        );
        assert_eq!(outcome.status, UpdateStatus::Failed);
        assert!(
            outcome.message.contains("composer install gagal")
                || outcome.message.contains("composer failed")
        );
    }

    #[test]
    fn post_update_command_is_executed() {
        let mut fixture = fixture();
        fixture.project.config.post_update_command = "echo ok".into();
        let runner = FakeRunner::new(standard_handler(
            "abc1234 fix\n",
            "app/Foo.php\n",
            "app/Foo.php\n",
        ));
        let outcome = update_project(
            &fixture.project,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            UpdateOptions::default(),
        );
        assert_eq!(outcome.status, UpdateStatus::Updated, "{}", outcome.message);
        assert!(runner.calls().iter().any(|call| {
            call.program.to_ascii_lowercase().contains("powershell")
                && call.args.iter().any(|arg| arg == "echo ok")
        }));
    }
}
