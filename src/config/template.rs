use crate::consts::*;
use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct TemplateConfig {
    #[serde(default = "default_detail")]
    pub detail: String,
    #[serde(default = "default_state")]
    pub state: String,
}

impl Default for TemplateConfig {
    fn default() -> Self {
        Self {
            detail: default_detail(),
            state: default_state(),
        }
    }
}
