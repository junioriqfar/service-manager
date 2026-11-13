use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialRequest {
    pub host: String,
    pub username: String,
    pub token: String,
    pub git_path: PathBuf,
    pub project_path: PathBuf,
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

pub fn validate_request(request: &CredentialRequest) -> Result<(), String> {
    if request.host.trim().is_empty() {
        return Err("Host Git wajib diisi.".to_string());
    }
    if request.username.trim().is_empty() {
        return Err("Username Git wajib diisi.".to_string());
    }
    if request.token.trim().is_empty() {
        return Err("Token wajib diisi.".to_string());
    }
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
    let input = credential_input(&request.host, &request.username, &request.token);
    sink.approve(&request.git_path, &input)
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
}
