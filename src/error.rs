use crate::config;
use crate::cover;
use mpris::{DBusError, FindingError};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Service initialization error: {0}")]
    ServiceInit(#[from] ServiceInitError),

    #[error("Service runtime error: {0}")]
    ServiceRuntime(#[from] ServiceRuntimeError),

    #[error("CLI error: {0}")]
    Cli(#[from] clap::Error),

    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum ServiceInitError {
    #[error("Failed to initialize player: {0}")]
    Player(#[from] PlayerError),

    #[error("Failed to initialize template: {0}")]
    Template(#[from] TemplateError),

    #[error("Failed to access config: {0}")]
    Config(#[from] config::ConfigError),

    #[error("Failed to initialize cover art: {0}")]
    CoverArt(#[from] cover::error::CoverArtError),

    #[error("Presence error: {0}")]
    Presence(#[from] PresenceError),
}

#[derive(Error, Debug)]
pub enum ServiceRuntimeError {
    #[error("Player error: {0}")]
    Player(#[from] PlayerError),

    #[error("Presence error: {0}")]
    Presence(#[from] PresenceError),

    #[error("Template error: {0}")]
    Template(#[from] TemplateError),

    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),
}

#[derive(Error, Debug)]
pub enum PlayerError {
    #[error("DBus error: {0}")]
    DBus(#[from] DBusError),

    #[error("Finding error: {0}")]
    Finding(#[from] FindingError),

    #[error("Config access error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("General error: {0}")]
    General(String),
}

#[derive(Error, Debug)]
pub enum PresenceError {
    #[error("Discord error: {0}")]
    Discord(#[from] discord_presence::DiscordError),

    #[error("Failed to update presence: {0}")]
    Update(String),

    #[error("Config access error: {0}")]
    Config(#[from] config::ConfigError),
}

#[derive(Error, Debug)]
pub enum TemplateError {
    #[error("Template render error: {0}")]
    Render(#[from] handlebars::RenderError),

    #[error("Template error: {0}")]
    Template(#[from] handlebars::TemplateError),
}

// Provide some convenience conversions for common error situations
impl From<String> for PlayerError {
    fn from(msg: String) -> Self {
        PlayerError::General(msg)
    }
}

impl From<&str> for PlayerError {
    fn from(msg: &str) -> Self {
        PlayerError::General(msg.to_string())
    }
}
