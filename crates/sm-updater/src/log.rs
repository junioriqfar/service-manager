use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Nomor urut global agar dua app yang dicek pada detik yang sama tidak pernah
/// memakai nama file log yang sama.
static UNIQUE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

const MAX_SLUG_CHARS: usize = 40;

pub struct UpdateLogger {
    run_log: PathBuf,
    project_log: Option<PathBuf>,
}

impl UpdateLogger {
    /// Nama lama `project-update-<stamp>.log`, dipakai mode legacy
    /// `update_project` dan test yang sudah ada.
    pub fn new(log_dir: &Path, project_path: &Path, stamp: &str) -> Self {
        Self::with_suffix(log_dir, project_path, stamp)
    }

    /// Nama unik per app: `project-update-<stamp>-<slug>-<sequence>.log`.
    /// Run log dan project log memakai suffix yang sama.
    pub fn new_unique(log_dir: &Path, project_path: &Path, stamp: &str) -> Self {
        let slug = project_slug(project_path);
        let sequence = UNIQUE_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
        Self::with_suffix(log_dir, project_path, &format!("{stamp}-{slug}-{sequence}"))
    }

    fn with_suffix(log_dir: &Path, project_path: &Path, suffix: &str) -> Self {
        let _ = fs::create_dir_all(log_dir);
        let run_log = log_dir.join(format!("project-update-{suffix}.log"));

        // Jangan membuat folder storage pada project non-Laravel: folder baru
        // membuat working tree Git kotor dan update akan selalu dilewati.
        let storage_dir = project_path.join("storage");
        let project_log_dir = storage_dir.join("logs");
        let project_log = if storage_dir.is_dir() && fs::create_dir_all(&project_log_dir).is_ok() {
            Some(project_log_dir.join(format!("project-update-{suffix}.log")))
        } else {
            None
        };

        Self {
            run_log,
            project_log,
        }
    }

    pub fn log(&self, message: &str) {
        let message = sm_core::redact_secrets(message);
        let line = format!(
            "{} {}\r\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            message
        );
        print!("{line}");
        let _ = append(&self.run_log, &line);
        if let Some(path) = &self.project_log {
            let _ = append(path, &line);
        }
    }

    pub fn run_log_path(&self) -> &Path {
        &self.run_log
    }

    pub fn project_log_path(&self) -> Option<&Path> {
        self.project_log.as_deref()
    }
}

fn append(path: &Path, line: &str) -> std::io::Result<()> {
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(line.as_bytes())
}

/// Folder project yang aman dipakai di nama file Windows. Karakter yang tidak
/// valid diganti `-`, duplikat `-` dirapikan, dan panjangnya dibatasi.
fn sanitize_log_slug(value: &str) -> String {
    let mut slug: String = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect();

    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    slug = slug
        .trim_matches(|character| matches!(character, '-' | '.' | ' '))
        .to_string();
    if slug.chars().count() > MAX_SLUG_CHARS {
        slug = slug.chars().take(MAX_SLUG_CHARS).collect();
        slug = slug
            .trim_matches(|character| matches!(character, '-' | '.' | ' '))
            .to_string();
    }
    slug
}

fn project_slug(project_path: &Path) -> String {
    let folder = project_path
        .file_name()
        .map(|value| value.to_string_lossy())
        .unwrap_or_default();
    let slug = sanitize_log_slug(&folder);
    if slug.is_empty() {
        "project".to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_run_and_project_logs() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(project.join("storage")).unwrap();
        let logger = UpdateLogger::new(&temp.path().join("logs"), &project, "20260101-000000");
        logger.log("hello");

        let run_log = logger.run_log_path();
        let project_log = logger.project_log_path().unwrap();
        assert!(run_log.is_file());
        assert!(project_log.is_file());
        assert!(fs::read_to_string(run_log).unwrap().contains("hello"));
        assert!(fs::read_to_string(project_log).unwrap().contains("hello"));
    }

    #[test]
    fn does_not_create_storage_for_non_laravel_project() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let logger = UpdateLogger::new(&temp.path().join("logs"), &project, "20260101-000000");
        logger.log("hello");
        assert!(logger.project_log_path().is_none());
        assert!(!project.join("storage").exists());
    }

    #[test]
    fn log_redacts_secrets() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let logger = UpdateLogger::new(&temp.path().join("logs"), &project, "20260101-000000");
        logger.log("token=ghp_abcdefghijklmnop12345 password=hunter2");
        let content = fs::read_to_string(logger.run_log_path()).unwrap();
        assert!(!content.contains("ghp_abcdefghijklmnop12345"));
        assert!(!content.contains("hunter2"));
        assert!(content.contains("[REDACTED]"));
    }

    #[test]
    fn unique_logger_paths_differ_within_same_stamp() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(project.join("storage")).unwrap();
        let log_dir = temp.path().join("logs");

        let first = UpdateLogger::new_unique(&log_dir, &project, "20260101-000000");
        let second = UpdateLogger::new_unique(&log_dir, &project, "20260101-000000");

        assert_ne!(first.run_log_path(), second.run_log_path());
        assert_ne!(first.project_log_path(), second.project_log_path());
        assert!(first
            .run_log_path()
            .to_string_lossy()
            .contains("20260101-000000-project-"));
    }

    #[test]
    fn unique_logger_uses_same_suffix_for_run_and_project_logs() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("My App");
        fs::create_dir_all(project.join("storage")).unwrap();
        let logger =
            UpdateLogger::new_unique(&temp.path().join("logs"), &project, "20260101-000000");

        let run_name = logger
            .run_log_path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let project_name = logger
            .project_log_path()
            .unwrap()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert_eq!(run_name, project_name);
        assert!(run_name.starts_with("project-update-20260101-000000-My-App-"));
        assert!(run_name.ends_with(".log"));
    }

    #[test]
    fn unique_logger_sanitizes_invalid_filename_characters() {
        assert_eq!(sanitize_log_slug("normal-app_1.0"), "normal-app_1.0");
        assert_eq!(sanitize_log_slug("bad<>:\"/\\|?*name"), "bad-name");
        assert_eq!(sanitize_log_slug("  ..  "), "");
        assert_eq!(sanitize_log_slug("my  app"), "my-app");
        assert_eq!(project_slug(Path::new("C:/apps/")), "apps");
        assert_eq!(project_slug(Path::new("")), "project");
        assert!(sanitize_log_slug(&"x".repeat(100)).chars().count() <= MAX_SLUG_CHARS);
    }

    #[test]
    fn unique_logger_keeps_retention_prefix() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        fs::create_dir_all(&project).unwrap();
        let logger =
            UpdateLogger::new_unique(&temp.path().join("logs"), &project, "20260101-000000");
        let name = logger
            .run_log_path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();

        assert!(name.starts_with("project-update-"));
        assert!(name.ends_with(".log"));
    }
}
