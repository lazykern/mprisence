use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Config already initialized")]
    AlreadyInitialized,

    #[error("Failed to deserialize config: {0}")]
    Deserialize(#[from] toml::de::Error),

    #[error("Failed to serialize config: {0}")]
    Serialize(#[from] toml::ser::Error),

    #[error("Figment error: {0}")]
    Figment(Box<figment::Error>),

    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),

    #[error("Lock error: {0}")]
    Lock(String),
}

impl<T> From<std::sync::PoisonError<T>> for ConfigError {
    fn from(err: std::sync::PoisonError<T>) -> Self {
        ConfigError::Lock(err.to_string())
    }
}

impl From<figment::Error> for ConfigError {
    fn from(err: figment::Error) -> Self {
        Self::Figment(Box::new(err))
    }
}
