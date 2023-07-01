pub mod image_provider;
pub mod player;
pub mod template;
pub mod time;

pub use crate::config::{image_provider::*, player::*, template::*, time::*};

use crate::consts::*;

use dirs::config_local_dir;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Debug)]
pub struct Config {
    #[serde(default = "default_false")]
    pub allow_streaming: bool,
    #[serde(default = "default_true")]
    pub clear_on_pause: bool,
    #[serde(default = "default_false")]
    pub playing_first: bool,
    pub image_provider: ImageProviderConfig,
    pub player: HashMap<String, PlayerConfig>,
    pub template: TemplateConfig,
    #[serde(default = "default_time_config")]
    pub time: TimeConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            allow_streaming: default_false(),
            clear_on_pause: default_true(),
            playing_first: default_false(),
            image_provider: ImageProviderConfig::default(),
            player: HashMap::new(),
            template: TemplateConfig::default(),
            time: TimeConfig::default(),
        }
    }
}

impl Config {
    pub fn load() -> Self {
        toml::from_str(
            &std::fs::read_to_string(
                config_local_dir()
                    .unwrap()
                    .join(APP_NAME)
                    .join("config.toml"),
            )
            .unwrap(),
        )
        .unwrap()
    }
}
