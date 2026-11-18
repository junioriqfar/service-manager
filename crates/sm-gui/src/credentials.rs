//! Git credential helper for the Settings UI.
//!
//! Tokens are sent only to `git credential approve` via stdin. They are never
//! persisted in `settings.json`, `projects.json`, or logs; Git Credential
//! Manager keeps them with DPAPI (`GCM_CREDENTIAL_STORE=dpapi`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sm_config::ConfigPaths;
use sm_updater::credential::{
    reject_credential, store_credential, validate_host, validate_token, validate_username,
    verify_remote, CredentialRequest, CredentialSink, GitCredentialSink,
};
use sm_updater::registry::load_config;
use sm_updater::settings::{effective_settings, settings_path_for};

/// Host Git yang dipakai app terdaftar. `apps` berisi app id (bukan token).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCredentialHostView {
    pub host: String,
    pub apps: Vec<String>,
}

fn strip_host_port(authority: &str) -> &str {
    let authority = authority.trim();
    if let Some(stripped) = authority.strip_prefix('[') {
        if let Some(end) = stripped.find(']') {
            return &stripped[..end];
        }
        return authority;
    }
    if authority.matches(':').count() > 1 {
        return authority;
    }
    match authority.rsplit_once(':') {
        Some((host, port))
            if !host.is_empty() && !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) =>
        {
            host
        }
        _ => authority,
    }
}

fn clean_host(host: &str) -> Option<String> {
    let host = host.trim();
    if host.is_empty()
        || host.chars().any(char::is_control)
        || host.chars().any(char::is_whitespace)
    {
        return None;
    }
    let host = host.to_ascii_lowercase();
    validate_host(&host).ok()?;
    Some(host)
}

/// Ambil host dari `repoUrl` tanpa skema, userinfo, port, atau path.
///
/// Mendukung `https://host/...`, `http://host/...`, `ssh://git@host:port/...`,
/// `git@host:path`, dan `git://host/...`. Skema lain dan URL tanpa host
/// menghasilkan `None`.
pub fn host_from_repo_url(repo_url: &str) -> Option<String> {
    let value = repo_url.trim();
    if value.is_empty() {
        return None;
    }

    if let Some((scheme, rest)) = value.split_once("://") {
        if !matches!(
            scheme.trim().to_ascii_lowercase().as_str(),
            "https" | "http" | "ssh" | "git"
        ) {
            return None;
        }
        let authority = rest
            .split(['/', '?', '#'])
            .next()
            .unwrap_or_default()
            .trim();
        if authority.is_empty() {
            return None;
        }
        let host_port = authority.rsplit('@').next().unwrap_or(authority);
        return clean_host(strip_host_port(host_port));
    }

    // SCP-like syntax: user@host:path (the SSH form used by Git).
    let (user, rest) = value.split_once('@')?;
    if user.trim().is_empty() {
        return None;
    }
    let host = rest.split([':', '/', '\\']).next().unwrap_or_default();
    clean_host(host)
}

/// Kelompokkan app terdaftar berdasarkan host `repoUrl`, case-insensitive.
/// Config yang hilang/invalid menghasilkan daftar kosong (read-only helper).
pub fn credential_hosts_at(config_path: &Path) -> Vec<GitCredentialHostView> {
    let config = load_config(config_path).unwrap_or_default();
    let mut hosts: BTreeMap<String, GitCredentialHostView> = BTreeMap::new();

    for app in &config.apps {
        let Some(host) = host_from_repo_url(&app.repo_url) else {
            continue;
        };
        let app_id = app.id.trim();
        if app_id.is_empty() {
            continue;
        }
        let entry =
            hosts
                .entry(host.to_ascii_lowercase())
                .or_insert_with(|| GitCredentialHostView {
                    host: host.clone(),
                    apps: Vec::new(),
                });
        if !entry
            .apps
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(app_id))
        {
            entry.apps.push(app_id.to_string());
        }
    }

    let mut views: Vec<GitCredentialHostView> = hosts.into_values().collect();
    for view in &mut views {
        view.apps.sort_by(|left, right| {
            left.to_ascii_lowercase()
                .cmp(&right.to_ascii_lowercase())
                .then_with(|| left.cmp(right))
        });
    }
    views
}

/// Path Git efektif dari `settings.json`, dengan fallback legacy
/// `projects.json.settings`.
pub(crate) fn effective_git_path_at(config_path: &Path) -> Result<PathBuf, String> {
    let config = load_config(config_path)?;
    let settings = effective_settings(
        &settings_path_for(config_path),
        &config.settings,
        &config.trigger,
    )
    .map_err(|error| error.to_string())?;
    let path = settings.tools.git.path.trim();
    if path.is_empty() {
        return Err(
            "Git path is not configured. Set it in Settings > Toolchain first.".to_string(),
        );
    }
    let path = PathBuf::from(path);
    if !path.is_file() {
        return Err(format!("Git executable not found: {}", path.display()));
    }
    Ok(path)
}

fn find_app<'a>(
    config: &'a sm_updater::registry::UpdaterConfig,
    app_id: &str,
) -> Result<&'a sm_updater::registry::AppConfig, String> {
    let app_id = app_id.trim();
    if app_id.is_empty() {
        return Err("App id is required.".to_string());
    }
    config
        .find(app_id)
        .ok_or_else(|| format!("Updater app '{app_id}' was not found."))
}

/// Simpan PAT untuk host+username. Tidak ada token di nilai balik/pesan error.
pub fn store_git_credential_at<S: CredentialSink>(
    config_path: &Path,
    host: &str,
    username: &str,
    token: &str,
    app_id: &str,
    git_path: &Path,
    sink: &S,
) -> Result<String, String> {
    validate_host(host)?;
    validate_username(username)?;
    validate_token(token)?;

    let config = load_config(config_path)?;
    let app = find_app(&config, app_id)?;
    let app_host = host_from_repo_url(&app.repo_url).ok_or_else(|| {
        format!(
            "Updater app '{}' has no supported HTTP/SSH Git remote URL.",
            app.id
        )
    })?;
    if !app_host.eq_ignore_ascii_case(host.trim()) {
        return Err(format!(
            "Host '{}' does not match the repository host '{}' for app '{}'.",
            host.trim(),
            app_host,
            app.id
        ));
    }
    let project_path = PathBuf::from(app.local_path.trim());
    if project_path.as_os_str().is_empty() {
        return Err(format!(
            "Updater app '{}' has no local path. Set it in the Updater tab first.",
            app.id
        ));
    }

    let request = CredentialRequest {
        host: host.trim().to_string(),
        username: username.trim().to_string(),
        token: token.trim().to_string(),
        git_path: git_path.to_path_buf(),
        project_path,
    };
    store_credential(&request, sink)?;

    Ok(format!(
        "Git credential stored for {}@{} (app {}). Git Credential Manager keeps the token with DPAPI; it is not written to settings.json.",
        username.trim(),
        host.trim(),
        app.id
    ))
}

/// Uji akses remote app memakai credential yang tersimpan. Output/error
/// di-redaksi sebelum dikembalikan ke UI.
pub fn test_git_credential_at(config_path: &Path, app_id: &str) -> Result<String, String> {
    let config = load_config(config_path)?;
    let app = find_app(&config, app_id)?;
    let host = host_from_repo_url(&app.repo_url).ok_or_else(|| {
        format!(
            "Updater app '{}' has no supported HTTP/SSH Git remote URL.",
            app.id
        )
    })?;
    let project_path = PathBuf::from(app.local_path.trim());
    if project_path.as_os_str().is_empty() {
        return Err(format!(
            "Updater app '{}' has no local path. Set it in the Updater tab first.",
            app.id
        ));
    }
    let git_path = effective_git_path_at(config_path)?;

    let request = CredentialRequest {
        host: host.clone(),
        username: String::new(),
        token: String::new(),
        git_path,
        project_path,
    };
    verify_remote(&request).map_err(|error| {
        sm_core::redact_secrets(&format!(
            "Remote credential test failed for {} ({}): {error}",
            app.id, host
        ))
    })?;

    Ok(format!(
        "Remote access verified for {} ({}). Git Credential Manager supplied the stored credential.",
        app.id, host
    ))
}

/// Hapus credential tersimpan untuk host+username (tanpa token/password).
pub fn clear_git_credential_at(git_path: &Path, host: &str, username: &str) -> Result<(), String> {
    reject_credential(git_path, host, username)
}

#[tauri::command]
pub async fn list_git_credential_hosts() -> Result<Vec<GitCredentialHostView>, String> {
    let config_path = ConfigPaths::detect().projects_path();
    tauri::async_runtime::spawn_blocking(move || Ok(credential_hosts_at(&config_path)))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn store_git_credential(
    host: String,
    username: String,
    token: String,
    app_id: String,
) -> Result<String, String> {
    let config_path = ConfigPaths::detect().projects_path();
    tauri::async_runtime::spawn_blocking(move || {
        let git_path = effective_git_path_at(&config_path)?;
        store_git_credential_at(
            &config_path,
            &host,
            &username,
            &token,
            &app_id,
            &git_path,
            &GitCredentialSink,
        )
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn test_git_credential(app_id: String) -> Result<String, String> {
    let config_path = ConfigPaths::detect().projects_path();
    tauri::async_runtime::spawn_blocking(move || test_git_credential_at(&config_path, &app_id))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn clear_git_credential(host: String, username: String) -> Result<(), String> {
    let config_path = ConfigPaths::detect().projects_path();
    tauri::async_runtime::spawn_blocking(move || {
        let git_path = effective_git_path_at(&config_path)?;
        clear_git_credential_at(&git_path, &host, &username)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingSink {
        calls: Mutex<Vec<String>>,
    }

    impl CredentialSink for RecordingSink {
        fn approve(&self, _git_path: &Path, input: &str) -> Result<(), String> {
            self.calls.lock().unwrap().push(input.to_string());
            Ok(())
        }
    }

    fn app(id: &str, repo: &str, local: &Path) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "name": format!("App {id}"),
            "repoUrl": repo,
            "localPath": local.to_string_lossy(),
        })
    }

    fn write_config(path: &Path, apps: Vec<serde_json::Value>) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let config = serde_json::json!({ "version": 2, "apps": apps });
        std::fs::write(path, serde_json::to_string(&config).unwrap()).unwrap();
    }

    fn write_settings_git_path(config_path: &Path, git_path: &str) {
        let settings_path = settings_path_for(config_path);
        std::fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        let mut settings = sm_updater::settings::Settings::default();
        settings.tools.git.path = git_path.to_string();
        std::fs::write(settings_path, serde_json::to_string(&settings).unwrap()).unwrap();
    }

    fn fake_git(temp: &tempfile::TempDir) -> PathBuf {
        let git = temp.path().join("git.exe");
        std::fs::write(&git, b"stub").unwrap();
        git
    }

    #[test]
    fn parses_supported_remote_host_variants() {
        assert_eq!(
            host_from_repo_url("https://gitlab.com/group/repo.git").as_deref(),
            Some("gitlab.com")
        );
        assert_eq!(
            host_from_repo_url("http://Example.COM:8080/group/repo.git").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            host_from_repo_url("ssh://git@git.example.com:2222/group/repo.git").as_deref(),
            Some("git.example.com")
        );
        assert_eq!(
            host_from_repo_url("git@github.com:owner/repo.git").as_deref(),
            Some("github.com")
        );
        assert_eq!(
            host_from_repo_url("git://git.example.com/repo.git").as_deref(),
            Some("git.example.com")
        );
        assert_eq!(
            host_from_repo_url("https://oauth2:secret@GitLab.com/group/repo").as_deref(),
            Some("gitlab.com")
        );
        assert_eq!(host_from_repo_url("  "), None);
        assert_eq!(host_from_repo_url("not a remote"), None);
        assert_eq!(host_from_repo_url("file:///C:/repo"), None);
        assert_eq!(host_from_repo_url("https:///repo"), None);
    }

    #[test]
    fn groups_hosts_case_insensitively_and_sorts() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("data/projects.json");
        write_config(
            &config_path,
            vec![
                app("alpha", "https://GitLab.com/group/a.git", temp.path()),
                app("beta", "ssh://git@gitlab.com:2222/group/b.git", temp.path()),
                app("gamma", "git@github.com:org/g.git", temp.path()),
                app("delta", "https://github.com/org/d.git", temp.path()),
                app("epsilon", "", temp.path()),
            ],
        );

        let hosts = credential_hosts_at(&config_path);
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].host, "github.com");
        assert_eq!(hosts[0].apps, ["delta", "gamma"]);
        assert_eq!(hosts[1].host, "gitlab.com");
        assert_eq!(hosts[1].apps, ["alpha", "beta"]);
    }

    #[test]
    fn missing_or_invalid_config_has_no_hosts() {
        let temp = tempfile::tempdir().unwrap();
        assert!(credential_hosts_at(&temp.path().join("missing.json")).is_empty());

        let invalid = temp.path().join("invalid.json");
        std::fs::write(&invalid, "{not-json").unwrap();
        assert!(credential_hosts_at(&invalid).is_empty());
    }

    #[test]
    fn effective_git_path_reports_missing_executable() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("data/projects.json");
        write_config(&config_path, Vec::new());
        write_settings_git_path(&config_path, r"C:\definitely-missing\git.exe");

        let error = effective_git_path_at(&config_path).unwrap_err();
        assert!(error.contains("not found"), "{error}");
    }

    #[test]
    fn store_credential_uses_mock_sink_without_returning_token() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let config_path = temp.path().join("data/projects.json");
        write_config(
            &config_path,
            vec![app("demo", "https://gitlab.com/group/demo.git", &project)],
        );
        let git = fake_git(&temp);
        let sink = RecordingSink::default();

        let message = store_git_credential_at(
            &config_path,
            "gitlab.com",
            "oauth2",
            "glpat-secret-token",
            "demo",
            &git,
            &sink,
        )
        .unwrap();

        assert!(message.contains("oauth2@gitlab.com"));
        assert!(message.contains("DPAPI"));
        assert!(!message.contains("glpat-secret-token"));
        let calls = sink.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert!(calls[0].contains("host=gitlab.com"));
        assert!(calls[0].contains("username=oauth2"));
        assert!(calls[0].contains("password=glpat-secret-token"));
    }

    #[test]
    fn store_credential_rejects_missing_app_project_and_token() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("data/projects.json");
        write_config(&config_path, Vec::new());
        let git = fake_git(&temp);
        let sink = RecordingSink::default();

        let error = store_git_credential_at(
            &config_path,
            "gitlab.com",
            "oauth2",
            "token",
            "missing",
            &git,
            &sink,
        )
        .unwrap_err();
        assert!(error.contains("missing"), "{error}");

        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        write_config(
            &config_path,
            vec![app("demo", "https://gitlab.com/group/demo.git", &project)],
        );
        let error = store_git_credential_at(
            &config_path,
            "gitlab.com",
            "oauth2",
            "token\npassword=leak",
            "demo",
            &git,
            &sink,
        )
        .unwrap_err();
        assert!(error.contains("Token"));
        assert!(!error.contains("leak"));
        assert!(sink.calls.lock().unwrap().is_empty());

        let missing_project = temp.path().join("missing-project");
        write_config(
            &config_path,
            vec![app(
                "demo",
                "https://gitlab.com/group/demo.git",
                &missing_project,
            )],
        );
        let error = store_git_credential_at(
            &config_path,
            "gitlab.com",
            "oauth2",
            "token",
            "demo",
            &git,
            &sink,
        )
        .unwrap_err();
        assert!(error.contains("Project folder"), "{error}");

        write_config(
            &config_path,
            vec![app("demo", "https://gitlab.com/group/demo.git", &project)],
        );
        let error = store_git_credential_at(
            &config_path,
            "github.com",
            "oauth2",
            "token",
            "demo",
            &git,
            &sink,
        )
        .unwrap_err();
        assert!(error.contains("does not match"), "{error}");
        assert!(sink.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn test_credential_reports_missing_app_and_git_path() {
        let temp = tempfile::tempdir().unwrap();
        let config_path = temp.path().join("data/projects.json");
        write_config(&config_path, Vec::new());
        let error = test_git_credential_at(&config_path, "missing").unwrap_err();
        assert!(error.contains("missing"), "{error}");

        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        write_config(
            &config_path,
            vec![app("demo", "https://gitlab.com/group/demo.git", &project)],
        );
        write_settings_git_path(&config_path, r"C:\definitely-missing\git.exe");
        let error = test_git_credential_at(&config_path, "demo").unwrap_err();
        assert!(error.contains("not found"), "{error}");
    }

    #[test]
    fn test_credential_rejects_unsupported_remote() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let config_path = temp.path().join("data/projects.json");
        write_config(&config_path, vec![app("demo", "file:///C:/repo", &project)]);

        let error = test_git_credential_at(&config_path, "demo").unwrap_err();
        assert!(error.contains("HTTP/SSH"), "{error}");
    }

    #[test]
    fn clear_credential_validates_before_spawning_git() {
        let temp = tempfile::tempdir().unwrap();
        let git = fake_git(&temp);
        let error = clear_git_credential_at(&git, "gitlab.com", "oauth\n2").unwrap_err();
        assert!(error.contains("Username"), "{error}");

        let missing = temp.path().join("missing-git.exe");
        let error = clear_git_credential_at(&missing, "gitlab.com", "oauth2").unwrap_err();
        assert!(error.contains("Git"), "{error}");
    }

    #[test]
    fn reject_input_has_no_password_or_token() {
        let input = sm_updater::credential::credential_reject_input("gitlab.com", "oauth2");
        assert_eq!(
            input,
            "protocol=https\nhost=gitlab.com\nusername=oauth2\n\n"
        );
        assert!(!input.contains("password="));
        assert!(!input.contains("token="));
    }
}
