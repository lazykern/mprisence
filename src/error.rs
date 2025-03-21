use crate::config;
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
}

#[derive(Error, Debug)]
pub enum ServiceInitError {
    #[error("Failed to initialize player: {0}")]
    Player(#[from] PlayerError),

    #[error("Failed to initialize template: {0}")]
    Template(#[from] TemplateError),

    #[error("Failed to access config: {0}")]
    Config(#[from] config::ConfigError),
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
    DBus(#[from] mpris::DBusError),
    #[error("Finding error: {0}")]
    Finding(#[from] mpris::FindingError),
    #[error("Config access error: {0}")]
    Config(#[from] config::ConfigError),
}

#[derive(Error, Debug)]
pub enum PresenceError {
    #[error("Failed to connect to Discord: {0}")]
    Connection(String),
    #[error("Failed to update presence: {0}")]
    Update(String),
    #[error("Config access error: {0}")]
    Config(#[from] config::ConfigError),
}

#[derive(Error, Debug)]
pub enum TemplateError {
    #[error("Template initialization error: {0}")]
    Init(String),
    #[error("Template render error: {0}")]
    Render(#[from] handlebars::RenderError),
}
