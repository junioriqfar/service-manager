use std::path::Path;

use sm_core::{
    validate_service, validate_service_paths, ServiceConfig, ServiceType, ValidationError,
};
use thiserror::Error;

use crate::error::ScmError;
use crate::native::{self, NativeError};
use crate::nssm::{Nssm, NssmError};
use crate::process::CommandRunner;
use crate::scm;

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error("Config service tidak valid.")]
    Validation(Vec<ValidationError>),
    #[error("Service '{0}' sudah terpasang.")]
    AlreadyInstalled(String),
    #[error("Service '{0}' belum terpasang.")]
    NotInstalled(String),
    #[error(transparent)]
    Nssm(#[from] NssmError),
    #[error(transparent)]
    Native(#[from] NativeError),
    #[error(transparent)]
    Scm(#[from] ScmError),
    #[error("{0}")]
    Other(String),
}

impl ManagerError {
    pub fn messages(&self) -> Vec<String> {
        match self {
            Self::Validation(errors) => errors.iter().map(|error| error.to_string()).collect(),
            other => vec![other.to_string()],
        }
    }
}

pub fn install_service(
    config: &ServiceConfig,
    nssm_path: &Path,
    runner: &dyn CommandRunner,
) -> Result<(), ManagerError> {
    validate_service(config).map_err(ManagerError::Validation)?;
    validate_service_paths(config).map_err(ManagerError::Validation)?;

    if scm::service_exists(&config.name)? {
        return Err(ManagerError::AlreadyInstalled(config.name.clone()));
    }

    match config.service_type.normalized() {
        ServiceType::Nssm => {
            let nssm = Nssm::new(nssm_path, runner);
            nssm.install(config)?;
        }
        ServiceType::Apache | ServiceType::PostgreSql => {
            native::install(config, runner)?;
        }
        ServiceType::Unknown => {
            return Err(ManagerError::Other("Jenis service tidak dikenal.".into()))
        }
    }

    Ok(())
}

pub fn apply_service(
    config: &ServiceConfig,
    nssm_path: &Path,
    runner: &dyn CommandRunner,
) -> Result<(), ManagerError> {
    validate_service(config).map_err(ManagerError::Validation)?;
    if !scm::service_exists(&config.name)? {
        return Err(ManagerError::NotInstalled(config.name.clone()));
    }

    match config.service_type.normalized() {
        ServiceType::Nssm => {
            let nssm = Nssm::new(nssm_path, runner);
            nssm.apply(config)?;
        }
        ServiceType::Apache | ServiceType::PostgreSql => {
            native::apply(config, runner, &crate::process::sc_exe_path())?;
        }
        ServiceType::Unknown => {
            return Err(ManagerError::Other("Jenis service tidak dikenal.".into()))
        }
    }

    Ok(())
}

pub fn remove_service(
    config: &ServiceConfig,
    nssm_path: &Path,
    runner: &dyn CommandRunner,
) -> Result<(), ManagerError> {
    if !scm::service_exists(&config.name)? {
        return Ok(());
    }

    let options = crate::scm::WaitOptions {
        timeout: std::time::Duration::from_secs(30),
        ..crate::scm::WaitOptions::default()
    };
    let _ = scm::stop_service(&config.name, &options);

    match config.service_type.normalized() {
        ServiceType::Nssm => {
            let nssm = Nssm::new(nssm_path, runner);
            nssm.remove_ignore_errors(&config.name);
        }
        ServiceType::Apache | ServiceType::PostgreSql => {
            let _ = native::remove(config, runner);
        }
        ServiceType::Unknown => {}
    }

    if scm::service_exists(&config.name)? {
        scm::delete_service(&config.name)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::process::MockRunner;

    fn nssm_config() -> ServiceConfig {
        ServiceConfig {
            name: "demo".into(),
            executable: r"C:\definitely-missing\app.exe".into(),
            ..ServiceConfig::default()
        }
    }

    #[test]
    fn install_validates_paths_before_touching_windows() {
        let runner = MockRunner::default();
        let error = install_service(&nssm_config(), Path::new(r"C:\missing\nssm.exe"), &runner)
            .unwrap_err();
        assert!(matches!(error, ManagerError::Validation(_)));
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn apply_rejects_invalid_config_before_scm() {
        let mut config = nssm_config();
        config.name = "bad name/".into();
        let runner = MockRunner::default();
        let error = apply_service(&config, Path::new("nssm.exe"), &runner).unwrap_err();
        assert!(matches!(error, ManagerError::Validation(_)));
        assert!(runner.calls().is_empty());
    }

    #[test]
    fn validation_messages_are_flattened() {
        let error = ManagerError::Validation(vec![
            ValidationError::EmptyName,
            ValidationError::EmptyExecutable,
        ]);
        assert_eq!(error.messages().len(), 2);
    }
}
