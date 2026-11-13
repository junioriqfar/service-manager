use std::path::{Path, PathBuf};

use sm_core::{ServiceConfig, StartupMode};
use thiserror::Error;

use crate::error::ScmError;
use crate::process::{sc_exe_path, CommandOutput, CommandRunner, ProcessError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NssmCommand {
    pub args: Vec<String>,
}

impl NssmCommand {
    pub fn new(args: &[&str]) -> Self {
        Self {
            args: args.iter().map(|value| value.to_string()).collect(),
        }
    }

    pub fn to_args(&self) -> Vec<String> {
        self.args.clone()
    }
}

#[derive(Debug, Error)]
pub enum NssmError {
    #[error("NSSM tidak ditemukan di {0}")]
    NotFound(PathBuf),
    #[error("Perintah nssm gagal (exit {exit_code}): {output}")]
    CommandFailed { exit_code: i32, output: String },
    #[error(transparent)]
    Process(#[from] ProcessError),
    #[error(transparent)]
    Scm(#[from] ScmError),
}

pub struct Nssm<'a> {
    path: &'a Path,
    runner: &'a dyn CommandRunner,
}

impl<'a> Nssm<'a> {
    pub fn new(path: &'a Path, runner: &'a dyn CommandRunner) -> Self {
        Self { path, runner }
    }

    pub fn is_available(&self) -> bool {
        self.path.is_file()
    }

    pub fn path(&self) -> &Path {
        self.path
    }

    fn run(&self, args: &[String]) -> Result<CommandOutput, NssmError> {
        if !self.path.is_file() {
            return Err(NssmError::NotFound(self.path.to_path_buf()));
        }
        self.runner.run(self.path, args).map_err(NssmError::from)
    }

    fn run_ok(&self, args: &[String]) -> Result<CommandOutput, NssmError> {
        let output = self.run(args)?;
        if !output.success() {
            return Err(NssmError::CommandFailed {
                exit_code: output.exit_code,
                output: output.combined(),
            });
        }
        Ok(output)
    }

    pub fn get_value(&self, name: &str, parameter: &str) -> Result<Option<String>, NssmError> {
        self.get_value_with_sub(name, parameter, None)
    }

    pub fn get_value_with_sub(
        &self,
        name: &str,
        parameter: &str,
        sub_parameter: Option<&str>,
    ) -> Result<Option<String>, NssmError> {
        let mut args = vec!["get".to_string(), name.to_string(), parameter.to_string()];
        if let Some(sub_parameter) = sub_parameter {
            args.push(sub_parameter.to_string());
        }

        let output = self.run(&args)?;
        if !output.success() {
            return Ok(None);
        }
        let value = output.stdout.trim();
        if value.is_empty() {
            Ok(None)
        } else {
            Ok(Some(value.to_string()))
        }
    }

    pub fn set_value(&self, name: &str, parameter: &str, value: &str) -> Result<(), NssmError> {
        if value.is_empty() {
            let _ = self.run(&["reset".to_string(), name.to_string(), parameter.to_string()]);
            Ok(())
        } else {
            self.run_ok(&[
                "set".to_string(),
                name.to_string(),
                parameter.to_string(),
                value.to_string(),
            ])?;
            Ok(())
        }
    }

    pub fn install(&self, config: &ServiceConfig) -> Result<(), NssmError> {
        if !self.path.is_file() {
            return Err(NssmError::NotFound(self.path.to_path_buf()));
        }
        let install_args = vec![
            "install".to_string(),
            config.name.clone(),
            config.executable.clone(),
        ];

        if let Err(error) = self.run_ok(&install_args) {
            let _ = self.run(&[
                "remove".to_string(),
                config.name.clone(),
                "confirm".to_string(),
            ]);
            return Err(error);
        }

        if let Err(error) = self.apply(config) {
            self.remove_ignore_errors(&config.name);
            return Err(error);
        }
        Ok(())
    }

    pub fn apply(&self, config: &ServiceConfig) -> Result<(), NssmError> {
        if !self.path.is_file() {
            return Err(NssmError::NotFound(self.path.to_path_buf()));
        }

        for command in apply_commands(config) {
            self.run_ok(&command.args)?;
        }

        let failure_args = failure_action_args(config);
        if !failure_args.is_empty() {
            // Recovery action bersifat best-effort, sama seperti versi PowerShell.
            let _ = self.runner.run(&sc_exe_path(), &failure_args);
        }

        crate::scm::set_delayed_start(&config.name, config.startup == StartupMode::Delayed)?;
        Ok(())
    }

    pub fn remove(&self, name: &str) -> Result<(), NssmError> {
        self.run_ok(&[
            "remove".to_string(),
            name.to_string(),
            "confirm".to_string(),
        ])?;
        Ok(())
    }

    pub fn remove_ignore_errors(&self, name: &str) {
        let _ = self.run(&[
            "remove".to_string(),
            name.to_string(),
            "confirm".to_string(),
        ]);
    }
}

pub fn apply_commands(config: &ServiceConfig) -> Vec<NssmCommand> {
    let mut commands = Vec::new();
    let name = config.name.as_str();

    commands.push(set(name, "Application", &config.executable));

    let work_dir = if !config.working_directory.is_empty() {
        config.working_directory.clone()
    } else {
        Path::new(&config.executable)
            .parent()
            .map(|parent| parent.to_string_lossy().to_string())
            .unwrap_or_default()
    };
    commands.push(set_or_reset(name, "AppDirectory", &work_dir));

    let display_name = if config.display_name.is_empty() {
        name
    } else {
        config.display_name.as_str()
    };
    commands.push(set(name, "DisplayName", display_name));
    commands.push(set_or_reset(name, "AppParameters", &config.arguments));

    let stdout_log = config.stdout_log.as_str();
    let stderr_log = if config.stderr_log.is_empty() {
        stdout_log
    } else {
        config.stderr_log.as_str()
    };
    if stdout_log.is_empty() {
        commands.push(reset(name, "AppStdout"));
        commands.push(reset(name, "AppStderr"));
    } else {
        commands.push(set(name, "AppStdout", stdout_log));
        commands.push(set(name, "AppStderr", stderr_log));
        commands.push(set(name, "AppRotateFiles", "1"));
        commands.push(set(name, "AppRotateOnline", "1"));
        commands.push(set(name, "AppRotateBytes", "10485760"));
    }

    let start_value = match config.startup.normalized() {
        StartupMode::Manual => "SERVICE_DEMAND_START",
        StartupMode::Auto | StartupMode::Delayed | StartupMode::Unknown => "SERVICE_AUTO_START",
    };
    commands.push(set(name, "Start", start_value));

    if config.restart_on_crash {
        commands.push(NssmCommand::new(&[
            "set", name, "AppExit", "Default", "Restart",
        ]));
        let delay_ms = config.restart_delay_sec.saturating_mul(1000).to_string();
        commands.push(set(name, "AppRestartDelay", &delay_ms));
        commands.push(set(name, "AppThrottle", "5000"));
    } else {
        commands.push(NssmCommand::new(&[
            "set", name, "AppExit", "Default", "Exit",
        ]));
    }

    let dependencies: Vec<String> = config
        .normalized_dependencies()
        .into_iter()
        .filter(|dependency| !dependency.eq_ignore_ascii_case(name))
        .collect();
    if dependencies.is_empty() {
        commands.push(reset(name, "DependOnService"));
    } else {
        let mut args = vec![
            "set".to_string(),
            name.to_string(),
            "DependOnService".to_string(),
        ];
        args.extend(dependencies);
        commands.push(NssmCommand { args });
    }

    commands
}

/// Argumen `sc failure` untuk NSSM. Kosong bila restart on crash dinonaktifkan.
pub fn failure_action_args(config: &ServiceConfig) -> Vec<String> {
    if config.restart_on_crash {
        vec![
            "failure".to_string(),
            config.name.clone(),
            "reset=".to_string(),
            "86400".to_string(),
            "actions=".to_string(),
            "restart/5000/restart/10000/restart/30000".to_string(),
        ]
    } else {
        vec![
            "failure".to_string(),
            config.name.clone(),
            "reset=".to_string(),
            "0".to_string(),
            "actions=".to_string(),
            "/".to_string(),
        ]
    }
}

fn set(name: &str, parameter: &str, value: &str) -> NssmCommand {
    NssmCommand::new(&["set", name, parameter, value])
}

fn set_or_reset(name: &str, parameter: &str, value: &str) -> NssmCommand {
    if value.is_empty() {
        reset(name, parameter)
    } else {
        set(name, parameter, value)
    }
}

fn reset(name: &str, parameter: &str) -> NssmCommand {
    NssmCommand::new(&["reset", name, parameter])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::MockRunner;

    fn service() -> ServiceConfig {
        ServiceConfig {
            name: "my-app".into(),
            display_name: "My App".into(),
            executable: r"C:\apps\my-app\node.exe".into(),
            arguments: "index.js".into(),
            working_directory: r"C:\apps\my-app".into(),
            stdout_log: r"C:\apps\my-app\out.log".into(),
            stderr_log: r"C:\apps\my-app\err.log".into(),
            restart_delay_sec: 7,
            dependencies: vec!["Tcpip".into(), "postgresql-16".into()],
            ..ServiceConfig::default()
        }
    }

    fn arg_strings(command: &NssmCommand) -> Vec<String> {
        command.args.clone()
    }

    fn fake_nssm(temp: &tempfile::TempDir) -> PathBuf {
        let path = temp.path().join("nssm.exe");
        std::fs::write(&path, b"").unwrap();
        path
    }

    #[test]
    fn apply_builds_expected_nssm_commands() {
        let commands = apply_commands(&service());
        let flattened: Vec<Vec<String>> = commands.iter().map(arg_strings).collect();

        assert!(flattened.contains(
            &vec!["set", "my-app", "Application", r"C:\apps\my-app\node.exe"]
                .into_iter()
                .map(String::from)
                .collect()
        ));
        assert!(flattened.contains(
            &vec!["set", "my-app", "AppParameters", "index.js"]
                .into_iter()
                .map(String::from)
                .collect()
        ));
        assert!(flattened.contains(
            &vec!["set", "my-app", "AppRestartDelay", "7000"]
                .into_iter()
                .map(String::from)
                .collect()
        ));
        assert!(flattened.contains(
            &vec!["set", "my-app", "AppRotateBytes", "10485760"]
                .into_iter()
                .map(String::from)
                .collect()
        ));
        assert!(flattened.contains(
            &vec!["set", "my-app", "DependOnService", "Tcpip", "postgresql-16"]
                .into_iter()
                .map(String::from)
                .collect()
        ));
    }

    #[test]
    fn manual_startup_uses_demand_start() {
        let mut config = service();
        config.startup = StartupMode::Manual;
        let commands = apply_commands(&config);
        assert!(commands
            .iter()
            .any(|command| command.args == vec!["set", "my-app", "Start", "SERVICE_DEMAND_START"]));
    }

    #[test]
    fn empty_logs_and_parameters_are_reset() {
        let mut config = service();
        config.arguments = String::new();
        config.stdout_log = String::new();
        config.stderr_log = String::new();
        let commands = apply_commands(&config);
        assert!(commands
            .iter()
            .any(|command| command.args == vec!["reset", "my-app", "AppParameters"]));
        assert!(commands
            .iter()
            .any(|command| command.args == vec!["reset", "my-app", "AppStdout"]));
        assert!(commands
            .iter()
            .any(|command| command.args == vec!["reset", "my-app", "AppStderr"]));
    }

    #[test]
    fn missing_stderr_falls_back_to_stdout() {
        let mut config = service();
        config.stderr_log = String::new();
        let commands = apply_commands(&config);
        assert!(commands
            .iter()
            .any(|command| command.args
                == vec!["set", "my-app", "AppStderr", r"C:\apps\my-app\out.log"]));
    }

    #[test]
    fn restart_disabled_sets_exit() {
        let mut config = service();
        config.restart_on_crash = false;
        let commands = apply_commands(&config);
        assert!(commands
            .iter()
            .any(|command| command.args == vec!["set", "my-app", "AppExit", "Default", "Exit"]));
        assert!(failure_action_args(&config).contains(&"/".to_string()));
    }

    #[test]
    fn empty_dependencies_are_reset() {
        let mut config = service();
        config.dependencies.clear();
        let commands = apply_commands(&config);
        assert!(commands
            .iter()
            .any(|command| command.args == vec!["reset", "my-app", "DependOnService"]));
    }

    #[test]
    fn self_dependency_is_ignored() {
        let mut config = service();
        config.dependencies = vec!["my-app".into(), "Tcpip".into()];
        let commands = apply_commands(&config);
        let deps = commands
            .iter()
            .find(|command| command.args.get(2) == Some(&"DependOnService".to_string()))
            .unwrap();
        assert_eq!(deps.args, vec!["set", "my-app", "DependOnService", "Tcpip"]);
    }

    #[test]
    fn default_working_directory_uses_executable_parent() {
        let mut config = service();
        config.working_directory = String::new();
        config.executable = r"C:\apps\demo\app.exe".into();
        let commands = apply_commands(&config);
        assert!(commands
            .iter()
            .any(|command| command.args == vec!["set", "my-app", "AppDirectory", r"C:\apps\demo"]));
    }

    #[test]
    fn nssm_get_returns_trimmed_value() {
        let temp = tempfile::tempdir().unwrap();
        let path = fake_nssm(&temp);
        let runner = MockRunner::with_outputs(vec![crate::process::output(
            0,
            "C:\\apps\\demo\\app.exe\r\n",
            "",
        )]);
        let nssm = Nssm::new(&path, &runner);
        assert_eq!(
            nssm.get_value("demo", "Application").unwrap(),
            Some(r"C:\apps\demo\app.exe".to_string())
        );
    }

    #[test]
    fn nssm_get_failure_returns_none() {
        let temp = tempfile::tempdir().unwrap();
        let path = fake_nssm(&temp);
        let runner = MockRunner::with_outputs(vec![crate::process::output(1, "", "not found")]);
        let nssm = Nssm::new(&path, &runner);
        assert_eq!(nssm.get_value("demo", "Application").unwrap(), None);
    }

    #[test]
    fn nssm_set_empty_value_resets() {
        let temp = tempfile::tempdir().unwrap();
        let path = fake_nssm(&temp);
        let runner = MockRunner::default();
        let nssm = Nssm::new(&path, &runner);
        nssm.set_value("demo", "AppParameters", "").unwrap();
        assert_eq!(runner.calls()[0].1, vec!["reset", "demo", "AppParameters"]);
    }

    #[test]
    fn nssm_install_cleans_up_on_failure() {
        let temp = tempfile::tempdir().unwrap();
        let path = fake_nssm(&temp);
        let runner = MockRunner::with_outputs(vec![
            crate::process::output(1, "", "install failed"),
            crate::process::output(0, "", ""),
        ]);
        let nssm = Nssm::new(&path, &runner);
        let config = service();
        let error = nssm.install(&config).unwrap_err();
        assert!(matches!(error, NssmError::CommandFailed { .. }));
        let calls = runner.calls();
        assert_eq!(calls[1].1, vec!["remove", "my-app", "confirm"]);
    }

    #[test]
    fn nssm_install_cleans_up_when_apply_fails() {
        let temp = tempfile::tempdir().unwrap();
        let path = fake_nssm(&temp);
        // Semua perintah nssm sukses; delayed-start SCM gagal karena service tidak ada.
        let runner = MockRunner::default();
        let nssm = Nssm::new(&path, &runner);
        let error = nssm.install(&service()).unwrap_err();
        assert!(matches!(error, NssmError::Scm(_)));
        assert!(runner
            .calls()
            .iter()
            .any(|(_, args)| args == &vec!["remove", "my-app", "confirm"]));
    }
}
