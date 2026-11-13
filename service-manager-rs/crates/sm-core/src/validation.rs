use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::model::{ServiceConfig, ServiceType};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ValidationError {
    #[error("Nama service wajib diisi.")]
    EmptyName,
    #[error("Nama service '{0}' tidak valid. Gunakan huruf, angka, titik, garis bawah, atau tanda hubung (maksimal 256 karakter).")]
    InvalidName(String),
    #[error("Executable wajib diisi.")]
    EmptyExecutable,
    #[error("Installer Apache harus httpd.exe: {0}")]
    InvalidApacheExecutable(String),
    #[error("Installer PostgreSQL harus pg_ctl.exe: {0}")]
    InvalidPostgreSqlExecutable(String),
    #[error("Apache root wajib diisi.")]
    EmptyApacheRoot,
    #[error("PGDATA wajib diisi.")]
    EmptyPgData,
    #[error("Service account PostgreSQL wajib diisi.")]
    EmptyPgAccount,
    #[error("restartDelaySec minimal 1 detik.")]
    InvalidRestartDelay,
    #[error("Executable tidak ditemukan: {0}")]
    ExecutableNotFound(String),
    #[error("Apache root tidak ditemukan: {0}")]
    ApacheRootNotFound(String),
    #[error("Config Apache tidak ditemukan: {0}")]
    ApacheConfigNotFound(String),
    #[error("PGDATA tidak ditemukan: {0}")]
    PgDataNotFound(String),
}

pub fn validate_service(service: &ServiceConfig) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();

    let name = service.name.trim();
    if name.is_empty() {
        errors.push(ValidationError::EmptyName);
    } else if name.len() > 256 || !name.chars().all(is_valid_service_name_char) {
        errors.push(ValidationError::InvalidName(name.to_string()));
    }

    if service.executable.trim().is_empty() {
        errors.push(ValidationError::EmptyExecutable);
    }

    if service.restart_delay_sec < 1 {
        errors.push(ValidationError::InvalidRestartDelay);
    }

    match service.service_type.normalized() {
        ServiceType::Apache => {
            if service.apache_root.trim().is_empty() {
                errors.push(ValidationError::EmptyApacheRoot);
            }
            let file_name = Path::new(service.executable.trim())
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default();
            if !file_name.is_empty() && !file_name.eq_ignore_ascii_case("httpd.exe") {
                errors.push(ValidationError::InvalidApacheExecutable(file_name));
            }
        }
        ServiceType::PostgreSql => {
            if service.pg_data.trim().is_empty() {
                errors.push(ValidationError::EmptyPgData);
            }
            if service.pg_account.trim().is_empty() {
                errors.push(ValidationError::EmptyPgAccount);
            }
            let file_name = Path::new(service.executable.trim())
                .file_name()
                .map(|value| value.to_string_lossy().to_string())
                .unwrap_or_default();
            if !file_name.is_empty() && !file_name.eq_ignore_ascii_case("pg_ctl.exe") {
                errors.push(ValidationError::InvalidPostgreSqlExecutable(file_name));
            }
        }
        ServiceType::Nssm | ServiceType::Unknown => {}
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn is_valid_service_name_char(value: char) -> bool {
    value.is_ascii_alphanumeric() || matches!(value, '.' | '_' | '-')
}

/// Resolusi path config Apache: relatif terhadap `apache_root` bila tidak absolut.
pub fn resolve_apache_config(service: &ServiceConfig) -> PathBuf {
    let config = if service.apache_config.trim().is_empty() {
        "conf/httpd.conf"
    } else {
        service.apache_config.trim()
    };
    let config_path = Path::new(config);
    if config_path.is_absolute() {
        config_path.to_path_buf()
    } else {
        Path::new(service.apache_root.trim()).join(config_path)
    }
}

/// Validasi keberadaan file/folder di disk. Dipisahkan dari validasi dasar agar
/// GUI dapat menampilkan error tanpa menyentuh filesystem pada unit test.
pub fn validate_service_paths(service: &ServiceConfig) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();
    let executable = Path::new(service.executable.trim());
    if !executable.is_file() {
        errors.push(ValidationError::ExecutableNotFound(
            service.executable.clone(),
        ));
    }

    match service.service_type.normalized() {
        ServiceType::Apache => {
            let root = Path::new(service.apache_root.trim());
            if !root.is_dir() {
                errors.push(ValidationError::ApacheRootNotFound(
                    service.apache_root.clone(),
                ));
            }
            let config = resolve_apache_config(service);
            if !config.is_file() {
                errors.push(ValidationError::ApacheConfigNotFound(
                    config.to_string_lossy().to_string(),
                ));
            }
        }
        ServiceType::PostgreSql => {
            if !Path::new(service.pg_data.trim()).is_dir() {
                errors.push(ValidationError::PgDataNotFound(service.pg_data.clone()));
            }
        }
        ServiceType::Nssm | ServiceType::Unknown => {}
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_nssm() -> ServiceConfig {
        ServiceConfig {
            name: "my-app".into(),
            executable: r"C:\apps\app.exe".into(),
            ..ServiceConfig::default()
        }
    }

    #[test]
    fn accepts_valid_nssm_service() {
        assert_eq!(validate_service(&valid_nssm()), Ok(()));
    }

    #[test]
    fn rejects_empty_name_and_executable() {
        let mut service = valid_nssm();
        service.name = " ".into();
        service.executable = "".into();
        let errors = validate_service(&service).unwrap_err();
        assert!(errors.contains(&ValidationError::EmptyName));
        assert!(errors.contains(&ValidationError::EmptyExecutable));
    }

    #[test]
    fn rejects_invalid_service_name() {
        let mut service = valid_nssm();
        service.name = "my app/../../bad".into();
        let errors = validate_service(&service).unwrap_err();
        assert!(matches!(errors[0], ValidationError::InvalidName(_)));
    }

    #[test]
    fn apache_requires_root_and_httpd() {
        let service = ServiceConfig {
            name: "Apache2.4".into(),
            service_type: ServiceType::Apache,
            executable: r"C:\Apache24\bin\not-httpd.exe".into(),
            ..ServiceConfig::default()
        };
        let errors = validate_service(&service).unwrap_err();
        assert!(errors.contains(&ValidationError::EmptyApacheRoot));
        assert!(errors.contains(&ValidationError::InvalidApacheExecutable(
            "not-httpd.exe".into()
        )));
    }

    #[test]
    fn postgres_requires_data_and_account() {
        let service = ServiceConfig {
            name: "postgresql-16".into(),
            service_type: ServiceType::PostgreSql,
            executable: r"C:\PostgreSQL\bin\pg_ctl.exe".into(),
            pg_data: "".into(),
            pg_account: " ".into(),
            ..ServiceConfig::default()
        };
        let errors = validate_service(&service).unwrap_err();
        assert!(errors.contains(&ValidationError::EmptyPgData));
        assert!(errors.contains(&ValidationError::EmptyPgAccount));
    }

    #[test]
    fn apache_config_resolution_handles_relative_and_absolute() {
        let mut service = ServiceConfig {
            apache_root: r"C:\Apache24".into(),
            apache_config: "conf/httpd.conf".into(),
            ..ServiceConfig::default()
        };
        assert_eq!(
            resolve_apache_config(&service),
            PathBuf::from(r"C:\Apache24\conf\httpd.conf")
        );

        service.apache_config = r"D:\config\httpd.conf".into();
        assert_eq!(
            resolve_apache_config(&service),
            PathBuf::from(r"D:\config\httpd.conf")
        );
    }

    #[test]
    fn path_validation_reports_missing_files() {
        let service = valid_nssm();
        let errors = validate_service_paths(&service).unwrap_err();
        assert!(errors
            .iter()
            .any(|error| matches!(error, ValidationError::ExecutableNotFound(_))));
    }

    #[test]
    fn path_validation_accepts_existing_executable() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("app.exe");
        std::fs::write(&executable, b"test").unwrap();
        let service = ServiceConfig {
            name: "app".into(),
            executable: executable.to_string_lossy().to_string(),
            ..ServiceConfig::default()
        };
        assert_eq!(validate_service_paths(&service), Ok(()));
    }
}
