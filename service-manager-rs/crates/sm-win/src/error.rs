use std::time::Duration;

use thiserror::Error;

use crate::scm::ServiceState;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScmError {
    #[error("Service '{0}' tidak ditemukan.")]
    NotFound(String),
    #[error("Service '{0}' tidak merespons dalam {1:?} (status akhir: {2}).")]
    Timeout(String, Duration, ServiceState),
    #[error("Operasi dibatalkan.")]
    Cancelled,
    #[error("{0}")]
    Windows(String),
    #[error("{0}")]
    Other(String),
}
