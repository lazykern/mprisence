pub mod default;
pub mod cover;
pub mod player;
pub mod template;
pub mod time;

pub use crate::config::{cover::*, player::*, template::*, time::*};

use crate::config::default::*;
use crate::consts::*;

use dirs::config_local_dir;
use figment::{
    providers::{Format, Toml},
    Figment,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

lazy_static::lazy_static! {
    pub static ref CONFIG: Config = Config::load();
}

#[derive(Deserialize, Debug)]
pub struct Config {
    #[serde(default = "default_true")]
    pub clear_on_pause: bool,
    #[serde(default = "default_interval")]
    pub interval: u64,
    #[serde(default = "default_cover_config")]
    pub cover: CoverConfig,
    #[serde(default = "default_player_hashmap_config")]
    pub player: HashMap<String, PlayerConfig>,
    #[serde(default = "default_template_config")]
    pub template: TemplateConfig,
    #[serde(default = "default_time_config")]
    pub time: TimeConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            clear_on_pause: default_true(),
            interval: default_interval(),
            cover: CoverConfig::default(),
            player: HashMap::new(),
            template: TemplateConfig::default(),
            time: TimeConfig::default(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        let default_config_str = include_str!("../../config/default.toml");

        let fig = Figment::new().merge(Toml::string(default_config_str));

        let config: Self = if let Some(config_path) = get_config_path() {
            let fig_m = fig.clone().merge(Toml::file(config_path));
            match fig_m.extract() {
                Ok(config) => config,
                Err(e) => {
                    log::error!("Error loading config, using default: {}", e);
                    fig.extract().unwrap_or_default()
                }
            }
        } else {
            fig.extract().unwrap_or_default()
        };

        config
    }
}

fn get_config_path() -> Option<PathBuf> {
    if let Some(config_path) = config_local_dir().map(|dir| dir.join(APP_NAME).join("config.toml"))
    {
        match config_path.exists() {
            true => return Some(config_path),
            false => return None,
        }
    }
    None
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum StringOrStringVec {
    String(String),
    Vec(Vec<String>)
}
