use crate::config::default::*;
use serde::Deserialize;

use super::CONFIG;

lazy_static::lazy_static! {
    pub static ref DEFAULT_PLAYER_CONFIG: PlayerConfig = match CONFIG.player.get("default") {
        Some(config) => {
            let mut conf = config.clone();
            if conf.app_id.is_none() {
                conf.app_id = Some(default_app_id());
            }
            if conf.icon.is_none() {
                conf.icon = Some(default_icon());
            }
            conf
        }
        None => PlayerConfig::default(),
    };
}

#[derive(Deserialize, Debug, Hash, Clone)]
pub struct PlayerConfig {
    pub app_id: Option<String>,
    pub icon: Option<String>,
    #[serde(default = "default_false")]
    pub ignore: bool,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            app_id: Some(default_app_id()),
            icon: Some(default_icon()),
            ignore: default_false(),
        }
    }
}

impl PlayerConfig {
    pub fn app_id_or_default(&self) -> &str {
        match &self.app_id {
            Some(app_id) => app_id,
            None => &DEFAULT_PLAYER_CONFIG.app_id.as_ref().unwrap(),
        }
    }

    pub fn icon_or_default(&self) -> &str {
        match &self.icon {
            Some(icon) => icon,
            None => &DEFAULT_PLAYER_CONFIG.icon.as_ref().unwrap(),
        }
    }

    pub fn get_or_default(identity: &str) -> &Self {
        match CONFIG.player.get(identity) {
            Some(config) => config,
            None => &DEFAULT_PLAYER_CONFIG,
        }
    }
}
