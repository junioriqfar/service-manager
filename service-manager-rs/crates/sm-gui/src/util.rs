use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use sm_config::ConfigPaths;
use sm_core::ServiceConfig;

pub const LOG_MAX_BYTES: u64 = 131_072;
pub const LOG_MAX_LINES: usize = 600;

pub fn read_log_tail(path: &Path, max_bytes: u64) -> String {
    if !path.is_file() {
        return "(file log belum ada)".to_string();
    }

    let mut file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) => return format!("(gagal membuka log: {error})"),
    };

    let length = match file.metadata() {
        Ok(metadata) => metadata.len(),
        Err(error) => return format!("(gagal membaca metadata log: {error})"),
    };
    if length == 0 {
        return "(file log kosong)".to_string();
    }

    let start = length.saturating_sub(max_bytes);
    if let Err(error) = file.seek(SeekFrom::Start(start)) {
        return format!("(gagal seek log: {error})");
    }

    let mut buffer = Vec::new();
    if let Err(error) = file.read_to_end(&mut buffer) {
        return format!("(gagal membaca log: {error})");
    }

    let mut text = String::from_utf8_lossy(&buffer).to_string();
    if start > 0 {
        if let Some(index) = text.find('\n') {
            text = text[index + 1..].to_string();
        }
    }

    let lines: Vec<&str> = text.split('\n').collect();
    if lines.len() > LOG_MAX_LINES {
        return lines[lines.len() - LOG_MAX_LINES..]
            .join("\n")
            .trim_end_matches('\r')
            .to_string();
    }

    text.trim_end_matches(['\r', '\n']).to_string()
}

pub fn build_log_view(config: &ServiceConfig) -> String {
    let mut paths: Vec<&str> = Vec::new();
    if !config.stdout_log.trim().is_empty() {
        paths.push(config.stdout_log.trim());
    }
    if !config.stderr_log.trim().is_empty()
        && !config
            .stderr_log
            .trim()
            .eq_ignore_ascii_case(config.stdout_log.trim())
    {
        paths.push(config.stderr_log.trim());
    }

    if paths.is_empty() {
        return format!(
            "(Service '{}' belum punya path log. Klik Edit untuk mengatur AppStdout/AppStderr.)",
            config.name
        );
    }

    let mut chunks: Vec<String> = Vec::new();
    for path in paths {
        chunks.push(format!("=== {path} ==="));
        chunks.push(read_log_tail(Path::new(path), LOG_MAX_BYTES));
        chunks.push(String::new());
    }

    chunks.join("\r\n").trim_end().to_string()
}

pub fn export_services(paths: &ConfigPaths, services: &[ServiceConfig]) -> Result<PathBuf, String> {
    fs::create_dir_all(paths.home()).map_err(|error| error.to_string())?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let destination = paths.home().join(format!("services-export-{stamp}.json"));
    let payload = serde_json::json!({
        "version": 1,
        "services": services,
    });
    let content = serde_json::to_string_pretty(&payload).map_err(|error| error.to_string())?;
    fs::write(&destination, content).map_err(|error| error.to_string())?;
    Ok(destination)
}

pub fn open_path(path: &Path) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("Path tidak ditemukan: {}", path.display()));
    }
    std::process::Command::new("explorer.exe")
        .arg(path)
        .spawn()
        .map_err(|error| format!("Gagal membuka path: {error}"))?;
    Ok(())
}

pub fn append_app_log(paths: &ConfigPaths, message: &str) {
    let _ = fs::create_dir_all(paths.logs_dir());
    let line = format!(
        "{}  {}\r\n",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        message
    );
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(paths.app_log_path())
    {
        let _ = file.write_all(line.as_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_log_is_reported() {
        let temp = tempfile::tempdir().unwrap();
        assert_eq!(
            read_log_tail(&temp.path().join("missing.log"), LOG_MAX_BYTES),
            "(file log belum ada)"
        );
    }

    #[test]
    fn empty_log_is_reported() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("empty.log");
        fs::write(&path, b"").unwrap();
        assert_eq!(read_log_tail(&path, LOG_MAX_BYTES), "(file log kosong)");
    }

    #[test]
    fn reads_tail_and_drops_partial_first_line() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("app.log");
        fs::write(&path, b"first-line\nsecond-line\nthird-line\n").unwrap();
        let tail = read_log_tail(&path, 20);
        assert!(tail.contains("third-line"));
        assert!(!tail.contains("first-line"));
    }

    #[test]
    fn limits_number_of_lines() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("big.log");
        let content = (0..1000)
            .map(|index| format!("line-{index}"))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&path, content).unwrap();
        let tail = read_log_tail(&path, 131_072);
        assert_eq!(tail.lines().count(), LOG_MAX_LINES);
        assert!(tail.ends_with("line-999"));
    }

    #[test]
    fn builds_combined_log_view() {
        let temp = tempfile::tempdir().unwrap();
        let stdout = temp.path().join("out.log");
        let stderr = temp.path().join("err.log");
        fs::write(&stdout, b"out\n").unwrap();
        fs::write(&stderr, b"err\n").unwrap();
        let config = ServiceConfig {
            name: "demo".into(),
            stdout_log: stdout.to_string_lossy().to_string(),
            stderr_log: stderr.to_string_lossy().to_string(),
            ..ServiceConfig::default()
        };
        let view = build_log_view(&config);
        assert!(view.contains("=== "));
        assert!(view.contains("out"));
        assert!(view.contains("err"));
    }

    #[test]
    fn log_view_without_paths_has_hint() {
        let config = ServiceConfig {
            name: "demo".into(),
            ..ServiceConfig::default()
        };
        assert!(build_log_view(&config).contains("belum punya path log"));
    }

    #[test]
    fn export_writes_versioned_json() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path());
        let service = ServiceConfig {
            name: "demo".into(),
            executable: r"C:\demo.exe".into(),
            ..ServiceConfig::default()
        };
        let exported = export_services(&paths, &[service]).unwrap();
        let content = fs::read_to_string(exported).unwrap();
        let value: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["services"][0]["name"], "demo");
    }

    #[test]
    fn app_log_is_appended_with_timestamp() {
        let temp = tempfile::tempdir().unwrap();
        let paths = ConfigPaths::new(temp.path());
        append_app_log(&paths, "hello");
        append_app_log(&paths, "world");
        let content = fs::read_to_string(paths.app_log_path()).unwrap();
        assert!(content.contains("hello"));
        assert!(content.contains("world"));
        assert!(content.lines().count() >= 2);
    }
}
