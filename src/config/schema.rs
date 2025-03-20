use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub clear_on_pause: bool,

    #[serde(default)]
    pub interval: u64,

    #[serde(default)]
    pub template: TemplateConfig,

    #[serde(default)]
    pub time: TimeConfig,

    #[serde(default)]
    pub cover: CoverConfig,

    #[serde(default)]
    pub player: PlayerConfig,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TemplateConfig {
    #[serde(default)]
    pub detail: String,

    #[serde(default)]
    pub state: String,

    #[serde(default)]
    pub large_text: String,

    #[serde(default)]
    pub small_text: String,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TimeConfig {
    #[serde(default)]
    pub show: bool,

    #[serde(default)]
    pub as_elapsed: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CoverConfig {
    #[serde(default)]
    pub file_names: Vec<String>,

    #[serde(default)]
    pub provider: CoverProviderConfig,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct CoverProviderConfig {
    #[serde(default)]
    pub provider: Vec<String>,

    #[serde(default)]
    pub imgbb: ImgbbConfig,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ImgbbConfig {
    pub api_key: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct PlayerConfig {
    #[serde(default)]
    pub default: DefaultPlayerConfig,

    #[serde(flatten)]
    pub players: HashMap<String, PlayerSpecificConfig>,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct DefaultPlayerConfig {
    #[serde(default)]
    pub ignore: bool,
    pub app_id: Option<String>,
    #[serde(default)]
    pub show_icon: bool,
    #[serde(default)]
    pub allow_streaming: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct PlayerSpecificConfig {
    pub ignore: Option<bool>,
    pub app_id: Option<String>,
    pub show_icon: Option<bool>,
    pub allow_streaming: Option<bool>,
    pub icon: Option<String>,
}
