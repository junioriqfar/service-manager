//! Manual log retention for updater and project logs.
//!
//! Retention is deliberately conservative: only the two known log file name
//! patterns are considered, symlinks/directories are ignored, and files
//! modified within an active grace window are never deleted. No retention is
//! executed automatically by updates; callers invoke this explicitly.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::registry::AppConfig;

/// Default grace period protecting logs that may still be open/written.
pub const DEFAULT_ACTIVE_GRACE: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PruneResult {
    /// ISO timestamp of the cutoff: files modified before this are candidates.
    pub cutoff: String,
    pub scanned_files: u64,
    pub deleted_files: u64,
    pub deleted_bytes: u64,
    pub skipped_active: u64,
    pub errors: Vec<String>,
}

impl PruneResult {
    fn empty(cutoff: String) -> Self {
        Self {
            cutoff,
            scanned_files: 0,
            deleted_files: 0,
            deleted_bytes: 0,
            skipped_active: 0,
            errors: Vec::new(),
        }
    }

    /// Merge another result into this one. `cutoff` is kept from `self`.
    pub fn merge(&mut self, other: PruneResult) {
        self.scanned_files += other.scanned_files;
        self.deleted_files += other.deleted_files;
        self.deleted_bytes += other.deleted_bytes;
        self.skipped_active += other.skipped_active;
        self.errors.extend(other.errors);
    }

    fn merge_stats(&mut self, stats: DirectoryStats) {
        self.scanned_files += stats.scanned_files;
        self.deleted_files += stats.deleted_files;
        self.deleted_bytes += stats.deleted_bytes;
        self.skipped_active += stats.skipped_active;
        self.errors.extend(stats.errors);
    }
}

#[derive(Debug, Default)]
struct DirectoryStats {
    scanned_files: u64,
    deleted_files: u64,
    deleted_bytes: u64,
    skipped_active: u64,
    errors: Vec<String>,
}

fn is_project_log_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    name.starts_with("project-update-") && name.ends_with(".log")
}

fn is_updater_log_name(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    (is_project_log_name(&name) || name.starts_with("updater-task-")) && name.ends_with(".log")
}

/// Delete old `project-update-*.log` / `updater-task-*.log` files directly in
/// `log_dir`.
pub fn prune_logs(log_dir: &Path, days: u32, active_grace: Duration) -> PruneResult {
    let now = SystemTime::now();
    let cutoff = cutoff_before(now, days);
    let active_cutoff = now.checked_sub(active_grace).unwrap_or(UNIX_EPOCH);
    let mut result = PruneResult::empty(iso_time(cutoff));
    result.merge_stats(prune_directory(
        log_dir,
        cutoff,
        active_cutoff,
        is_updater_log_name,
    ));
    result
}

/// Delete old `project-update-*.log` files under `<localPath>/storage/logs` for
/// every registered app. Missing/empty paths are skipped; counts are merged.
pub fn prune_project_logs(apps: &[AppConfig], days: u32, active_grace: Duration) -> PruneResult {
    let now = SystemTime::now();
    let cutoff = cutoff_before(now, days);
    let active_cutoff = now.checked_sub(active_grace).unwrap_or(UNIX_EPOCH);
    let mut result = PruneResult::empty(iso_time(cutoff));

    for app in apps {
        let local_path = app.local_path.trim();
        if local_path.is_empty() {
            continue;
        }
        let log_dir = Path::new(local_path).join("storage").join("logs");
        if !log_dir.is_dir() {
            continue;
        }
        result.merge_stats(prune_directory(
            &log_dir,
            cutoff,
            active_cutoff,
            is_project_log_name,
        ));
    }

    result
}

fn cutoff_before(now: SystemTime, days: u32) -> SystemTime {
    let seconds = (days as u64).saturating_mul(24 * 60 * 60);
    now.checked_sub(Duration::from_secs(seconds))
        .unwrap_or(UNIX_EPOCH)
}

fn prune_directory(
    dir: &Path,
    cutoff: SystemTime,
    active_cutoff: SystemTime,
    matcher: fn(&str) -> bool,
) -> DirectoryStats {
    let mut stats = DirectoryStats::default();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return stats,
        Err(error) => {
            stats.errors.push(format!("{}: {error}", dir.display()));
            return stats;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                stats.errors.push(format!("{}: {error}", dir.display()));
                continue;
            }
        };
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !matcher(name) {
            continue;
        }

        let metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(error) => {
                stats
                    .errors
                    .push(format!("{}: {error}", entry.path().display()));
                continue;
            }
        };
        if !metadata.is_file() {
            continue;
        }
        stats.scanned_files += 1;

        let modified = match metadata.modified() {
            Ok(modified) => modified,
            Err(error) => {
                stats
                    .errors
                    .push(format!("{}: {error}", entry.path().display()));
                continue;
            }
        };
        if modified >= active_cutoff {
            stats.skipped_active += 1;
            continue;
        }
        if modified >= cutoff {
            continue;
        }

        let size = metadata.len();
        match fs::remove_file(entry.path()) {
            Ok(()) => {
                stats.deleted_files += 1;
                stats.deleted_bytes += size;
            }
            Err(error) => stats
                .errors
                .push(format!("{}: {error}", entry.path().display())),
        }
    }

    stats
}

fn iso_time(time: SystemTime) -> String {
    chrono::DateTime::<chrono::Utc>::from(time)
        .with_timezone(&chrono::Local)
        .to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn write_log(path: &Path, age: Duration, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
        let modified = SystemTime::now().checked_sub(age).unwrap_or(UNIX_EPOCH);
        let file = fs::OpenOptions::new().write(true).open(path).unwrap();
        file.set_times(fs::FileTimes::new().set_modified(modified))
            .unwrap();
    }

    fn app(id: &str, path: &Path) -> AppConfig {
        AppConfig {
            id: id.into(),
            local_path: path.to_string_lossy().to_string(),
            ..AppConfig::default()
        }
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn deletes_old_updater_and_project_logs_and_keeps_others() {
        let temp = tempfile::tempdir().unwrap();
        let logs = temp.path().join("logs");
        write_log(
            &logs.join("project-update-old.log"),
            Duration::from_secs(40 * 86_400),
            "old-project",
        );
        write_log(
            &logs.join("updater-task-old.log"),
            Duration::from_secs(40 * 86_400),
            "old-task",
        );
        write_log(
            &logs.join("PROJECT-UPDATE-UPPER.LOG"),
            Duration::from_secs(40 * 86_400),
            "upper",
        );
        write_log(
            &logs.join("project-update-recent.log"),
            Duration::from_secs(2 * 86_400),
            "recent",
        );
        write_log(
            &logs.join("project-update-active.log"),
            Duration::ZERO,
            "active",
        );
        write_log(
            &logs.join("unrelated-old.log"),
            Duration::from_secs(40 * 86_400),
            "unrelated",
        );

        let result = prune_logs(&logs, 30, DEFAULT_ACTIVE_GRACE);

        assert_eq!(result.deleted_files, 3);
        assert_eq!(
            result.deleted_bytes,
            ("old-project".len() + "old-task".len() + "upper".len()) as u64
        );
        assert_eq!(result.skipped_active, 1);
        assert_eq!(result.scanned_files, 5);
        assert!(result.errors.is_empty());
        assert!(result.cutoff.contains('T'));
        assert_eq!(
            names(&logs),
            vec![
                "project-update-active.log",
                "project-update-recent.log",
                "unrelated-old.log"
            ]
        );
    }

    #[test]
    fn keeps_files_newer_than_retention_days() {
        let temp = tempfile::tempdir().unwrap();
        let logs = temp.path().join("logs");
        write_log(
            &logs.join("project-update-8d.log"),
            Duration::from_secs(8 * 86_400),
            "old",
        );
        write_log(
            &logs.join("project-update-3d.log"),
            Duration::from_secs(3 * 86_400),
            "new",
        );

        let result = prune_logs(&logs, 7, DEFAULT_ACTIVE_GRACE);

        assert_eq!(result.deleted_files, 1);
        assert_eq!(result.scanned_files, 2);
        assert_eq!(names(&logs), vec!["project-update-3d.log".to_string()]);
    }

    #[test]
    fn missing_log_directory_is_empty_without_error() {
        let temp = tempfile::tempdir().unwrap();
        let result = prune_logs(&temp.path().join("missing"), 30, DEFAULT_ACTIVE_GRACE);
        assert_eq!(result.scanned_files, 0);
        assert_eq!(result.deleted_files, 0);
        assert!(result.errors.is_empty());
        assert!(!result.cutoff.is_empty());
    }

    #[test]
    fn project_logs_prune_only_registered_apps_and_project_pattern() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        let missing = temp.path().join("missing");
        write_log(
            &first.join("storage/logs/project-update-old.log"),
            Duration::from_secs(10 * 86_400),
            "first",
        );
        write_log(
            &first.join("storage/logs/updater-task-old.log"),
            Duration::from_secs(10 * 86_400),
            "not-project",
        );
        write_log(
            &second.join("storage/logs/project-update-old.log"),
            Duration::from_secs(10 * 86_400),
            "second",
        );
        write_log(
            &second.join("logs/project-update-old.log"),
            Duration::from_secs(10 * 86_400),
            "outside-storage",
        );
        write_log(
            &first.join("storage/logs/project-update-new.log"),
            Duration::ZERO,
            "active",
        );

        let apps = vec![
            app("first", &first),
            app("second", &second),
            app("missing", &missing),
        ];
        let result = prune_project_logs(&apps, 7, DEFAULT_ACTIVE_GRACE);

        assert_eq!(result.deleted_files, 2);
        assert_eq!(
            result.deleted_bytes,
            ("first".len() + "second".len()) as u64
        );
        assert_eq!(result.scanned_files, 3);
        assert_eq!(result.skipped_active, 1);
        assert!(result.errors.is_empty());
        assert!(!first.join("storage/logs/project-update-old.log").exists());
        assert!(!second.join("storage/logs/project-update-old.log").exists());
        assert!(first.join("storage/logs/updater-task-old.log").exists());
        assert!(second.join("logs/project-update-old.log").exists());
        assert!(first.join("storage/logs/project-update-new.log").exists());
    }

    #[test]
    fn merges_counts_and_protects_active_files() {
        let temp = tempfile::tempdir().unwrap();
        let logs = temp.path().join("logs");
        write_log(
            &logs.join("project-update-a.log"),
            Duration::from_secs(9 * 86_400),
            "aa",
        );
        write_log(
            &logs.join("updater-task-b.log"),
            Duration::from_secs(9 * 86_400),
            "bbb",
        );
        write_log(
            &logs.join("project-update-active.log"),
            Duration::from_secs(60),
            "active",
        );

        let result = prune_logs(&logs, 7, DEFAULT_ACTIVE_GRACE);

        assert_eq!(result.scanned_files, 3);
        assert_eq!(result.deleted_files, 2);
        assert_eq!(result.deleted_bytes, 5);
        assert_eq!(result.skipped_active, 1);
        assert!(logs.join("project-update-active.log").is_file());
    }

    #[cfg(windows)]
    #[test]
    fn delete_errors_do_not_abort_remaining_files() {
        use std::os::windows::fs::OpenOptionsExt;

        let temp = tempfile::tempdir().unwrap();
        let logs = temp.path().join("logs");
        let locked = logs.join("project-update-locked.log");
        write_log(&locked, Duration::from_secs(40 * 86_400), "locked");
        write_log(
            &logs.join("project-update-after.log"),
            Duration::from_secs(40 * 86_400),
            "after",
        );

        // Exclusive handle without FILE_SHARE_DELETE makes deletion fail.
        let handle = fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&locked)
            .unwrap();

        let result = prune_logs(&logs, 30, DEFAULT_ACTIVE_GRACE);

        assert!(!result.errors.is_empty());
        assert_eq!(result.deleted_files, 1);
        assert!(!logs.join("project-update-after.log").exists());
        assert!(locked.exists());
        drop(handle);
    }
}
