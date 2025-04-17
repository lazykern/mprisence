use crate::config;
use crate::cover;
use mpris::{DBusError, FindingError};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Mprisence error: {0}")]
    Mprisence(#[from] MprisenceError),

    #[error("CLI error: {0}")]
    Cli(#[from] clap::Error),

    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),

    #[error("DBus error: {0}")]
    DBus(#[from] DBusError),

    #[error("Player finding error: {0}")]
    PlayerFinding(#[from] FindingError),
}

#[derive(Error, Debug)]
pub enum MprisenceError {
    #[error("Failed to initialize template: {0}")]
    Template(#[from] TemplateError),

    #[error("Failed to access config: {0}")]
    Config(#[from] config::ConfigError),

    #[error("Failed to initialize cover art: {0}")]
    CoverArt(#[from] cover::error::CoverArtError),

    #[error("Failed to create player finder")]
    DBus(#[from] DBusError),

    #[error("Discord error: {0}")]
    Discord(#[from] DiscordError),

    #[error("Player finding error: {0}")]
    PlayerFinding(#[from] FindingError),
}

#[derive(Error, Debug)]
pub enum DiscordError {
    #[error("Failed to connect to Discord: {0}")]
    ConnectionError(String),

    #[error("Failed to close Discord: {0}")]
    CloseError(String),

    #[error("Failed to set activity: {0}")]
    ActivityError(String),

    #[error("Failed to reconnect: {0}")]
    ReconnectionError(String),

    #[error("Invalid player: {0}")]
    InvalidPlayer(String),

    #[error("Template error: {0}")]
    Template(#[from] TemplateError),
}

#[derive(Error, Debug)]
pub enum TemplateError {
    #[error("Template render error: {0}")]
    HandlebarsRender(#[from] handlebars::RenderError),

    #[error("Template error: {0}")]
    HandlebarsTemplate(#[from] handlebars::TemplateError),
}