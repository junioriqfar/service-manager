//! Append-only per-app update history (`data/updater-history.jsonl`).
//!
//! Each line is one JSON object. History is intentionally separate from the
//! per-run log files: log retention never prunes this file, and malformed or
//! partially written lines are skipped when reading.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const MAX_MESSAGE_CHARS: usize = 4_000;
const MAX_HISTORY_LIMIT: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    #[serde(default)]
    pub timestamp: String,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub commit: String,
    #[serde(default)]
    pub duration_ms: u64,
    #[serde(default)]
    pub client_ip: String,
}

/// `updater-history.jsonl` lives next to `projects.json` (normally `data/`).
pub fn history_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("updater-history.jsonl")
}

/// Append one JSON object followed by a newline. The message is redacted and
/// capped before it is written.
pub fn append_history(config_path: &Path, entry: &HistoryEntry) -> Result<(), String> {
    let path = history_path(config_path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }

    let mut entry = entry.clone();
    entry.message = cap_message(&sm_core::redact_secrets(&entry.message));
    let line = serde_json::to_string(&entry).map_err(|error| error.to_string())?;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    writeln!(file, "{line}").map_err(|error| format!("{}: {error}", path.display()))
}

/// Read history newest first, optionally filtered by app id (case-insensitive).
///
/// Reading is best effort: missing files, IO errors, and malformed lines all
/// produce an empty or partial list instead of an error. `limit` is clamped to
/// `1..=500`.
pub fn load_history(config_path: &Path, app_id: Option<&str>, limit: usize) -> Vec<HistoryEntry> {
    let limit = limit.clamp(1, MAX_HISTORY_LIMIT);
    let filter = app_id.map(str::trim).filter(|value| !value.is_empty());
    let Ok(file) = File::open(history_path(config_path)) else {
        return Vec::new();
    };

    let mut entries = Vec::new();
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<HistoryEntry>(line) else {
            continue;
        };
        if let Some(filter) = filter {
            if !entry.app_id.eq_ignore_ascii_case(filter) {
                continue;
            }
        }
        entries.push(entry);
    }

    entries.reverse();
    entries.truncate(limit);
    entries
}

fn cap_message(message: &str) -> String {
    message.chars().take(MAX_MESSAGE_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(app_id: &str, status: &str, message: &str) -> HistoryEntry {
        HistoryEntry {
            timestamp: "2026-11-17T10:00:00+07:00".into(),
            app_id: app_id.into(),
            status: status.into(),
            message: message.into(),
            commit: "abc1234".into(),
            duration_ms: 42,
            client_ip: "10.255.240.143".into(),
        }
    }

    #[test]
    fn appends_and_loads_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("data/projects.json");
        let expected = entry("kasirin", "updated", "deploy ok");

        append_history(&config, &expected).unwrap();

        let loaded = load_history(&config, None, 10);
        assert_eq!(loaded, vec![expected]);
        assert!(history_path(&config).ends_with("updater-history.jsonl"));
        assert_eq!(
            history_path(&config),
            temp.path().join("data/updater-history.jsonl")
        );
    }

    #[test]
    fn returns_newest_first_and_applies_limit() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("data/projects.json");
        for (index, status) in ["first", "second", "third"].iter().enumerate() {
            let mut value = entry("demo", status, "message");
            value.timestamp = format!("2026-11-17T10:00:0{index}+07:00");
            append_history(&config, &value).unwrap();
        }

        let loaded = load_history(&config, None, 2);
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].status, "third");
        assert_eq!(loaded[1].status, "second");

        // Limits are clamped instead of returning nothing/too many entries.
        assert_eq!(load_history(&config, None, 0).len(), 1);
        assert_eq!(load_history(&config, None, usize::MAX).len(), 3);
    }

    #[test]
    fn filters_by_app_id_case_insensitively() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("data/projects.json");
        append_history(&config, &entry("Kasirin", "updated", "one")).unwrap();
        append_history(&config, &entry("other", "failed", "two")).unwrap();

        let loaded = load_history(&config, Some("kasirin"), 50);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].app_id, "Kasirin");
        assert_eq!(loaded[0].status, "updated");
    }

    #[test]
    fn skips_malformed_lines_and_keeps_valid_ones() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("data/projects.json");
        append_history(&config, &entry("demo", "updated", "ok")).unwrap();
        let path = history_path(&config);
        let mut raw = fs::read_to_string(&path).unwrap();
        raw.push_str("this is not json\n");
        raw.push_str("{\"appId\":\"demo\"\n");
        raw.push('\n');
        fs::write(&path, raw).unwrap();

        let loaded = load_history(&config, None, 10);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].status, "updated");
    }

    #[test]
    fn missing_history_file_is_empty() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("data/projects.json");
        assert!(load_history(&config, None, 50).is_empty());
        assert!(!history_path(&config).exists());
    }

    #[test]
    fn redacts_secrets_and_caps_message() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("data/projects.json");
        let message = format!(
            "token=ghp_abcdefghijklmnop12345 password=hunter2 {}",
            "x".repeat(MAX_MESSAGE_CHARS + 100)
        );
        append_history(&config, &entry("demo", "failed", &message)).unwrap();

        let raw = fs::read_to_string(history_path(&config)).unwrap();
        assert!(!raw.contains("ghp_abcdefghijklmnop12345"));
        assert!(!raw.contains("hunter2"));
        assert!(raw.contains("[REDACTED]"));

        let loaded = load_history(&config, None, 10);
        assert_eq!(loaded[0].message.chars().count(), MAX_MESSAGE_CHARS);
    }

    #[test]
    fn writes_one_compact_json_object_per_line() {
        let temp = tempfile::tempdir().unwrap();
        let config = temp.path().join("data/projects.json");
        append_history(&config, &entry("demo", "updated", "one")).unwrap();
        append_history(&config, &entry("demo", "failed", "two")).unwrap();

        let raw = fs::read_to_string(history_path(&config)).unwrap();
        assert!(raw.ends_with('\n'));
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            assert!(line.starts_with('{') && line.ends_with('}'));
            assert!(!line.contains("\n\t"));
            let value: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(value.get("appId").is_some());
            assert!(value.get("durationMs").is_some());
            assert!(value.get("clientIp").is_some());
        }
    }
}
