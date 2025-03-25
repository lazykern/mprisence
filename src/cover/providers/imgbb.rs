use async_trait::async_trait;
use log::{debug, info, trace, warn};
use mpris::Metadata;
use std::sync::Arc;
use std::time::Duration;
use imgbb::ImgBB;

use crate::cover::error::CoverArtError;
use crate::cover::sources::ArtSource;
use crate::config::schema::ImgBBConfig;
use super::{CoverArtProvider, CoverResult};

pub struct ImgbbProvider {
    config: ImgBBConfig,
    client: Arc<ImgBB>,
}

impl ImgbbProvider {
    pub fn with_config(config: ImgBBConfig) -> Self {
        info!("Initializing ImgBB provider");
        let api_key = config.api_key.clone().expect("API key must be provided");
        Self { 
            client: Arc::new(ImgBB::new(api_key)),
            config,
        }
    }

    /// Generate a meaningful name for the image based on metadata
    fn generate_image_name(&self, metadata: &Metadata) -> String {
        let artist = metadata.artists()
            .and_then(|artists| artists.first().map(ToString::to_string))
            .unwrap_or_default();
        
        let title = metadata.title()
            .map(ToString::to_string)
            .unwrap_or_default();
        
        if artist.is_empty() && title.is_empty() {
            "mprisence_cover".to_string()
        } else if artist.is_empty() {
            title
        } else if title.is_empty() {
            artist
        } else {
            format!("{} - {}", artist, title)
        }
    }
}

#[async_trait]
impl CoverArtProvider for ImgbbProvider {
    fn name(&self) -> &'static str {
        "imgbb"
    }
    
    fn supports_source_type(&self, source: &ArtSource) -> bool {
        matches!(source, 
            ArtSource::Base64(_) | 
            ArtSource::File(_) |
            ArtSource::Bytes(_)
        )
    }
    
    async fn process(&self, source: ArtSource, metadata: &Metadata) -> Result<Option<CoverResult>, CoverArtError> {
        if self.config.api_key.is_none() {
            warn!("ImgBB provider is disabled (no API key configured)");
            return Ok(None);
        }
        
        debug!("Processing cover art with ImgBB provider");
        let image_name = self.generate_image_name(metadata);
        
        let mut builder = self.client.upload_builder().name(&image_name);
        
        if self.config.expiration > 0 {
            trace!("Setting image expiration to {} seconds", self.config.expiration);
            builder = builder.expiration(self.config.expiration);
        }

        let response = match source {
            ArtSource::Base64(data) => builder.data(&data),
            ArtSource::Bytes(data) => builder.bytes(&data),
            ArtSource::File(path) => {
                let data = tokio::fs::read(&path).await.map_err(|e| {
                    CoverArtError::provider_error("imgbb", &format!("Failed to read file: {}", e))
                })?;
                builder.bytes(&data)
            },
            ArtSource::Url(_) => return Ok(None),
        }.upload().await.map_err(|e| {
            CoverArtError::provider_error("imgbb", &format!("Upload failed: {}", e))
        })?;

        let url = response.data
            .and_then(|data| data.url.or(data.display_url));

        match &url {
            Some(url) => {
                info!("Successfully uploaded image to ImgBB");
                trace!("ImgBB provided URL: {}", url);
            },
            None => warn!("ImgBB upload succeeded but no URL was returned"),
        }

        let expiration = if self.config.expiration > 0 {
            Some(Duration::from_secs(self.config.expiration as u64))
        } else {
            None
        };

        Ok(url.map(|url| CoverResult {
            url,
            provider: self.name().to_string(),
            expiration,
        }))
    }
}
