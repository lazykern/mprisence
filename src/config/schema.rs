use log::warn;
use mime_guess::{mime, Mime};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::utils::to_snake_case;

use super::default::get_default_config;

mod snake_case_string {
    use crate::utils::to_snake_case;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::HashMap;

    pub fn serialize<S>(
        map: &HashMap<String, super::PlayerConfig>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        map.serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<HashMap<String, super::PlayerConfig>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let map = HashMap::<String, super::PlayerConfig>::deserialize(deserializer)?;
        Ok(map
            .into_iter()
            .map(|(k, v)| (to_snake_case(&k), v))
            .collect())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityTypesConfig {
    #[serde(default = "default_use_content_type")]
    pub use_content_type: bool,

    #[serde(default = "default_activity_type")]
    pub default: ActivityType,
}

fn default_use_content_type() -> bool {
    get_default_config().activity_type.use_content_type
}

fn default_activity_type() -> ActivityType {
    get_default_config().activity_type.default.clone()
}

impl Default for ActivityTypesConfig {
    fn default() -> Self {
        Self {
            use_content_type: default_use_content_type(),
            default: default_activity_type(),
        }
    }
}

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
    pub activity_type: ActivityTypesConfig,

    #[serde(default)]
    #[serde(with = "snake_case_string")]
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
            activity_type: ActivityTypesConfig::default(),
            player: HashMap::default(),
        }
    }
}

impl Config {
    /// Get the normalized (snake_case) name for a player identity
    pub fn normalize_player_name(identity: &str) -> String {
        to_snake_case(identity)
    }

    /// Get player config by raw identity (will be normalized internally)
    pub fn get_player_config(&self, identity: &str) -> PlayerConfig {
        let normalized = Self::normalize_player_name(identity);
        self.get_player_config_normalized(&normalized)
    }

    /// Get player config by pre-normalized identity
    pub fn get_player_config_normalized(&self, normalized_identity: &str) -> PlayerConfig {
        // First try to get the specific player config
        if let Some(config) = self.player.get(normalized_identity) {
            return config.clone();
        }

        // If not found, try to get the default config
        self.player.get("default").cloned().unwrap_or_else(|| {
            // If no default config exists, create a new one
            warn!("No default player config found, using built-in defaults");
            PlayerConfig::default()
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateConfig {
    #[serde(default = "default_template_detail")]
    pub detail: Box<str>,

    #[serde(default = "default_template_state")]
    pub state: Box<str>,

    #[serde(default = "default_template_large_text")]
    pub large_text: Box<str>,

    #[serde(default = "default_template_small_text")]
    pub small_text: Box<str>,

    #[serde(default = "default_template_unknown_text")]
    pub unknown_text: Box<str>,
}

fn default_template_detail() -> Box<str> {
    get_default_config()
        .template
        .detail
        .clone()
        .into_boxed_str()
}

fn default_template_state() -> Box<str> {
    get_default_config().template.state.clone().into_boxed_str()
}

fn default_template_large_text() -> Box<str> {
    get_default_config()
        .template
        .large_text
        .clone()
        .into_boxed_str()
}

fn default_template_small_text() -> Box<str> {
    get_default_config()
        .template
        .small_text
        .clone()
        .into_boxed_str()
}

fn default_template_unknown_text() -> Box<str> {
    get_default_config()
        .template
        .unknown_text
        .clone()
        .into_boxed_str()
}

impl Default for TemplateConfig {
    fn default() -> Self {
        TemplateConfig {
            detail: default_template_detail(),
            state: default_template_state(),
            large_text: default_template_large_text(),
            small_text: default_template_small_text(),
            unknown_text: default_template_unknown_text(),
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
    // API key for ImgBB service
    pub api_key: Option<String>,

    // Optional expiration time in seconds (0 = no expiration)
    #[serde(default)]
    pub expiration: Option<u64>,

    // Optional default name for uploaded images
    #[serde(default)]
    pub default_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Copy)]
#[serde(rename_all = "lowercase")]
pub enum ActivityType {
    Listening,
    Watching,
    Playing,
    Competing,
}

impl Default for ActivityType {
    fn default() -> Self {
        ActivityType::Listening
    }
}

impl From<ActivityType> for discord_rich_presence::activity::ActivityType {
    fn from(activity_type: ActivityType) -> Self {
        match activity_type {
            ActivityType::Listening => Self::Listening,
            ActivityType::Watching => Self::Watching,
            ActivityType::Playing => Self::Playing,
            ActivityType::Competing => Self::Competing,
        }
    }
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

    #[serde(default)]
    pub override_activity_type: Option<ActivityType>,
}

fn default_player_ignore() -> bool {
    get_default_config()
        .player
        .get("default")
        .expect("Failed to get default player config")
        .ignore
        .expect("Failed to get default player ignore")
}

fn default_player_app_id() -> String {
    get_default_config()
        .player
        .get("default")
        .expect("Failed to get default player config")
        .app_id
        .clone()
        .expect("Failed to get default player app_id")
}

fn default_player_icon() -> String {
    get_default_config()
        .player
        .get("default")
        .expect("Failed to get default player config")
        .icon
        .clone()
        .expect("Failed to get default player icon")
}

fn default_player_show_icon() -> bool {
    get_default_config()
        .player
        .get("default")
        .expect("Failed to get default player config")
        .show_icon
        .expect("Failed to get default player show_icon")
}

fn default_player_allow_streaming() -> bool {
    get_default_config()
        .player
        .get("default")
        .expect("Failed to get default player config")
        .allow_streaming
        .expect("Failed to get default player allow_streaming")
}

impl Default for PlayerConfig {
    fn default() -> PlayerConfig {
        PlayerConfig {
            ignore: default_player_ignore(),
            app_id: default_player_app_id(),
            icon: default_player_icon(),
            show_icon: default_player_show_icon(),
            allow_streaming: default_player_allow_streaming(),
            override_activity_type: None,
        }
    }
}
