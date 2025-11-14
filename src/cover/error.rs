use std::path::PathBuf;
use thiserror::Error;
use url;

use crate::config;

#[derive(Debug, Error)]
pub enum CoverArtError {
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),

    #[error("Invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),

    #[error("Cache error: {0}")]
    Cache(std::io::Error),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("ImgBB error: {0}")]
    ImgBB(String),

    #[error("Provider error: {0}")]
    Provider(String),

    #[error("JSON error: {0}")]
    Json(String),

    #[error("File processing error: {0}")]
    FileProcessing(String),

    #[error("Other error: {0}")]
    Other(String),
}

impl CoverArtError {
    pub fn from_cache_io(err: std::io::Error) -> Self {
        Self::Cache(err)
    }

    pub fn from_imgbb_error<E: std::fmt::Display>(err: E) -> Self {
        CoverArtError::ImgBB(format!("API error: {}", err))
    }

    pub fn json_error<E: std::fmt::Display>(err: E) -> Self {
        Self::Json(err.to_string())
    }

    pub fn provider_error<S: AsRef<str>>(provider_name: S, message: S) -> Self {
        Self::Provider(format!("{}: {}", provider_name.as_ref(), message.as_ref()))
    }

    pub fn file_error<S: AsRef<str>>(path: &PathBuf, message: S) -> Self {
        CoverArtError::FileProcessing(format!("{:?}: {}", path, message.as_ref()))
    }

    pub fn other<S: AsRef<str>>(message: S) -> Self {
        CoverArtError::Other(message.as_ref().to_string())
    }
}
