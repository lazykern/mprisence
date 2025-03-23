use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
use super::error::ConfigError;
use super::schema::ActivityType;

// Static cell to hold the default config after first load
static DEFAULT_CONFIG: OnceLock<RawConfig> = OnceLock::new();

// Helper function to get default config
pub(crate) fn get_default_config() -> &'static RawConfig {
    DEFAULT_CONFIG.get_or_init(|| {
        let default_config = include_str!("../../config/default.toml");
        toml::from_str(default_config)
            .map_err(|e| ConfigError::Deserialize(e))
            .expect("Failed to parse default configuration")
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawConfig {
    pub clear_on_pause: bool,
    pub interval: u64,
    pub template: RawTemplateConfig,
    pub time: RawTimeConfig,
    pub cover: RawCoverConfig,
    pub activity_type: RawActivityTypeConfig,
    pub player: HashMap<String, RawPlayerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawTemplateConfig {
    pub detail: String,
    pub state: String,
    pub large_text: String,
    pub small_text: String,
    pub unknown_text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawTimeConfig {
    pub show: bool,
    pub as_elapsed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawCoverConfig {
    pub file_names: Vec<String>,
    pub provider: CoverProviderConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverProviderConfig {
    pub provider: Vec<String>,
    pub imgbb: Option<ImgBBConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ImgBBConfig {
    pub api_key: Option<String>,
    pub expiration: Option<u64>,
    pub default_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawActivityTypeConfig {
    pub use_content_type: bool,
    pub default: ActivityType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawPlayerConfig {
    pub ignore: Option<bool>,
    pub app_id: Option<String>,
    pub icon: Option<String>,
    pub show_icon: Option<bool>,
    pub allow_streaming: Option<bool>,
    pub override_activity_type: Option<ActivityType>,
}
