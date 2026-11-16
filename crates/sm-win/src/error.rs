use std::time::Duration;

use thiserror::Error;

use crate::scm::ServiceState;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScmError {
    #[error("Service '{0}' was not found.")]
    NotFound(String),
    #[error("Service '{0}' did not respond within {1:?} (last state: {2}).")]
    Timeout(String, Duration, ServiceState),
    #[error("Operation cancelled.")]
    Cancelled,
    #[error("{0}")]
    Windows(String),
    #[error("{0}")]
    Other(String),
}
