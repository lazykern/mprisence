use serde::Deserialize;

pub mod provider;

pub use provider::CoverProviderConfig;

use crate::config::default::*;

#[derive(Deserialize, Debug)]
pub struct CoverConfig {
    #[serde(default = "default_cover_file_names")]
    pub file_names: Vec<String>,
    #[serde(default = "default_cover_provider_config")]
    pub provider: CoverProviderConfig,
}

impl Default for CoverConfig {
    fn default() -> Self {
        Self {
            file_names: default_cover_file_names(),
            provider: CoverProviderConfig::default(),
        }
    }
}
