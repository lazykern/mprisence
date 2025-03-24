use crate::config;
use crate::cover;
use mpris::DBusError;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Service error: {0}")]
    Service(#[from] ServiceError),

    #[error("CLI error: {0}")]
    Cli(#[from] clap::Error),

    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
}

#[derive(Error, Debug)]
pub enum ServiceError {
    #[error("Failed to initialize template: {0}")]
    Template(#[from] TemplateError),

    #[error("Failed to access config: {0}")]
    Config(#[from] config::ConfigError),

    #[error("Failed to initialize cover art: {0}")]
    CoverArt(#[from] cover::error::CoverArtError),

    #[error("Failed to create player finder")]
    DBus(#[from] DBusError),
}

#[derive(Error, Debug)]
pub enum MprisenceError {
    #[error("Invalid player: {0}")]
    InvalidPlayer(String),

    #[error("Discord error: {0}")]
    Discord(String),

    #[error("Failed to create player finder")]
    DBus(#[from] DBusError),

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