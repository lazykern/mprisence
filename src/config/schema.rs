use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::utils::to_snake_case;

use super::default::{get_default_config};

mod snake_case_string {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use crate::utils::to_snake_case;
    use std::collections::HashMap;

    pub fn serialize<S>(map: &HashMap<String, super::PlayerConfig>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        map.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<String, super::PlayerConfig>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let map = HashMap::<String, super::PlayerConfig>::deserialize(deserializer)?;
        Ok(map.into_iter()
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
    pub fn get_player_config(&self, identity: &str) -> &PlayerConfig {
        let normalized = Self::normalize_player_name(identity);
        self.get_player_config_normalized(&normalized)
    }

    /// Get player config by pre-normalized identity
    pub fn get_player_config_normalized(&self, normalized_identity: &str) -> &PlayerConfig {
        self.player
            .get(normalized_identity)
            .or_else(|| self.player.get("default"))
            .expect("No default player config found")
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
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
            override_activity_type: None,
        }
    }
}

impl PlayerConfig {
    pub fn activity_type(&self, content_type: Option<&str>) -> ActivityType {
        // First check if there's an override specifically for this player
        if let Some(override_type) = &self.override_activity_type {
            return override_type.clone();
        }
        
        // If there's no override and content type detection is enabled,
        // determine based on content type
        let config = get_default_config();
        if config.activity_type.use_content_type {
            if let Some(content) = content_type {
                let media_type: Option<String> = content.split('/').next().map(|s| s.to_lowercase());
                
                // Check config for activity type based on media type
                if let Some(media_type) = media_type {
                    if media_type == "audio" {
                        return ActivityType::Listening;
                    } else if media_type == "video" {
                        return ActivityType::Watching;
                    } else if media_type == "image" {
                        return ActivityType::Watching;
                    }
                }
            }
        }
        
        // Fallback to default
        config.activity_type.default.clone()
    }
}
