use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use sm_core::{ServiceConfig, ServiceType, StartupMode};
use thiserror::Error;

use crate::error::ScmError;
use crate::process::{sc_exe_path, CommandOutput, CommandRunner, ProcessError};
use crate::scm;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NativeCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
}

impl NativeCommand {
    pub fn new(program: impl Into<PathBuf>, args: &[&str]) -> Self {
        Self {
            program: program.into(),
            args: args.iter().map(|value| value.to_string()).collect(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallPlan {
    pub install: NativeCommand,
    pub remove: NativeCommand,
}

#[derive(Debug, Error)]
pub enum NativeError {
    #[error("Jenis service '{0}' tidak memiliki installer native.")]
    UnsupportedType(String),
    #[error("Field wajib untuk service native tidak diisi: {0}")]
    MissingField(&'static str),
    #[error("Perintah '{program}' gagal (exit {exit_code}): {output}")]
    CommandFailed {
        program: String,
        exit_code: i32,
        output: String,
    },
    #[error("Service '{0}' tidak muncul setelah proses install.")]
    ServiceNotCreated(String),
    #[error(transparent)]
    Process(#[from] ProcessError),
    #[error(transparent)]
    Scm(#[from] ScmError),
}

pub fn install_plan(config: &ServiceConfig) -> Result<InstallPlan, NativeError> {
    match config.service_type.normalized() {
        ServiceType::Apache => {
            if config.apache_root.is_empty() {
                return Err(NativeError::MissingField("apacheRoot"));
            }
            let root = config.apache_root.as_str();
            let apache_config = if config.apache_config.is_empty() {
                "conf/httpd.conf"
            } else {
                config.apache_config.as_str()
            };
            Ok(InstallPlan {
                install: NativeCommand::new(
                    &config.executable,
                    &[
                        "-d",
                        root,
                        "-f",
                        apache_config,
                        "-k",
                        "install",
                        "-n",
                        &config.name,
                    ],
                ),
                remove: NativeCommand::new(
                    &config.executable,
                    &[
                        "-d",
                        root,
                        "-f",
                        apache_config,
                        "-k",
                        "uninstall",
                        "-n",
                        &config.name,
                    ],
                ),
            })
        }
        ServiceType::PostgreSql => {
            if config.pg_data.is_empty() {
                return Err(NativeError::MissingField("pgData"));
            }
            if config.pg_account.is_empty() {
                return Err(NativeError::MissingField("pgAccount"));
            }
            Ok(InstallPlan {
                install: NativeCommand::new(
                    &config.executable,
                    &[
                        "register",
                        "-N",
                        &config.name,
                        "-D",
                        &config.pg_data,
                        "-U",
                        &config.pg_account,
                        "-S",
                        "auto",
                        "-w",
                    ],
                ),
                remove: NativeCommand::new(&config.executable, &["unregister", "-N", &config.name]),
            })
        }
        ServiceType::Nssm | ServiceType::Unknown => Err(NativeError::UnsupportedType(
            config.service_type.label().to_string(),
        )),
    }
}

pub fn apply_commands(config: &ServiceConfig, sc_path: &Path) -> Vec<NativeCommand> {
    let mut commands = Vec::new();
    let name = config.name.as_str();
    let display_name = if config.display_name.is_empty() {
        name
    } else {
        config.display_name.as_str()
    };

    commands.push(NativeCommand::new(
        sc_path,
        &["config", name, "DisplayName=", display_name],
    ));

    let start_mode = match config.startup.normalized() {
        StartupMode::Manual => "demand",
        StartupMode::Auto | StartupMode::Delayed | StartupMode::Unknown => "auto",
    };
    commands.push(NativeCommand::new(
        sc_path,
        &["config", name, "start=", start_mode],
    ));

    let dependencies: Vec<String> = config
        .normalized_dependencies()
        .into_iter()
        .filter(|dependency| !dependency.eq_ignore_ascii_case(name))
        .collect();
    let dependency_value = if dependencies.is_empty() {
        "/".to_string()
    } else {
        dependencies.join("/")
    };
    commands.push(NativeCommand::new(
        sc_path,
        &["config", name, "depend=", &dependency_value],
    ));

    let failure_args = failure_action_args(config);
    commands.push(NativeCommand {
        program: sc_path.to_path_buf(),
        args: failure_args,
    });

    commands
}

pub fn failure_action_args(config: &ServiceConfig) -> Vec<String> {
    if config.restart_on_crash {
        let delay_ms = config.restart_delay_sec.max(1) * 1000;
        vec![
            "failure".to_string(),
            config.name.clone(),
            "reset=".to_string(),
            "86400".to_string(),
            "actions=".to_string(),
            format!(
                "restart/{delay_ms}/restart/{}/restart/{}",
                delay_ms * 2,
                delay_ms * 4
            ),
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

pub fn apply(
    config: &ServiceConfig,
    runner: &dyn CommandRunner,
    sc_path: &Path,
) -> Result<(), NativeError> {
    let display_name = if config.display_name.is_empty() {
        config.name.clone()
    } else {
        config.display_name.clone()
    };
    let start_mode = match config.startup.normalized() {
        StartupMode::Manual => "demand",
        StartupMode::Auto | StartupMode::Delayed | StartupMode::Unknown => "auto",
    };
    let dependencies: Vec<String> = config
        .normalized_dependencies()
        .into_iter()
        .filter(|dependency| !dependency.eq_ignore_ascii_case(&config.name))
        .collect();
    let dependency_value = if dependencies.is_empty() {
        "/".to_string()
    } else {
        dependencies.join("/")
    };

    let strict_commands = [
        vec![
            "config".to_string(),
            config.name.clone(),
            "DisplayName=".to_string(),
            display_name,
        ],
        vec![
            "config".to_string(),
            config.name.clone(),
            "start=".to_string(),
            start_mode.to_string(),
        ],
        vec![
            "config".to_string(),
            config.name.clone(),
            "depend=".to_string(),
            dependency_value,
        ],
    ];

    for args in strict_commands {
        run_checked(runner, sc_path, &args)?;
    }

    // Recovery action bersifat best-effort, sama seperti versi PowerShell.
    let failure_args = failure_action_args(config);
    if !failure_args.is_empty() {
        let _ = runner.run(sc_path, &failure_args);
    }

    scm::set_delayed_start(&config.name, config.startup == StartupMode::Delayed)?;
    Ok(())
}

pub fn install(config: &ServiceConfig, runner: &dyn CommandRunner) -> Result<(), NativeError> {
    install_with_timeout(config, runner, Duration::from_secs(20))
}

pub fn install_with_timeout(
    config: &ServiceConfig,
    runner: &dyn CommandRunner,
    timeout: Duration,
) -> Result<(), NativeError> {
    let plan = install_plan(config)?;
    let output = runner.run(&plan.install.program, &plan.install.args)?;
    if !output.success() {
        return Err(NativeError::CommandFailed {
            program: plan.install.program.to_string_lossy().to_string(),
            exit_code: output.exit_code,
            output: output.combined(),
        });
    }

    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if scm::service_exists(&config.name)? {
            return apply(config, runner, &sc_exe_path());
        }
        thread::sleep(Duration::from_millis(400));
    }

    Err(NativeError::ServiceNotCreated(config.name.clone()))
}

pub fn remove(config: &ServiceConfig, runner: &dyn CommandRunner) -> Result<(), NativeError> {
    let plan = install_plan(config)?;
    let output = runner.run(&plan.remove.program, &plan.remove.args)?;
    if !output.success() {
        return Err(NativeError::CommandFailed {
            program: plan.remove.program.to_string_lossy().to_string(),
            exit_code: output.exit_code,
            output: output.combined(),
        });
    }
    Ok(())
}

fn run_checked(
    runner: &dyn CommandRunner,
    program: &Path,
    args: &[String],
) -> Result<CommandOutput, NativeError> {
    let output = runner.run(program, args)?;
    if !output.success() {
        return Err(NativeError::CommandFailed {
            program: program.to_string_lossy().to_string(),
            exit_code: output.exit_code,
            output: output.combined(),
        });
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apache() -> ServiceConfig {
        ServiceConfig {
            name: "Apache2.4".into(),
            display_name: "Apache 2.4".into(),
            service_type: ServiceType::Apache,
            executable: r"C:\Apache24\bin\httpd.exe".into(),
            apache_root: r"C:\Apache24".into(),
            apache_config: "conf/httpd.conf".into(),
            startup: StartupMode::Auto,
            restart_on_crash: true,
            restart_delay_sec: 5,
            dependencies: vec!["Tcpip".into()],
            ..ServiceConfig::default()
        }
    }

    fn postgres() -> ServiceConfig {
        ServiceConfig {
            name: "postgresql-16".into(),
            display_name: "PostgreSQL 16".into(),
            service_type: ServiceType::PostgreSql,
            executable: r"C:\PostgreSQL\16\bin\pg_ctl.exe".into(),
            pg_data: r"C:\PostgreSQL\16\data".into(),
            pg_account: r"NT AUTHORITY\NetworkService".into(),
            ..ServiceConfig::default()
        }
    }

    #[test]
    fn apache_install_plan_matches_native_syntax() {
        let plan = install_plan(&apache()).unwrap();
        assert_eq!(
            plan.install.args,
            vec![
                "-d",
                r"C:\Apache24",
                "-f",
                "conf/httpd.conf",
                "-k",
                "install",
                "-n",
                "Apache2.4"
            ]
        );
        assert_eq!(
            plan.remove.args,
            vec![
                "-d",
                r"C:\Apache24",
                "-f",
                "conf/httpd.conf",
                "-k",
                "uninstall",
                "-n",
                "Apache2.4"
            ]
        );
    }

    #[test]
    fn postgres_install_plan_matches_native_syntax() {
        let plan = install_plan(&postgres()).unwrap();
        assert_eq!(
            plan.install.args,
            vec![
                "register",
                "-N",
                "postgresql-16",
                "-D",
                r"C:\PostgreSQL\16\data",
                "-U",
                r"NT AUTHORITY\NetworkService",
                "-S",
                "auto",
                "-w"
            ]
        );
        assert_eq!(plan.remove.args, vec!["unregister", "-N", "postgresql-16"]);
    }

    #[test]
    fn nssm_type_has_no_native_install_plan() {
        let config = ServiceConfig {
            service_type: ServiceType::Nssm,
            ..ServiceConfig::default()
        };
        assert!(matches!(
            install_plan(&config).unwrap_err(),
            NativeError::UnsupportedType(_)
        ));
    }

    #[test]
    fn apply_commands_use_sc_config_and_failure() {
        let commands = apply_commands(&apache(), Path::new(r"C:\Windows\System32\sc.exe"));
        assert!(commands.iter().any(
            |command| command.args == vec!["config", "Apache2.4", "DisplayName=", "Apache 2.4"]
        ));
        assert!(commands
            .iter()
            .any(|command| command.args == vec!["config", "Apache2.4", "start=", "auto"]));
        assert!(commands
            .iter()
            .any(|command| command.args == vec!["config", "Apache2.4", "depend=", "Tcpip"]));
        assert!(commands.iter().any(|command| command.args
            == vec![
                "failure",
                "Apache2.4",
                "reset=",
                "86400",
                "actions=",
                "restart/5000/restart/10000/restart/20000"
            ]));
    }

    #[test]
    fn manual_start_and_empty_dependencies() {
        let mut config = apache();
        config.startup = StartupMode::Manual;
        config.dependencies.clear();
        let commands = apply_commands(&config, Path::new("sc.exe"));
        assert!(commands
            .iter()
            .any(|command| command.args == vec!["config", "Apache2.4", "start=", "demand"]));
        assert!(commands
            .iter()
            .any(|command| command.args == vec!["config", "Apache2.4", "depend=", "/"]));
    }

    #[test]
    fn restart_delay_is_minimum_one_second() {
        let mut config = apache();
        config.restart_delay_sec = 0;
        let args = failure_action_args(&config);
        assert!(args.contains(&"restart/1000/restart/2000/restart/4000".to_string()));
    }

    #[test]
    fn disabled_restart_clears_failure_actions() {
        let mut config = apache();
        config.restart_on_crash = false;
        let args = failure_action_args(&config);
        assert_eq!(args.last().unwrap(), "/");
        assert!(args.contains(&"0".to_string()));
    }

    #[test]
    fn install_failure_is_reported() {
        use crate::process::MockRunner;
        let runner =
            MockRunner::with_outputs(vec![crate::process::output(1, "", "install failed")]);
        let error = install_with_timeout(&apache(), &runner, Duration::from_millis(1)).unwrap_err();
        assert!(matches!(error, NativeError::CommandFailed { .. }));
        assert!(runner.calls()[0].0.ends_with("httpd.exe"));
    }
}
