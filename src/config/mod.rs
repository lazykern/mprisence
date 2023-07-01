pub mod default;
pub mod image;
pub mod player;
pub mod template;
pub mod time;

pub use crate::config::{image::*, player::*, template::*, time::*};

use crate::config::default::*;
use crate::consts::*;

use dirs::config_local_dir;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Debug)]
pub struct Config {
    #[serde(default = "default_false")]
    pub show_icon: bool,
    #[serde(default = "default_false")]
    pub allow_streaming: bool,
    #[serde(default = "default_true")]
    pub clear_on_pause: bool,
    #[serde(default = "default_image_config")]
    pub image: ImageConfig,
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
            show_icon: default_false(),
            allow_streaming: default_false(),
            clear_on_pause: default_true(),
            image: ImageConfig::default(),
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
