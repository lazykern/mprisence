use serde::Deserialize;

#[derive(Deserialize, Debug)]
pub struct ImageProviderConfig {
    pub imgbb: ImgBBConfig,
}

impl Default for ImageProviderConfig {
    fn default() -> Self {
        Self {
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
