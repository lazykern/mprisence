use std::collections::HashMap;

use crate::consts::*;

use super::{
    image::provider::{ImageProviderConfig, ImgBBConfig},
    time::TimeConfig,
    ImageConfig, PlayerConfig, TemplateConfig,
};

pub fn default_app_id() -> String {
    DEFAULT_APP_ID.to_string()
}

pub fn default_icon() -> String {
    DEFAULT_ICON.to_string()
}

pub fn default_detail() -> String {
    DEFAULT_DETAIL.to_string()
}
pub const DEFAULT_STATE: &str = "{{{album}}}";

pub fn default_state() -> String {
    DEFAULT_STATE.to_string()
}

pub fn default_i8max() -> i8 {
    i8::MAX
}

pub fn default_false() -> bool {
    false
}

pub fn default_true() -> bool {
    true
}

pub fn default_image_file_names() -> Vec<String> {
    DEFAULT_IMAGE_FILE_NAMES
        .iter()
        .map(|s| s.to_string())
        .collect()
}

pub fn default_image_provider() -> String {
    DEFAULT_IMAGE_PROVIDER.to_string()
}

pub fn default_image_config() -> ImageConfig {
    ImageConfig::default()
}

pub fn default_player_hashmap_config() -> HashMap<String, PlayerConfig> {
    HashMap::new()
}

pub fn default_template_config() -> TemplateConfig {
    TemplateConfig::default()
}

pub fn default_time_config() -> TimeConfig {
    TimeConfig::default()
}

pub fn default_imagebb_config() -> ImgBBConfig {
    ImgBBConfig::default()
}

pub fn default_image_provider_config() -> ImageProviderConfig {
    ImageProviderConfig::default()
}
