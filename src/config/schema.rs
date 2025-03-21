use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

use super::default::{get_default_config, RawTemplateConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_clear_on_pause")]
    pub clear_on_pause: bool,

    #[serde(default = "default_interval")]
    pub interval: u64,

    #[serde(default)]
    pub template: TemplateConfig,

    #[serde(default)]
    pub time: TimeConfig,

    #[serde(default)]
    pub cover: CoverConfig,

    #[serde(default)]
    pub player: HashMap<String, PlayerConfig>,
}

fn default_clear_on_pause() -> bool {
    get_default_config().clear_on_pause
}

fn default_interval() -> u64 {
    get_default_config().interval
}

impl Default for Config {
    fn default() -> Self {
        Config {
            clear_on_pause: default_clear_on_pause(),
            interval: default_interval(),
            template: TemplateConfig::default(),
            time: TimeConfig::default(),
            cover: CoverConfig::default(),
            player: HashMap::default(),
        }
    }
}

impl Config {
    pub fn get_player_config(&self, player_name: &str) -> &PlayerConfig {
        self.player
            .get(player_name)
            .or_else(|| self.player.get("default"))
            .expect("No default player config found")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateConfig {
    #[serde(default = "default_template_detail")]
    pub detail: String,

    #[serde(default = "default_template_state")]
    pub state: String,

    #[serde(default = "default_template_large_text")]
    pub large_text: String,

    #[serde(default = "default_template_small_text")]
    pub small_text: String,
}

fn default_template_detail() -> String {
    get_default_config().template.detail.clone()
}

fn default_template_state() -> String {
    get_default_config().template.state.clone()
}

fn default_template_large_text() -> String {
    get_default_config().template.large_text.clone()
}

fn default_template_small_text() -> String {
    get_default_config().template.small_text.clone()
}

impl Default for TemplateConfig {
    fn default() -> Self {
        TemplateConfig {
            detail: default_template_detail(),
            state: default_template_state(),
            large_text: default_template_large_text(),
            small_text: default_template_small_text(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeConfig {
    #[serde(default = "default_time_show")]
    pub show: bool,

    #[serde(default = "default_time_as_elapsed")]
    pub as_elapsed: bool,
}

fn default_time_show() -> bool {
    get_default_config().time.show
}

fn default_time_as_elapsed() -> bool {
    get_default_config().time.as_elapsed
}

impl Default for TimeConfig {
    fn default() -> Self {
        TimeConfig {
            show: default_time_show(),
            as_elapsed: default_time_as_elapsed(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverConfig {
    #[serde(default = "default_cover_file_names")]
    pub file_names: Vec<String>,

    #[serde(default)]
    pub provider: CoverProviderConfig,
}

fn default_cover_file_names() -> Vec<String> {
    get_default_config().cover.file_names.clone()
}

impl Default for CoverConfig {
    fn default() -> Self {
        CoverConfig {
            file_names: default_cover_file_names(),
            provider: CoverProviderConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverProviderConfig {
    #[serde(default = "default_cover_providers")]
    pub provider: Vec<String>,

    #[serde(default)]
    pub imgbb: Option<ImgBBConfig>,
}

fn default_cover_providers() -> Vec<String> {
    get_default_config().cover.provider.provider.clone()
}

impl Default for CoverProviderConfig {
    fn default() -> Self {
        CoverProviderConfig {
            provider: default_cover_providers(),
            imgbb: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImgBBConfig {
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerConfig {
    #[serde(default = "default_player_ignore")]
    pub ignore: bool,

    #[serde(default = "default_player_app_id")]
    pub app_id: String,

    #[serde(default = "default_player_icon")]
    pub icon: String,

    #[serde(default = "default_player_show_icon")]
    pub show_icon: bool,

    #[serde(default = "default_player_allow_streaming")]
    pub allow_streaming: bool,
}

fn default_player_ignore() -> bool {
    get_default_config()
        .player
        .get("default")
        .expect("Default player config missing")
        .ignore
        .expect("player.default.ignore missing in default player config")
}

fn default_player_app_id() -> String {
    get_default_config()
        .player
        .get("default")
        .expect("Default player config missing")
        .app_id
        .clone()
        .expect("player.default.app_id missing in default player config")
}

fn default_player_icon() -> String {
    get_default_config()
        .player
        .get("default")
        .expect("Default player config missing")
        .icon
        .clone()
        .expect("player.default.icon missing in default player config")
}

fn default_player_show_icon() -> bool {
    get_default_config()
        .player
        .get("default")
        .expect("Default player config missing")
        .show_icon
        .expect("player.default.show_icon missing in default player config")
}

fn default_player_allow_streaming() -> bool {
    get_default_config()
        .player
        .get("default")
        .expect("Default player config missing")
        .allow_streaming
        .expect("player.default.allow_streaming missing in default player config")
}

impl Default for PlayerConfig {
    fn default() -> Self {
        PlayerConfig {
            ignore: default_player_ignore(),
            app_id: default_player_app_id(),
            icon: default_player_icon(),
            show_icon: default_player_show_icon(),
            allow_streaming: default_player_allow_streaming(),
        }
    }
}
