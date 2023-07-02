use crate::config::default::*;
use serde::Deserialize;

#[derive(Deserialize, Debug, Hash)]
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
