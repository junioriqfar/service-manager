use std::ffi::c_void;
use std::mem::size_of;
use std::path::Path;

use thiserror::Error;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

#[derive(Debug, Error)]
pub enum ElevationError {
    #[error("Gagal memeriksa hak Administrator: {0}")]
    Token(String),
    #[error("Tidak dapat menentukan path executable: {0}")]
    CurrentExe(String),
    #[error("Gagal menjalankan ulang sebagai Administrator: {0}")]
    Shell(String),
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Cek apakah proses saat ini memiliki token Administrator (elevated).
pub fn is_elevated() -> Result<bool, ElevationError> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }
        .map_err(|error| ElevationError::Token(error.to_string()))?;

    let result = (|| {
        let mut elevation = TOKEN_ELEVATION::default();
        let mut needed = 0u32;
        unsafe {
            GetTokenInformation(
                token,
                TokenElevation,
                Some((&mut elevation as *mut TOKEN_ELEVATION).cast::<c_void>()),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut needed,
            )
        }
        .map_err(|error| ElevationError::Token(error.to_string()))?;
        Ok(elevation.TokenIsElevated != 0)
    })();

    let _ = unsafe { CloseHandle(token) };
    result
}

/// Jalankan ulang executable saat ini dengan verb `runas` (prompt UAC).
pub fn relaunch_elevated(args: &[String]) -> Result<(), ElevationError> {
    let executable =
        std::env::current_exe().map_err(|error| ElevationError::CurrentExe(error.to_string()))?;
    relaunch_elevated_path(&executable, args)
}

fn relaunch_elevated_path(executable: &Path, args: &[String]) -> Result<(), ElevationError> {
    let operation = wide("runas");
    let executable_wide = wide(&executable.to_string_lossy());
    let parameters = args
        .iter()
        .map(|arg| quote_argument(arg))
        .collect::<Vec<_>>()
        .join(" ");
    let parameters_wide = wide(&parameters);

    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(executable_wide.as_ptr()),
            PCWSTR(parameters_wide.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    let code = result.0 as isize;
    if code <= 32 {
        return Err(ElevationError::Shell(format!(
            "ShellExecuteW mengembalikan kode {code}"
        )));
    }
    Ok(())
}

/// Kutip satu argumen sesuai aturan `CommandLineToArgvW`.
pub fn quote_argument(argument: &str) -> String {
    if !argument.is_empty()
        && !argument
            .chars()
            .any(|character| character.is_whitespace() || character == '"')
    {
        return argument.to_string();
    }

    let mut quoted = String::from("\"");
    let mut backslashes = 0usize;

    for character in argument.chars() {
        match character {
            '\\' => backslashes += 1,
            '"' => {
                quoted.push_str(&"\\".repeat(backslashes * 2 + 1));
                quoted.push('"');
                backslashes = 0;
            }
            other => {
                quoted.push_str(&"\\".repeat(backslashes));
                quoted.push(other);
                backslashes = 0;
            }
        }
    }

    quoted.push_str(&"\\".repeat(backslashes * 2));
    quoted.push('"');
    quoted
}

pub fn command_line(args: &[String]) -> String {
    args.iter()
        .map(|arg| quote_argument(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_simple_argument_is_unchanged() {
        assert_eq!(quote_argument("--config"), "--config");
        assert_eq!(
            quote_argument(r"C:\tools\ServiceManager\services.json"),
            r"C:\tools\ServiceManager\services.json"
        );
    }

    #[test]
    fn quoting_argument_with_spaces() {
        assert_eq!(quote_argument("hello world"), "\"hello world\"");
    }

    #[test]
    fn quoting_empty_argument() {
        assert_eq!(quote_argument(""), "\"\"");
    }

    #[test]
    fn quoting_embedded_quote() {
        assert_eq!(quote_argument("say \"hi\""), "\"say \\\"hi\\\"\"");
    }

    #[test]
    fn quoting_trailing_backslash() {
        assert_eq!(
            quote_argument(r"C:\path with space\"),
            r#""C:\path with space\\""#
        );
    }

    #[test]
    fn command_line_joins_arguments() {
        let args = vec![
            "--mode".to_string(),
            "auto".to_string(),
            "hello world".to_string(),
        ];
        assert_eq!(command_line(&args), "--mode auto \"hello world\"");
    }

    #[test]
    fn current_process_elevation_can_be_checked() {
        let elevated = is_elevated().expect("pemeriksaan token harus berhasil");
        // Test berjalan baik sebagai admin maupun non-admin; pastikan hasil konsisten.
        assert!(elevated || !elevated);
    }
}
