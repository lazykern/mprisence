use crate::config::default::*;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct TemplateConfig {
    #[serde(default = "default_detail_template")]
    pub detail: String,
    #[serde(default = "default_state_template")]
    pub state: String,
    #[serde(default = "default_large_text_template")]
    pub large_text: String,
    #[serde(default = "default_small_text_template")]
    pub small_text: String,
    #[serde(default = "default_large_text_no_album_image_template")]
    pub large_text_no_album_image: String,
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            detail: default_detail_template(),
            state: default_state_template(),
            large_text: default_large_text_template(),
            small_text: default_small_text_template(),
            large_text_no_album_image: default_large_text_no_album_image_template(),
        }
    }
}
