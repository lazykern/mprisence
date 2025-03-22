use thiserror::Error;

use crate::config;

#[derive(Error, Debug)]
pub enum CoverArtError {
    #[error("Provider error: {0}")]
    Provider(String),

    #[error("No valid cover art found")]
    NotFound,

    #[error("Cache error: {0}")]
    Cache(#[from] std::io::Error),

    #[error("URL parse error: {0}")]
    Url(#[from] url::ParseError),

    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("MusicBrainz error: {0}")]
    MusicBrainz(#[from] musicbrainz_rs::Error),
}
