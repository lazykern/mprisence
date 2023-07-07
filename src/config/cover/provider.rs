use crate::config::default::*;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct CoverProviderConfig {
    #[serde(default = "default_cover_provider")]
    pub provider: String,
    #[serde(default = "default_imagebb_config")]
    pub imgbb: ImgBBConfig,
}

impl Default for CoverProviderConfig {
    fn default() -> Self {
        Self {
            provider: default_cover_provider(),
            imgbb: ImgBBConfig::default(),
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct ImgBBConfig {
    pub api_key: Option<String>,
}

impl Default for ImgBBConfig {
    fn default() -> Self {
        Self { api_key: None }
    }
}
