use crate::config::default::*;
use serde::Deserialize;

#[derive(Deserialize, Debug, Hash)]
pub struct PlayerConfig {
    #[serde(default = "default_i8max")]
    pub i: i8,
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
            i: default_i8max(),
            app_id: default_app_id(),
            icon: default_icon(),
            ignore: default_false(),
        }
    }
}

impl PartialEq for PlayerConfig {
    fn eq(&self, other: &Self) -> bool {
        self.i == other.i
    }
}

impl Eq for PlayerConfig {}

impl Ord for PlayerConfig {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.i.cmp(&other.i)
    }
}

impl PartialOrd for PlayerConfig {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
