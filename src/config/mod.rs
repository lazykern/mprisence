pub mod image_provider;
pub mod player;
pub mod template;

pub use crate::config::{image_provider::*, player::*, template::*};

use crate::consts::*;

use dirs::config_local_dir;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Deserialize, Debug)]
pub struct Config {
    #[serde(default = "default_false")]
    pub allow_streaming: bool,
    pub player: HashMap<String, PlayerConfig>,
    pub template: TemplateConfig,
    pub image_provider: ImageProviderConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            allow_streaming: default_false(),
            player: HashMap::new(),
            template: TemplateConfig::default(),
            image_provider: ImageProviderConfig::default(),
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
