use thiserror::Error;
use std::path::PathBuf;
use url;

use crate::config;

/// Error type for cover art related operations
#[derive(Debug, Error)]
pub enum CoverArtError {
    /// IO error
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
    
    /// Invalid URL
    #[error("Invalid URL: {0}")]
    InvalidUrl(#[from] url::ParseError),
    
    /// Cache error 
    #[error("Cache error: {0}")]
    Cache(std::io::Error),
    
    /// MusicBrainz API error
    #[error("MusicBrainz error: {0}")]
    MusicBrainz(#[from] musicbrainz_rs::Error),
    
    /// Config error
    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),
    
    /// ImgBB API error
    #[error("ImgBB error: {0}")]
    ImgBB(String),
    
    /// Provider error
    #[error("Provider error: {0}")]
    Provider(String),
    
    /// JSON serialization error
    #[error("JSON error: {0}")]
    Json(String),
    
    /// File processing error
    #[error("File processing error: {0}")]
    FileProcessing(String),
    
    /// Other error
    #[error("Other error: {0}")]
    Other(String),
}

// Helper methods to create specialized errors
impl CoverArtError {
    /// Convert an IO error to a Cache error variant
    pub fn from_cache_io(err: std::io::Error) -> Self {
        Self::Cache(err)
    }
    
    /// Convert an ImgBB error to ImgBB error variant with context
    pub fn from_imgbb_error<E: std::fmt::Display>(err: E) -> Self {
        CoverArtError::ImgBB(format!("API error: {}", err))
    }
    
    /// Create a JSON error variant
    pub fn json_error<E: std::fmt::Display>(err: E) -> Self {
        Self::Json(err.to_string())
    }
    
    /// Create a provider-specific error with provider name
    pub fn provider_error<S: AsRef<str>>(provider_name: S, message: S) -> Self {
        Self::Provider(format!("{}: {}", provider_name.as_ref(), message.as_ref()))
    }
    
    /// Create a File processing error
    pub fn file_error<S: AsRef<str>>(path: &PathBuf, message: S) -> Self {
        CoverArtError::FileProcessing(format!("{:?}: {}", path, message.as_ref()))
    }
    
    /// Create an Other error with custom message
    pub fn other<S: AsRef<str>>(message: S) -> Self {
        CoverArtError::Other(message.as_ref().to_string())
    }
}