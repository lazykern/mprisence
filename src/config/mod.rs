mod schema;

use figment::Figment;
pub use schema::{
    Config, CoverConfig, CoverProviderConfig, DefaultPlayerConfig, ImgbbConfig, PlayerConfig,
    PlayerSpecificConfig, TemplateConfig, TimeConfig,
};

const DEFAULT_CONFIG: &str = include_str!("../../config/default.toml");

pub fn load_config() -> Result<Config, figment::Error> {
    use figment::providers::{Format, Toml};

    let config_path = dirs::config_dir()
        .unwrap_or_default()
        .join("mprisence")
        .join("config.toml");

    Figment::new()
        .merge(Toml::string(DEFAULT_CONFIG))
        .merge(Toml::file(config_path))
        .extract()
}
