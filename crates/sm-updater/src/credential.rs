use std::fmt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use sm_win::CommandNoWindow;

#[derive(Clone, PartialEq, Eq)]
pub struct CredentialRequest {
    pub host: String,
    pub username: String,
    pub token: String,
    pub git_path: PathBuf,
    pub project_path: PathBuf,
}

impl fmt::Debug for CredentialRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CredentialRequest")
            .field("host", &self.host)
            .field("username", &self.username)
            .field("token", &"[REDACTED]")
            .field("git_path", &self.git_path)
            .field("project_path", &self.project_path)
            .finish()
    }
}

pub trait CredentialSink {
    fn approve(&self, git_path: &Path, input: &str) -> Result<(), String>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct GitCredentialSink;

impl CredentialSink for GitCredentialSink {
    fn approve(&self, git_path: &Path, input: &str) -> Result<(), String> {
        let mut child = Command::new(git_path)
            .args(["credential", "approve"])
            .env("GCM_CREDENTIAL_STORE", "dpapi")
            .env("GCM_INTERACTIVE", "never")
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .no_window()
            .spawn()
            .map_err(|error| format!("Gagal menjalankan git credential approve: {error}"))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(input.as_bytes())
                .map_err(|error| format!("Gagal mengirim kredensial: {error}"))?;
        }

        let output = child
            .wait_with_output()
            .map_err(|error| format!("Gagal menunggu git credential approve: {error}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!(
                "git credential approve gagal: {}",
                sm_core::redact_secrets(stderr.trim())
            ));
        }
        Ok(())
    }
}

/// Format input untuk `git credential approve`. Token tidak pernah di-log.
pub fn credential_input(host: &str, username: &str, token: &str) -> String {
    format!("protocol=https\nhost={host}\nusername={username}\npassword={token}\n\n")
}

/// Format input untuk `git credential reject`. Tidak ada token/password.
pub fn credential_reject_input(host: &str, username: &str) -> String {
    format!("protocol=https\nhost={host}\nusername={username}\n\n")
}

fn validate_field(value: &str, label: &str) -> Result<String, String> {
    if value.trim().is_empty() {
        return Err(format!("{label} wajib diisi."));
    }
    if value.chars().any(char::is_control) {
        return Err(format!(
            "{label} tidak boleh berisi karakter kontrol atau baris baru."
        ));
    }
    if value.chars().any(char::is_whitespace) {
        return Err(format!("{label} tidak boleh berisi spasi."));
    }
    Ok(value.to_string())
}

/// Host harus bersih agar tidak bisa menyuntikkan baris tambahan ke protokol
/// `git credential`. Skema/path/userinfo tidak diperbolehkan.
pub fn validate_host(host: &str) -> Result<(), String> {
    let value = validate_field(host, "Host Git")?;
    if value.contains(['/', '\\', '@', '?', '#']) {
        return Err(
            "Host Git tidak boleh memuat skema, path, atau userinfo (contoh: gitlab.com)."
                .to_string(),
        );
    }
    Ok(())
}

/// Username tidak boleh berisi baris baru/karakter kontrol.
pub fn validate_username(username: &str) -> Result<(), String> {
    validate_field(username, "Username Git").map(|_| ())
}

/// Token tidak boleh berisi baris baru/karakter kontrol dan tidak pernah
/// dikembalikan di pesan error.
pub fn validate_token(token: &str) -> Result<(), String> {
    validate_field(token, "Token").map(|_| ())
}

pub fn validate_request(request: &CredentialRequest) -> Result<(), String> {
    validate_host(&request.host)?;
    validate_username(&request.username)?;
    validate_token(&request.token)?;
    if !request.git_path.is_file() {
        return Err(format!(
            "Git tidak ditemukan: {}",
            request.git_path.display()
        ));
    }
    if !request.project_path.is_dir() {
        return Err(format!(
            "Folder project tidak ditemukan: {}",
            request.project_path.display()
        ));
    }
    Ok(())
}

pub fn store_credential<S: CredentialSink>(
    request: &CredentialRequest,
    sink: &S,
) -> Result<(), String> {
    validate_request(request)?;
    let token = request.token.trim();
    let input = credential_input(request.host.trim(), request.username.trim(), token);
    sink.approve(&request.git_path, &input).map_err(|error| {
        let redacted = sm_core::redact_secrets(&error);
        if token.is_empty() {
            redacted
        } else {
            redacted.replace(token, "[REDACTED]")
        }
    })
}

/// Hapus kredensial tersimpan untuk host+username melalui
/// `git credential reject`. Tidak ada token atau password yang dikirim.
pub fn reject_credential(git_path: &Path, host: &str, username: &str) -> Result<(), String> {
    validate_host(host)?;
    validate_username(username)?;
    if !git_path.is_file() {
        return Err(format!("Git tidak ditemukan: {}", git_path.display()));
    }

    let input = credential_reject_input(host.trim(), username.trim());
    let mut child = Command::new(git_path)
        .args(["credential", "reject"])
        .env("GCM_CREDENTIAL_STORE", "dpapi")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .no_window()
        .spawn()
        .map_err(|error| format!("Gagal menjalankan git credential reject: {error}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .map_err(|error| format!("Gagal mengirim permintaan hapus kredensial: {error}"))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|error| format!("Gagal menunggu git credential reject: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "git credential reject gagal: {}",
            sm_core::redact_secrets(stderr.trim())
        ));
    }
    Ok(())
}

/// Verifikasi kredensial dengan `git ls-remote --heads origin`.
pub fn verify_remote(request: &CredentialRequest) -> Result<(), String> {
    let output = Command::new(&request.git_path)
        .args(["-C"])
        .arg(&request.project_path)
        .args(["ls-remote", "--heads", "origin"])
        .env("GCM_CREDENTIAL_STORE", "dpapi")
        .env("GCM_INTERACTIVE", "never")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .no_window()
        .output()
        .map_err(|error| format!("Gagal menjalankan git ls-remote: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "Verifikasi remote gagal: {}",
            sm_core::redact_secrets(stderr.trim())
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockSink {
        calls: Mutex<Vec<(String, String)>>,
    }

    impl CredentialSink for MockSink {
        fn approve(&self, git_path: &Path, input: &str) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap()
                .push((git_path.to_string_lossy().to_string(), input.to_string()));
            Ok(())
        }
    }

    struct Fixture {
        _temp: tempfile::TempDir,
        git_path: PathBuf,
        project_path: PathBuf,
    }

    fn fixture() -> Fixture {
        let temp = tempfile::tempdir().unwrap();
        let git_path = temp.path().join("git.exe");
        std::fs::write(&git_path, b"dummy").unwrap();
        let project_path = temp.path().join("project");
        std::fs::create_dir_all(&project_path).unwrap();
        Fixture {
            _temp: temp,
            git_path,
            project_path,
        }
    }

    fn request(fixture: &Fixture) -> CredentialRequest {
        CredentialRequest {
            host: "gitlab.com".into(),
            username: "oauth2".into(),
            token: "glpat-secret-token".into(),
            git_path: fixture.git_path.clone(),
            project_path: fixture.project_path.clone(),
        }
    }

    #[test]
    fn debug_output_redacts_token() {
        let fixture = fixture();
        let text = format!("{:?}", request(&fixture));
        assert!(text.contains("gitlab.com"));
        assert!(text.contains("oauth2"));
        assert!(text.contains("[REDACTED]"));
        assert!(!text.contains("glpat-secret-token"));
    }

    #[test]
    fn builds_git_credential_input() {
        let input = credential_input("gitlab.com", "oauth2", "secret");
        assert_eq!(
            input,
            "protocol=https\nhost=gitlab.com\nusername=oauth2\npassword=secret\n\n"
        );
    }

    #[test]
    fn validates_required_fields() {
        let fixture = fixture();
        let mut value = request(&fixture);
        value.host.clear();
        assert!(validate_request(&value).unwrap_err().contains("Host"));

        let mut value = request(&fixture);
        value.username.clear();
        assert!(validate_request(&value).unwrap_err().contains("Username"));

        let mut value = request(&fixture);
        value.token.clear();
        assert!(validate_request(&value).unwrap_err().contains("Token"));
    }

    #[test]
    fn validates_paths() {
        let fixture = fixture();
        let mut value = request(&fixture);
        value.git_path = fixture.project_path.join("missing.exe");
        assert!(validate_request(&value).unwrap_err().contains("Git"));

        let mut value = request(&fixture);
        value.project_path = fixture.project_path.join("missing");
        assert!(validate_request(&value)
            .unwrap_err()
            .contains("Folder project"));
    }

    #[test]
    fn store_credential_passes_input_to_sink() {
        let fixture = fixture();
        let sink = MockSink::default();
        store_credential(&request(&fixture), &sink).unwrap();

        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].1.contains("host=gitlab.com"));
        assert!(calls[0].1.contains("username=oauth2"));
        assert!(calls[0].1.contains("password=glpat-secret-token"));
    }

    #[test]
    fn redaction_hides_token_in_error_messages() {
        let text =
            sm_core::redact_secrets("fatal: https://oauth2:glpat-abcdefghij@gitlab.com/repo.git");
        assert!(!text.contains("glpat-abcdefghij"));
    }

    #[test]
    fn builds_git_credential_reject_input_without_secret() {
        let input = credential_reject_input("gitlab.com", "oauth2");
        assert_eq!(
            input,
            "protocol=https\nhost=gitlab.com\nusername=oauth2\n\n"
        );
        assert!(!input.contains("password="));
    }

    #[test]
    fn validation_rejects_empty_control_and_whitespace_fields() {
        assert!(validate_host("").is_err());
        assert!(validate_host("gitlab.com\nhost=evil.example").is_err());
        assert!(validate_host("gitlab.com\n").is_err());
        assert!(validate_host(" gitlab.com ").is_err());
        assert!(validate_host("https://gitlab.com/repo").is_err());
        assert!(validate_host("git@gitlab.com").is_err());
        assert!(validate_host("git lab.com").is_err());
        assert!(validate_host("gitlab.com").is_ok());

        assert!(validate_username("").is_err());
        assert!(validate_username("oauth2\r\npassword=secret").is_err());
        assert!(validate_username("oauth2 ").is_err());
        assert!(validate_username("oauth two").is_err());
        assert!(validate_username("oauth2").is_ok());

        assert!(validate_token("").is_err());
        assert!(validate_token("glpat-token\n").is_err());
        assert!(validate_token("glpat-token\npassword=leak").is_err());
        assert!(validate_token("glpat-token").is_ok());
    }

    #[test]
    fn store_credential_rejects_newline_injection_before_sink() {
        let fixture = fixture();
        let sink = MockSink::default();
        let mut value = request(&fixture);
        value.host = "gitlab.com\nhost=evil.example".into();
        let error = store_credential(&value, &sink).unwrap_err();
        assert!(error.contains("Host"));
        assert!(sink.calls.lock().unwrap().is_empty());

        let mut value = request(&fixture);
        value.token = "glpat-secret-token\npassword=other".into();
        let error = store_credential(&value, &sink).unwrap_err();
        assert!(error.contains("Token"));
        assert!(!error.contains("glpat-secret-token"));
        assert!(!error.contains("other"));
        assert!(sink.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn sink_errors_are_redacted_and_never_expose_token() {
        struct FailingSink;

        impl CredentialSink for FailingSink {
            fn approve(&self, _git_path: &Path, input: &str) -> Result<(), String> {
                let token = input
                    .lines()
                    .find_map(|line| line.strip_prefix("password="))
                    .unwrap_or_default();
                Err(format!("helper failed: {token}"))
            }
        }

        let fixture = fixture();
        let error = store_credential(&request(&fixture), &FailingSink).unwrap_err();
        assert!(error.contains("helper failed"));
        assert!(!error.contains("glpat-secret-token"));
        assert!(error.contains("[REDACTED]"));
    }

    #[test]
    fn reject_credential_validates_fields_and_git_before_spawning() {
        let fixture = fixture();
        let error = reject_credential(&fixture.git_path, "", "oauth2").unwrap_err();
        assert!(error.contains("Host"));

        let error = reject_credential(&fixture.git_path, "gitlab.com", "oauth\n2").unwrap_err();
        assert!(error.contains("Username"));

        let missing = fixture.project_path.join("missing-git.exe");
        let error = reject_credential(&missing, "gitlab.com", "oauth2").unwrap_err();
        assert!(error.contains("Git"));
    }
}
