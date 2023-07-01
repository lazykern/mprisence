use serde::Deserialize;

pub mod provider;

use provider::ImageProviderConfig;

use crate::config::default::*;

#[derive(Deserialize, Debug)]
pub struct ImageConfig {
    #[serde(default = "default_image_file_names")]
    pub file_names: Vec<String>,
    #[serde(default = "default_image_provider_config")]
    pub provider: ImageProviderConfig,
}

impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            file_names: default_image_file_names(),
            provider: ImageProviderConfig::default(),
        }
    }
}
