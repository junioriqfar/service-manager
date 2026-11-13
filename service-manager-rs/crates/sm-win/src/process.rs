use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub program: String,
    pub args: Vec<String>,
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
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
pub enum ProcessError {
    #[error("Gagal menjalankan '{program}': {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("'{program}' gagal (exit {exit_code}): {output}")]
    Failed {
        program: String,
        exit_code: i32,
        output: String,
    },
}

pub trait CommandRunner: Send + Sync {
    fn run(&self, program: &Path, args: &[String]) -> Result<CommandOutput, ProcessError>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&self, program: &Path, args: &[String]) -> Result<CommandOutput, ProcessError> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|source| ProcessError::Spawn {
                program: program.to_string_lossy().to_string(),
                source,
            })?;

        Ok(CommandOutput {
            program: program.to_string_lossy().to_string(),
            args: args.to_vec(),
            exit_code: output.status.code().unwrap_or(-1),
            stdout: decode_output(&output.stdout),
            stderr: decode_output(&output.stderr),
        })
    }
}

/// NSSM/sc.exe dapat mengeluarkan UTF-16LE saat stdout dialihkan.
pub fn decode_output(bytes: &[u8]) -> String {
    let odd_nuls = bytes
        .iter()
        .enumerate()
        .filter(|(index, value)| **value == 0 && index % 2 == 1)
        .count();
    let even_nuls = bytes
        .iter()
        .enumerate()
        .filter(|(index, value)| **value == 0 && index % 2 == 0)
        .count();

    if bytes.len() >= 2 && odd_nuls > even_nuls && odd_nuls * 4 >= bytes.len() {
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u16::from_le_bytes(*chunk))
            .collect();
        String::from_utf16_lossy(&units)
            .trim_end_matches('\0')
            .to_string()
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}

pub fn sc_exe_path() -> PathBuf {
    std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join("sc.exe")
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct MockRunner {
    responses: std::sync::Mutex<std::collections::VecDeque<CommandOutput>>,
    calls: std::sync::Mutex<Vec<(String, Vec<String>)>>,
}

#[cfg(test)]
impl MockRunner {
    pub(crate) fn with_outputs(outputs: Vec<CommandOutput>) -> Self {
        Self {
            responses: std::sync::Mutex::new(outputs.into()),
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn calls(&self) -> Vec<(String, Vec<String>)> {
        self.calls.lock().unwrap().clone()
    }
}

#[cfg(test)]
impl CommandRunner for MockRunner {
    fn run(&self, program: &Path, args: &[String]) -> Result<CommandOutput, ProcessError> {
        self.calls
            .lock()
            .unwrap()
            .push((program.to_string_lossy().to_string(), args.to_vec()));

        if let Some(mut output) = self.responses.lock().unwrap().pop_front() {
            output.program = program.to_string_lossy().to_string();
            output.args = args.to_vec();
            Ok(output)
        } else {
            Ok(CommandOutput {
                program: program.to_string_lossy().to_string(),
                args: args.to_vec(),
                exit_code: 0,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }
}

#[cfg(test)]
pub(crate) fn output(exit_code: i32, stdout: &str, stderr: &str) -> CommandOutput {
    CommandOutput {
        program: String::new(),
        args: Vec::new(),
        exit_code,
        stdout: stdout.to_string(),
        stderr: stderr.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_utf8_output() {
        assert_eq!(decode_output(b"Service running"), "Service running");
    }

    #[test]
    fn decodes_utf16le_output() {
        let text = "C:\\Program Files\\App";
        let mut bytes = Vec::new();
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 0]);
        assert_eq!(decode_output(&bytes), text);
    }

    #[test]
    fn command_output_reports_success_and_combined_text() {
        let output = CommandOutput {
            program: "nssm".into(),
            args: vec!["get".into()],
            exit_code: 0,
            stdout: "value".into(),
            stderr: "warning".into(),
        };
        assert!(output.success());
        assert_eq!(output.combined(), "value\r\nwarning");
    }

    #[test]
    fn sc_path_is_under_system_root() {
        let path = sc_exe_path();
        assert_eq!(
            path.file_name().unwrap().to_string_lossy().to_lowercase(),
            "sc.exe"
        );
    }
}
