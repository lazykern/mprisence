use crate::config::default::*;
use serde::Deserialize;

#[derive(Deserialize, Debug, Hash)]
pub struct PlayerConfig {
    #[serde(default = "default_app_id")]
    pub app_id: String,
    #[serde(default = "default_icon")]
    pub icon: String,
    #[serde(default = "default_false")]
    pub ignore: bool,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            app_id: default_app_id(),
            icon: default_icon(),
            ignore: default_false(),
        }
    }
}

