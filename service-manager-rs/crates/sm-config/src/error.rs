use thiserror::Error;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Gagal membaca/menulis file: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON tidak valid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Invalid(String),
}
