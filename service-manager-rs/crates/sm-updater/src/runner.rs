use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

use thiserror::Error;
use wait_timeout::ChildExt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn success(&self) -> bool {
        self.exit_code == 0
    }

    pub fn combined(&self) -> String {
        let mut text = self.stdout.trim().to_string();
        let stderr = self.stderr.trim();
        if !stderr.is_empty() {
            if !text.is_empty() {
                text.push_str("\r\n");
            }
            text.push_str(stderr);
        }
        text
    }
}

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("Gagal menjalankan '{program}': {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("'{program}' timeout setelah {timeout:?}.")]
    Timeout { program: String, timeout: Duration },
    #[error("Gagal membaca output proses: {0}")]
    Io(String),
}

pub trait Runner: Send + Sync {
    fn run(
        &self,
        program: &Path,
        args: &[String],
        cwd: &Path,
        timeout: Duration,
    ) -> Result<Output, RunnerError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(
        &self,
        program: &Path,
        args: &[String],
        cwd: &Path,
        timeout: Duration,
    ) -> Result<Output, RunnerError> {
        let mut child = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .env("GCM_CREDENTIAL_STORE", "dpapi")
            .env("GCM_INTERACTIVE", "never")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| RunnerError::Spawn {
                program: program.to_string_lossy().to_string(),
                source,
            })?;

        let status = match child.wait_timeout(timeout) {
            Ok(Some(status)) => status,
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(RunnerError::Timeout {
                    program: program.to_string_lossy().to_string(),
                    timeout,
                });
            }
            Err(error) => {
                return Err(RunnerError::Spawn {
                    program: program.to_string_lossy().to_string(),
                    source: error,
                });
            }
        };

        let mut stdout = String::new();
        if let Some(mut pipe) = child.stdout.take() {
            pipe.read_to_string(&mut stdout)
                .map_err(|error| RunnerError::Io(error.to_string()))?;
        }
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            pipe.read_to_string(&mut stderr)
                .map_err(|error| RunnerError::Io(error.to_string()))?;
        }

        Ok(Output {
            exit_code: status.code().unwrap_or(-1),
            stdout,
            stderr,
        })
    }
}

pub fn cmd_exe_path() -> std::path::PathBuf {
    std::env::var_os("ComSpec")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(r"C:\Windows\System32\cmd.exe"))
}

/// Bungkus `.bat`/`.cmd` agar bisa dijalankan melalui `cmd.exe /c`.
pub fn wrap_command(program: &Path, args: &[String]) -> (std::path::PathBuf, Vec<String>) {
    let is_script = program
        .extension()
        .map(|extension| {
            extension.eq_ignore_ascii_case("bat") || extension.eq_ignore_ascii_case("cmd")
        })
        .unwrap_or(false);

    if is_script {
        let mut wrapped = vec!["/c".to_string(), program.to_string_lossy().to_string()];
        wrapped.extend(args.iter().cloned());
        (cmd_exe_path(), wrapped)
    } else {
        (program.to_path_buf(), args.to_vec())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_batch_and_cmd_files() {
        let args = vec!["install".to_string()];
        let (program, wrapped) = wrap_command(Path::new(r"C:\composer\composer.bat"), &args);
        assert!(program
            .to_string_lossy()
            .to_lowercase()
            .ends_with("cmd.exe"));
        assert_eq!(wrapped[0], "/c");
        assert_eq!(wrapped[1], r"C:\composer\composer.bat");
        assert_eq!(wrapped[2], "install");
    }

    #[test]
    fn leaves_executables_unwrapped() {
        let args = vec!["status".to_string()];
        let (program, wrapped) = wrap_command(Path::new(r"C:\git\git.exe"), &args);
        assert_eq!(program, Path::new(r"C:\git\git.exe"));
        assert_eq!(wrapped, args);
    }

    #[test]
    fn system_runner_captures_exit_code() {
        let temp = tempfile::tempdir().unwrap();
        let output = SystemRunner
            .run(
                Path::new(r"C:\Windows\System32\cmd.exe"),
                &["/c".into(), "exit 3".into()],
                temp.path(),
                Duration::from_secs(10),
            )
            .unwrap();
        assert_eq!(output.exit_code, 3);
        assert!(!output.success());
    }

    #[test]
    fn system_runner_times_out() {
        let temp = tempfile::tempdir().unwrap();
        let error = SystemRunner
            .run(
                Path::new(r"C:\Windows\System32\cmd.exe"),
                &["/c".into(), "ping -n 5 127.0.0.1 > nul".into()],
                temp.path(),
                Duration::from_millis(80),
            )
            .unwrap_err();
        assert!(matches!(error, RunnerError::Timeout { .. }));
    }

    #[test]
    fn output_combines_streams() {
        let output = Output {
            exit_code: 0,
            stdout: "out\n".into(),
            stderr: "err\n".into(),
        };
        assert_eq!(output.combined(), "out\r\nerr");
    }
}
