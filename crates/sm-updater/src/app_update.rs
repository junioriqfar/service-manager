use std::cell::RefCell;
use std::path::Path;

use crate::log::UpdateLogger;
use crate::registry::{AppConfig, CommandSpec, UpdaterSettings};
use crate::runner::Runner;
use crate::services::{Lifecycle, ServiceControl};
use crate::sync::{run_command_with_env, shell_command, sync_project, SyncResult};
use crate::toolchain::{self, PlannedCommand, ToolchainPlan};
use crate::update::{UpdateOptions, UpdateOutcome, UpdateStatus};

pub fn update_app(
    app: &AppConfig,
    settings: &UpdaterSettings,
    log_dir: &Path,
    runner: &dyn Runner,
    services: &dyn ServiceControl,
    options: UpdateOptions,
) -> UpdateOutcome {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let project_path = Path::new(&app.local_path);
    let logger = UpdateLogger::new(log_dir, project_path, &stamp);
    logger.log(&format!(
        "===== {}: {} =====",
        app.id,
        if options.check_only { "CHECK" } else { "AUTO" }
    ));

    let result = execute_app(app, settings, runner, services, &logger, options);
    let (status, message) = match result {
        Ok(value) => value,
        Err(message) => {
            logger.log(&format!("GAGAL: {message}"));
            (UpdateStatus::Failed, message)
        }
    };

    if status == UpdateStatus::Updated {
        logger.log("DEPLOY SELESAI");
    } else if status == UpdateStatus::Degraded {
        logger.log(&format!("DEGRADED: {message}"));
    }

    UpdateOutcome {
        project: app.id.clone(),
        status,
        message,
        log_path: Some(logger.run_log_path().to_path_buf()),
    }
}

fn execute_app(
    app: &AppConfig,
    settings: &UpdaterSettings,
    runner: &dyn Runner,
    services: &dyn ServiceControl,
    logger: &UpdateLogger,
    options: UpdateOptions,
) -> Result<(UpdateStatus, String), String> {
    let project = Path::new(&app.local_path);
    if !project.is_dir() {
        return Err(format!(
            "Folder project tidak ditemukan: {}",
            project.display()
        ));
    }
    if app.branch.trim().is_empty() {
        return Err(format!("Branch app '{}' belum diisi.", app.id));
    }

    let lifecycle = Lifecycle::new(services, app.services.wait_timeout_seconds);
    if options.check_only {
        lifecycle.validate_readonly(&app.services)?;
    } else {
        lifecycle.validate(&app.services)?;
    }

    let plan_cell: RefCell<Option<ToolchainPlan>> = RefCell::new(None);
    let git_path = Path::new(settings.git_path.trim());

    let sync = sync_project(
        project,
        &app.branch,
        git_path,
        runner,
        logger,
        options,
        |preview| {
            let plan = toolchain::resolve_plan(app, project, preview, settings)?;
            for note in &plan.notes {
                logger.log(&format!("INFO: {note}"));
            }
            *plan_cell.borrow_mut() = Some(plan);
            run_custom_list(
                &app.commands.pre_update,
                project,
                runner,
                logger,
                "pre-update",
            )?;
            run_shell_option(
                &app.pre_update_command,
                project,
                runner,
                logger,
                "pre-update command",
            )?;
            lifecycle.stop_before_pull(&app.services)
        },
    );

    let sync = match sync {
        Ok(value) => value,
        Err(error) => {
            restore_services(&lifecycle, logger);
            return Err(error);
        }
    };

    let outcome = match sync {
        SyncResult::UpToDate(message) => {
            restore_services(&lifecycle, logger);
            return Ok((UpdateStatus::UpToDate, message));
        }
        SyncResult::Dirty(message) => {
            restore_services(&lifecycle, logger);
            return Ok((UpdateStatus::Dirty, message));
        }
        SyncResult::Check(count) => {
            restore_services(&lifecycle, logger);
            return Ok((
                UpdateStatus::Check,
                format!("{count} commit baru terdeteksi (dry-run)."),
            ));
        }
        SyncResult::BlockedMigration(count) => {
            restore_services(&lifecycle, logger);
            return Ok((
                UpdateStatus::BlockedMigration,
                format!("Update diblokir: {count} file migration harus dijalankan manual."),
            ));
        }
        SyncResult::Merged(outcome) => outcome,
    };

    let plan = plan_cell
        .into_inner()
        .ok_or_else(|| "Rencana toolchain tidak tersedia.".to_string())?;

    if let Err(error) = lifecycle.stop_before_build(&app.services) {
        restore_services(&lifecycle, logger);
        return Err(error);
    }

    if let Err(error) = execute_plan(&plan, runner, logger) {
        restore_services(&lifecycle, logger);
        return Err(error);
    }

    if let Err(error) = run_custom_list(
        &app.commands.post_update,
        project,
        runner,
        logger,
        "post-update",
    )
    .and_then(|_| {
        run_shell_option(
            &app.post_update_command,
            project,
            runner,
            logger,
            "post-update command",
        )
    }) {
        restore_services(&lifecycle, logger);
        return Err(error);
    }

    if let Err(error) = lifecycle.finish(&app.services) {
        return Ok((
            UpdateStatus::Degraded,
            format!("Update diterapkan tetapi service gagal dipulihkan: {error}"),
        ));
    }

    logger.log(&format!(
        "Rollback kode: git reset --hard {}",
        outcome.before
    ));
    Ok((UpdateStatus::Updated, "Update berhasil.".to_string()))
}

fn execute_plan(
    plan: &ToolchainPlan,
    runner: &dyn Runner,
    logger: &UpdateLogger,
) -> Result<(), String> {
    for (stage, commands) in [
        ("install", &plan.install),
        ("build", &plan.build),
        ("artisan", &plan.artisan),
    ] {
        for command in commands {
            logger.log(&format!("{stage}: {}", command.label));
            run_planned(command, runner, logger)?;
        }
    }
    Ok(())
}

fn run_planned(
    command: &PlannedCommand,
    runner: &dyn Runner,
    logger: &UpdateLogger,
) -> Result<(), String> {
    let output = run_command_with_env(
        runner,
        &command.program,
        &command.args,
        &command.cwd,
        command.timeout,
        &command.env,
        logger,
    )?;
    if !output.success() {
        return Err(format!(
            "{} gagal (exit {}): {}",
            command.label,
            output.exit_code,
            output.combined()
        ));
    }
    Ok(())
}

fn run_custom_list(
    specs: &[CommandSpec],
    project: &Path,
    runner: &dyn Runner,
    logger: &UpdateLogger,
    stage: &str,
) -> Result<(), String> {
    for spec in specs {
        let result = toolchain::planned_from_spec(spec, project)
            .and_then(|command| run_planned(&command, runner, logger));
        if let Err(error) = result {
            if spec.continue_on_error {
                logger.log(&format!("WARNING {stage}: {error}"));
            } else {
                return Err(format!("{stage} gagal: {error}"));
            }
        }
    }
    Ok(())
}

fn run_shell_option(
    line: &str,
    project: &Path,
    runner: &dyn Runner,
    logger: &UpdateLogger,
    label: &str,
) -> Result<(), String> {
    let line = line.trim();
    if line.is_empty() {
        return Ok(());
    }
    logger.log(&format!("{label}: {line}"));
    let (program, args) = shell_command(line);
    let output = run_command_with_env(
        runner,
        &program,
        &args,
        project,
        crate::sync::TOOL_TIMEOUT,
        &Default::default(),
        logger,
    )?;
    if !output.success() {
        return Err(format!(
            "{label} gagal (exit {}): {}",
            output.exit_code,
            output.combined()
        ));
    }
    Ok(())
}

fn restore_services(lifecycle: &Lifecycle<'_>, logger: &UpdateLogger) {
    for error in lifecycle.best_effort_restore() {
        logger.log(&format!("WARNING service: {error}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{AppConfig, Preset, ServicePlan};
    use crate::runner::{Output, RunnerError};
    use crate::services::ServiceControl;
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
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

    #[derive(Default)]
    struct FakeServices {
        known: BTreeSet<String>,
        running: Mutex<BTreeSet<String>>,
        calls: Mutex<Vec<String>>,
    }

    impl FakeServices {
        fn new(known: &[&str], running: &[&str]) -> Self {
            Self {
                known: known.iter().map(|value| value.to_string()).collect(),
                running: Mutex::new(running.iter().map(|value| value.to_string()).collect()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    impl ServiceControl for FakeServices {
        fn exists(&self, name: &str) -> Result<bool, String> {
            Ok(self.known.contains(name))
        }

        fn is_running(&self, name: &str) -> Result<bool, String> {
            Ok(self.running.lock().unwrap().contains(name))
        }

        fn stop(&self, name: &str, _timeout: Duration) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("stop:{name}"));
            self.running.lock().unwrap().remove(name);
            Ok(())
        }

        fn start(&self, name: &str, _timeout: Duration) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("start:{name}"));
            self.running.lock().unwrap().insert(name.to_string());
            Ok(())
        }

        fn restart(&self, name: &str, _timeout: Duration) -> Result<(), String> {
            self.calls.lock().unwrap().push(format!("restart:{name}"));
            self.running.lock().unwrap().insert(name.to_string());
            Ok(())
        }
    }

    struct Fixture {
        _temp: tempfile::TempDir,
        app: AppConfig,
        settings: UpdaterSettings,
        log_dir: PathBuf,
    }

    fn fixture() -> Fixture {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("demo");
        std::fs::create_dir_all(project.join(".git")).unwrap();
        std::fs::write(
            project.join("package.json"),
            r#"{"scripts":{"build":"vite build"}}"#,
        )
        .unwrap();
        std::fs::write(project.join("package-lock.json"), "{}").unwrap();
        std::fs::write(project.join("index.html"), "hello").unwrap();

        let tools = temp.path().join("tools");
        std::fs::create_dir_all(&tools).unwrap();
        for name in ["git.exe", "php.exe", "composer.bat", "npm.cmd"] {
            std::fs::write(tools.join(name), b"dummy").unwrap();
        }
        let settings = UpdaterSettings {
            git_path: tools.join("git.exe").to_string_lossy().to_string(),
            php_path: tools.join("php.exe").to_string_lossy().to_string(),
            composer_path: tools.join("composer.bat").to_string_lossy().to_string(),
            npm_path: tools.join("npm.cmd").to_string_lossy().to_string(),
            ..UpdaterSettings::default()
        };
        let app = AppConfig {
            id: "demo".into(),
            name: "Demo".into(),
            repo_url: "https://example.com/demo.git".into(),
            local_path: project.to_string_lossy().to_string(),
            branch: "main".into(),
            preset: Preset::Node,
            package_install: sm_core::ToggleMode::Always,
            build: sm_core::ToggleMode::Always,
            services: ServicePlan {
                stop_before_build: vec!["api".into()],
                restart_after: vec!["api".into()],
                ..ServicePlan::default()
            },
            ..AppConfig::default()
        };
        let log_dir = temp.path().join("logs");
        Fixture {
            _temp: temp,
            app,
            settings,
            log_dir,
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
    fn successful_app_update_stops_builds_and_restarts_service() {
        let fixture = fixture();
        let services = FakeServices::new(&["api"], &["api"]);
        let runner = FakeRunner::new(standard_handler(
            "abc1234 fix\n",
            "src/app.ts\n",
            "src/app.ts\n",
        ));

        let outcome = update_app(
            &fixture.app,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            &services,
            UpdateOptions::default(),
        );

        assert_eq!(outcome.status, UpdateStatus::Updated, "{}", outcome.message);
        assert_eq!(services.calls(), ["stop:api", "start:api"]);
        assert!(runner.has_call("vite build") || runner.has_call("build"));
    }

    #[test]
    fn check_mode_does_not_touch_services() {
        let fixture = fixture();
        let services = FakeServices::new(&["api"], &["api"]);
        let runner = FakeRunner::new(standard_handler("abc1234 fix\n", "src/app.ts\n", ""));

        let outcome = update_app(
            &fixture.app,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            &services,
            UpdateOptions { check_only: true },
        );

        assert_eq!(outcome.status, UpdateStatus::Check);
        assert!(services.calls().is_empty());
        assert!(!runner.has_call("merge"));
    }

    #[test]
    fn build_failure_restores_stopped_service() {
        let fixture = fixture();
        let services = FakeServices::new(&["api"], &["api"]);
        let inner = standard_handler("abc1234 fix\n", "src/app.ts\n", "src/app.ts\n");
        let runner = FakeRunner::new(move |_program, args| {
            if args.iter().any(|arg| arg.contains("npm.cmd")) {
                return fail("vite build failed");
            }
            inner(_program, args)
        });

        let outcome = update_app(
            &fixture.app,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            &services,
            UpdateOptions::default(),
        );

        assert_eq!(outcome.status, UpdateStatus::Failed);
        assert!(outcome.message.contains("vite build failed"));
        assert_eq!(services.calls(), ["stop:api", "start:api"]);
    }

    #[test]
    fn missing_service_fails_before_touching_git() {
        let fixture = fixture();
        let services = FakeServices::new(&[], &[]);
        let runner = FakeRunner::new(standard_handler("", "", ""));

        let outcome = update_app(
            &fixture.app,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            &services,
            UpdateOptions::default(),
        );

        assert_eq!(outcome.status, UpdateStatus::Failed);
        assert!(outcome.message.contains("api"));
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn custom_pre_and_post_commands_are_executed() {
        let mut fixture = fixture();
        fixture.app.commands.pre_update = vec![CommandSpec {
            name: "pre".into(),
            program: "custom-executable".into(),
            args: vec!["pre".into()],
            ..CommandSpec::default()
        }];
        fixture.app.commands.post_update = vec![CommandSpec {
            name: "post".into(),
            program: "custom-executable".into(),
            args: vec!["post".into()],
            ..CommandSpec::default()
        }];
        let services = FakeServices::new(&["api"], &["api"]);
        let runner = FakeRunner::new(standard_handler(
            "abc1234 fix\n",
            "src/app.ts\n",
            "src/app.ts\n",
        ));

        let outcome = update_app(
            &fixture.app,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            &services,
            UpdateOptions::default(),
        );

        assert_eq!(outcome.status, UpdateStatus::Updated, "{}", outcome.message);
        assert!(runner
            .calls()
            .iter()
            .any(|call| { call.program.contains("custom-executable") && call.args == ["pre"] }));
        assert!(runner
            .calls()
            .iter()
            .any(|call| { call.program.contains("custom-executable") && call.args == ["post"] }));
        let calls = runner.calls();
        let pre = calls.iter().position(|call| call.args == ["pre"]).unwrap();
        let merge = calls
            .iter()
            .position(|call| call.args.first().map(String::as_str) == Some("merge"))
            .unwrap();
        let post = calls.iter().position(|call| call.args == ["post"]).unwrap();
        assert!(pre < merge);
        assert!(merge < post);
    }

    #[test]
    fn app_update_reports_degraded_when_service_start_fails() {
        struct FailingStart(FakeServices);
        impl ServiceControl for FailingStart {
            fn exists(&self, name: &str) -> Result<bool, String> {
                self.0.exists(name)
            }
            fn is_running(&self, name: &str) -> Result<bool, String> {
                self.0.is_running(name)
            }
            fn stop(&self, name: &str, timeout: Duration) -> Result<(), String> {
                self.0.stop(name, timeout)
            }
            fn start(&self, name: &str, timeout: Duration) -> Result<(), String> {
                self.0.start(name, timeout)?;
                Err("start failed".to_string())
            }
            fn restart(&self, name: &str, timeout: Duration) -> Result<(), String> {
                self.0.restart(name, timeout)
            }
        }

        let fixture = fixture();
        let services = FailingStart(FakeServices::new(&["api"], &["api"]));
        let runner = FakeRunner::new(standard_handler(
            "abc1234 fix\n",
            "src/app.ts\n",
            "src/app.ts\n",
        ));

        let outcome = update_app(
            &fixture.app,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            &services,
            UpdateOptions::default(),
        );

        assert_eq!(outcome.status, UpdateStatus::Degraded);
        assert!(outcome.message.contains("start failed"));
    }

    #[test]
    fn build_commands_are_ordered_install_then_build() {
        let fixture = fixture();
        let services = FakeServices::new(&["api"], &["api"]);
        let runner = FakeRunner::new(standard_handler(
            "abc1234 fix\n",
            "src/app.ts\n",
            "src/app.ts\n",
        ));

        let outcome = update_app(
            &fixture.app,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            &services,
            UpdateOptions::default(),
        );
        assert_eq!(outcome.status, UpdateStatus::Updated, "{}", outcome.message);
        let calls = runner.calls();
        let install = calls
            .iter()
            .position(|call| call.args.iter().any(|arg| arg == "ci"))
            .unwrap();
        let build = calls
            .iter()
            .position(|call| {
                call.args.iter().any(|arg| arg == "run")
                    && call.args.iter().any(|arg| arg == "build")
            })
            .unwrap();
        assert!(install < build);
    }

    #[test]
    fn merge_failure_leaves_service_untouched() {
        let fixture = fixture();
        let services = FakeServices::new(&["api"], &["api"]);
        let inner = standard_handler("abc1234 fix\n", "src/app.ts\n", "src/app.ts\n");
        let runner = FakeRunner::new(move |program, args| {
            if args.first().map(String::as_str) == Some("merge") {
                return fail("merge failed");
            }
            inner(program, args)
        });

        let outcome = update_app(
            &fixture.app,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            &services,
            UpdateOptions::default(),
        );

        assert_eq!(outcome.status, UpdateStatus::Failed);
        assert!(outcome.message.contains("merge failed"));
        assert!(services.calls().is_empty());
    }

    #[test]
    fn service_calls_are_empty_when_no_incoming_commits() {
        let fixture = fixture();
        let services = FakeServices::new(&["api"], &["api"]);
        let runner = FakeRunner::new(standard_handler("", "", ""));

        let outcome = update_app(
            &fixture.app,
            &fixture.settings,
            &fixture.log_dir,
            &runner,
            &services,
            UpdateOptions::default(),
        );

        assert_eq!(outcome.status, UpdateStatus::UpToDate);
        assert!(services.calls().is_empty());
    }
}
