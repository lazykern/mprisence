use crate::consts::*;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct TimeConfig {
    #[serde(default = "default_true")]
    pub show: bool,
    #[serde(default = "default_false")]
    pub as_elapsed: bool,
}

impl Default for TimeConfig {
    fn default() -> Self {
        Self {
            show: default_true(),
            as_elapsed: default_false(),
        }
    }
}

pub fn default_time_config() -> TimeConfig {
    TimeConfig::default()
}
