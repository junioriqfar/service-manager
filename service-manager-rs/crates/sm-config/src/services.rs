use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use chrono::{SecondsFormat, Utc};
use serde::Serialize;
use sm_core::ServiceConfig;

use crate::ConfigError;

const BACKUP_RETENTION: usize = 20;

#[derive(Debug, Default, Clone)]
pub struct ServicesLoadResult {
    pub services: Vec<ServiceConfig>,
    pub warning: Option<String>,
    pub broken_backup: Option<PathBuf>,
}

#[derive(Serialize)]
struct ServicesFile<'a> {
    version: u32,
    services: &'a [ServiceConfig],
}

fn now_iso() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn stamp() -> String {
    Utc::now().format("%Y%m%d-%H%M%S-%3f").to_string()
}

fn fill_timestamps(services: &mut [ServiceConfig]) {
    for service in services.iter_mut() {
        if service.added_at.trim().is_empty() {
            service.added_at = now_iso();
        }
        if service.updated_at.trim().is_empty() {
            service.updated_at = service.added_at.clone();
        }
    }
}

fn parse_services(raw: &str) -> Result<Vec<ServiceConfig>, String> {
    let value: serde_json::Value = serde_json::from_str(raw).map_err(|error| error.to_string())?;

    let items = match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Object(map) => match map.get("services") {
            Some(serde_json::Value::Array(items)) => items.clone(),
            Some(_) => return Err("Field 'services' harus berupa array.".to_string()),
            None => Vec::new(),
        },
        _ => return Err("Config harus berupa object atau array.".to_string()),
    };

    let mut services = Vec::with_capacity(items.len());
    for (index, item) in items.into_iter().enumerate() {
        let mut service: ServiceConfig = serde_json::from_value(item)
            .map_err(|error| format!("Service #{} tidak valid: {}", index + 1, error))?;
        service.normalize();
        services.push(service);
    }

    Ok(services)
}

fn copy_broken(path: &Path, backup_dir: &Path) -> Option<PathBuf> {
    if !path.is_file() {
        return None;
    }
    if fs::create_dir_all(backup_dir).is_err() {
        return None;
    }
    let mut destination = backup_dir.join(format!("services-broken-{}.json", stamp()));
    let mut counter = 1;
    while destination.exists() {
        destination = backup_dir.join(format!("services-broken-{}-{}.json", stamp(), counter));
        counter += 1;
    }
    fs::copy(path, &destination).ok()?;
    Some(destination)
}

/// Baca config dengan perilaku GUI: config rusak tidak membuat aplikasi gagal,
/// tetapi dicadangkan dan dikembalikan sebagai daftar kosong + peringatan.
pub fn load_services_lenient(path: &Path, backup_dir: &Path) -> ServicesLoadResult {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return ServicesLoadResult::default(),
        Err(error) => {
            return ServicesLoadResult {
                services: Vec::new(),
                warning: Some(format!("Gagal membaca config: {error}")),
                broken_backup: None,
            }
        }
    };

    if raw.trim().is_empty() {
        return ServicesLoadResult::default();
    }

    match parse_services(&raw) {
        Ok(mut services) => {
            fill_timestamps(&mut services);
            ServicesLoadResult {
                services,
                warning: None,
                broken_backup: None,
            }
        }
        Err(message) => {
            let backup = copy_broken(path, backup_dir);
            let warning = match &backup {
                Some(path) => format!(
                    "Config tidak valid: {message}. File lama dicadangkan ke {}.",
                    path.display()
                ),
                None => format!("Config tidak valid: {message}."),
            };
            ServicesLoadResult {
                services: Vec::new(),
                warning: Some(warning),
                broken_backup: backup,
            }
        }
    }
}

/// Parse isi JSON (object `{version, services}` atau array legacy) tanpa menyentuh disk.
pub fn parse_services_json(raw: &str) -> Result<Vec<ServiceConfig>, ConfigError> {
    let mut services = parse_services(raw).map_err(ConfigError::Invalid)?;
    fill_timestamps(&mut services);
    Ok(services)
}

/// Baca config secara ketat untuk CLI. Config rusak tetap dicadangkan lebih dulu.
pub fn load_services_strict(
    path: &Path,
    backup_dir: &Path,
) -> Result<Vec<ServiceConfig>, ConfigError> {
    let raw = match fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };

    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }

    match parse_services(&raw) {
        Ok(mut services) => {
            fill_timestamps(&mut services);
            Ok(services)
        }
        Err(message) => {
            copy_broken(path, backup_dir);
            Err(ConfigError::Invalid(message))
        }
    }
}

fn prune_backups(backup_dir: &Path, keep: usize) {
    let Ok(entries) = fs::read_dir(backup_dir) else {
        return;
    };

    let mut files: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| {
            entry
                .file_type()
                .map(|kind| kind.is_file())
                .unwrap_or(false)
        })
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(|name| name.starts_with("services-") && name.ends_with(".json"))
                .unwrap_or(false)
        })
        .collect();

    // Nama file memiliki timestamp, sehingga urutan nama = urutan waktu.
    files.sort_by(|left, right| right.file_name().cmp(&left.file_name()));
    for path in files.into_iter().skip(keep) {
        let _ = fs::remove_file(path);
    }
}

/// Backup config yang sekarang (jika ada) lalu jaga maksimal 20 file.
pub fn backup_services(path: &Path, backup_dir: &Path) -> Result<Option<PathBuf>, ConfigError> {
    if !path.is_file() {
        return Ok(None);
    }
    fs::create_dir_all(backup_dir)?;

    let mut destination = backup_dir.join(format!("services-{}.json", stamp()));
    let mut counter = 1;
    while destination.exists() {
        destination = backup_dir.join(format!("services-{}-{}.json", stamp(), counter));
        counter += 1;
    }
    fs::copy(path, &destination)?;
    prune_backups(backup_dir, BACKUP_RETENTION);
    Ok(Some(destination))
}

/// Simpan config secara atomik dan buat backup file sebelumnya.
pub fn save_services(
    path: &Path,
    backup_dir: &Path,
    services: &[ServiceConfig],
) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    backup_services(path, backup_dir)?;

    let mut normalized: Vec<ServiceConfig> = services.to_vec();
    for service in normalized.iter_mut() {
        service.normalize();
    }
    fill_timestamps(&mut normalized);

    let payload = ServicesFile {
        version: 1,
        services: &normalized,
    };
    let json = serde_json::to_string_pretty(&payload)?;

    let temp_path = path.with_extension(format!("json.tmp-{}", std::process::id()));
    fs::write(&temp_path, json)?;
    if let Err(error) = fs::rename(&temp_path, path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sm_core::{ServiceType, StartupMode};

    fn sample() -> ServiceConfig {
        ServiceConfig {
            name: "my-app".into(),
            executable: r"C:\apps\app.exe".into(),
            startup: StartupMode::Delayed,
            ..ServiceConfig::default()
        }
    }

    #[test]
    fn missing_config_returns_empty() {
        let temp = tempfile::tempdir().unwrap();
        let result = load_services_lenient(
            &temp.path().join("services.json"),
            &temp.path().join("backup"),
        );
        assert!(result.services.is_empty());
        assert!(result.warning.is_none());
    }

    #[test]
    fn roundtrip_save_and_load() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("services.json");
        let backup = temp.path().join("backup");

        save_services(&path, &backup, &[sample()]).unwrap();
        let result = load_services_lenient(&path, &backup);
        assert!(result.warning.is_none());
        assert_eq!(result.services.len(), 1);
        assert_eq!(result.services[0].name, "my-app");
        assert_eq!(result.services[0].display_name, "my-app");
        assert_eq!(result.services[0].startup, StartupMode::Delayed);
        assert!(!result.services[0].added_at.is_empty());
        assert_eq!(result.services[0].updated_at, result.services[0].added_at);
    }

    #[test]
    fn accepts_legacy_array_format() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("services.json");
        fs::write(
            &path,
            r#"[{"name":"legacy","executable":"C:\\legacy.exe"}]"#,
        )
        .unwrap();

        let result = load_services_lenient(&path, &temp.path().join("backup"));
        assert_eq!(result.services.len(), 1);
        assert_eq!(result.services[0].name, "legacy");
        assert_eq!(result.services[0].service_type, ServiceType::Nssm);
    }

    #[test]
    fn ignores_unknown_fields() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("services.json");
        fs::write(
            &path,
            r#"{"version":1,"futureOption":true,"services":[{"name":"app","executable":"C:\\app.exe","unknown":"x"}]}"#,
        )
        .unwrap();

        let result = load_services_lenient(&path, &temp.path().join("backup"));
        assert_eq!(result.services.len(), 1);
        assert_eq!(result.services[0].name, "app");
    }

    #[test]
    fn broken_config_is_backed_up_and_reported() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("services.json");
        let backup = temp.path().join("backup");
        fs::write(&path, "{ not valid json").unwrap();

        let result = load_services_lenient(&path, &backup);
        assert!(result.services.is_empty());
        assert!(result.warning.is_some());
        let backup_path = result.broken_backup.expect("backup harus dibuat");
        assert!(backup_path.is_file());
        assert!(backup_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("services-broken-"));

        let strict = load_services_strict(&path, &backup);
        assert!(strict.is_err());
    }

    #[test]
    fn save_creates_backup_of_previous_file() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("services.json");
        let backup = temp.path().join("backup");

        save_services(&path, &backup, &[sample()]).unwrap();
        let mut second = sample();
        second.name = "second".into();
        save_services(&path, &backup, &[second]).unwrap();

        let backups: Vec<_> = fs::read_dir(&backup)
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().starts_with("services-"))
            .collect();
        assert_eq!(backups.len(), 1);
    }

    #[test]
    fn backup_retention_keeps_twenty_newest() {
        let temp = tempfile::tempdir().unwrap();
        let backup = temp.path().join("backup");
        fs::create_dir_all(&backup).unwrap();
        for index in 0..25 {
            let name = format!("services-20260101-000000-{:03}.json", index);
            fs::write(backup.join(name), "{}").unwrap();
        }

        prune_backups(&backup, BACKUP_RETENTION);

        let count = fs::read_dir(&backup).unwrap().count();
        assert_eq!(count, 20);
        assert!(!backup.join("services-20260101-000000-000.json").exists());
        assert!(backup.join("services-20260101-000000-024.json").exists());
    }

    #[test]
    fn parses_json_text_without_disk_access() {
        let services = parse_services_json(
            r#"{"version":1,"services":[{"name":"demo","executable":"C:\\demo.exe"}]}"#,
        )
        .unwrap();
        assert_eq!(services.len(), 1);
        assert_eq!(services[0].display_name, "demo");
        assert!(!services[0].added_at.is_empty());
        assert!(parse_services_json("{ broken").is_err());
    }

    #[test]
    fn atomic_save_leaves_valid_json() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("services.json");
        let backup = temp.path().join("backup");
        save_services(&path, &backup, &[sample()]).unwrap();

        let raw = fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(value["version"], 1);
        assert!(value["services"].is_array());
        let temp_files: Vec<_> = fs::read_dir(temp.path())
            .unwrap()
            .flatten()
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(temp_files.is_empty());
    }
}
