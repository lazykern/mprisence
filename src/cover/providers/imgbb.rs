use async_trait::async_trait;
use log::debug;
use mpris::Metadata;

use crate::cover::{error::CoverArtError, CoverArtSource, LocalUtils};

use super::CoverArtProvider;

#[derive(Clone)]
pub struct ImgbbProvider {
    api_key: String,
    local_utils: LocalUtils,
}

impl ImgbbProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            local_utils: LocalUtils::new(vec![
                "cover".to_string(),
                "folder".to_string(),
                "album".to_string(),
            ]),
        }
    }
}

#[async_trait]
impl CoverArtProvider for ImgbbProvider {
    fn name(&self) -> &'static str {
        "imgbb"
    }

    async fn get_cover_url(&self, metadata: &Metadata) -> Result<Option<String>, CoverArtError> {
        // If API key is empty, provider is disabled
        if self.api_key.is_empty() {
            debug!("ImgBB provider is disabled (no API key configured)");
            return Ok(None);
        }

        // Try to find a local file using LocalUtils
        if let Some(source) = self.local_utils.find_cover_art(metadata).await? {
            match source {
                CoverArtSource::LocalFile(path) => {
                    debug!("ImgBB upload not implemented yet for: {:?}", path);
                    Ok(None)
                }
                CoverArtSource::HttpUrl(url) => Ok(Some(url)),
            }
        } else {
            Ok(None)
        }
    }
}
