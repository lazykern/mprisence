use async_trait::async_trait;
use log::{debug, info, trace, warn, error};
use mpris::Metadata;
use std::{borrow::Cow, time::Duration, sync::Arc};
use image::{GenericImageView, ExtendedColorType};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use imgbb::ImgBB;

use crate::cover::error::CoverArtError;
use crate::cover::sources::ArtSource as ExternalArtSource;
use crate::config::schema::ImgBBConfig;
use super::{CoverArtProvider, CoverResult};

// Discord Rich Presence optimized constants
const THUMBNAIL_SIZE: u32 = 500; // Discord displays ~300-400px
const JPEG_QUALITY: u8 = 85;

pub struct ImgbbProvider {
    config: ImgBBConfig,
    client: Arc<ImgBB>,
}

impl ImgbbProvider {
    pub fn with_config(config: ImgBBConfig) -> Self {
        info!("Initializing ImgBB provider");
        let api_key = config.api_key.clone().expect("API key must be provided");
        trace!("Creating ImgBB client with provided API key");
        Self { 
            client: Arc::new(ImgBB::new(api_key)),
            config,
        }
    }

    /// Generate a meaningful name for the image based on metadata
    fn generate_image_name(&self, metadata: &Metadata) -> String {
        trace!("Generating image name from metadata");
        let artist_part = metadata.artists()
            .and_then(|artists| artists.first().map(|s| s.to_string()));
        
        let title_part = metadata.title().map(|s| s.to_string());
        
        let name = match (artist_part, title_part) {
            (Some(artist), Some(title)) => format!("{} - {}", artist, title),
            (Some(artist), None) => artist,
            (None, Some(title)) => title,
            (None, None) => "mprisence_cover".to_string()
        };
        
        trace!("Generated image name: {}", name);
        name
    }
    
    /// Resize image if it exceeds Discord Rich Presence optimal size.
    /// Uses Lanczos3 algorithm for high quality resizing and optimal JPEG compression.
    fn resize_if_needed(&self, image_data: &[u8]) -> Result<Vec<u8>, CoverArtError> {
        trace!("Analyzing image for potential resizing");
        let img = image::load_from_memory(image_data)
            .map_err(|e| {
                error!("Failed to load image data: {}", e);
                CoverArtError::provider_error("imgbb", &format!("Failed to load image: {}", e))
            })?;

        let (width, height) = img.dimensions();
        
        if width <= THUMBNAIL_SIZE && height <= THUMBNAIL_SIZE {
            debug!("Image dimensions {}x{} are within Discord limits", width, height);
            return Ok(image_data.to_vec());
        }

        debug!("Resizing image from {}x{} to fit Discord size limit ({}px)",
            width, height, THUMBNAIL_SIZE);

        let ratio = f64::min(
            THUMBNAIL_SIZE as f64 / width as f64,
            THUMBNAIL_SIZE as f64 / height as f64
        );
        
        let new_width = (width as f64 * ratio).floor() as u32;
        let new_height = (height as f64 * ratio).floor() as u32;

        trace!("Performing image resize to {}x{}", new_width, new_height);
        let resized = img.resize_exact(new_width, new_height, image::imageops::FilterType::Lanczos3);
        let rgb_image = resized.to_rgb8();

        let mut buffer = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, JPEG_QUALITY);
        encoder.encode(
            rgb_image.as_raw(),
            new_width,
            new_height,
            ExtendedColorType::Rgb8
        ).map_err(|e| {
            error!("Failed to encode resized image: {}", e);
            CoverArtError::provider_error("imgbb", &format!("Failed to encode resized image: {}", e))
        })?;

        debug!("Successfully resized image to {}x{}, new size: {} bytes", 
            new_width, new_height, buffer.len());
        Ok(buffer)
    }

    /// Process different types of art sources and upload to ImgBB
    async fn upload_art_source<'a>(&self, source: &'a ImgbbArtSource<'a>, metadata: &Metadata) -> Result<Option<String>, CoverArtError> {
        if self.config.api_key.is_none() {
            error!("ImgBB provider called without API key configuration");
            return Err(CoverArtError::provider_error(
                "imgbb", 
                "API key not configured"
            ));
        }

        let image_name = self.generate_image_name(metadata);
        debug!("Preparing ImgBB upload with name: {}", image_name);

        let response = match source {
            ImgbbArtSource::Bytes(data) => {
                trace!("Processing binary data: {} bytes", data.len());
                let processed_data = self.resize_if_needed(data)?;
                debug!("Uploading processed data ({} bytes) to ImgBB", processed_data.len());
                
                let mut builder = self.client.upload_builder()
                    .name(&image_name);
                
                if self.config.expiration > 0 {
                    trace!("Setting image expiration to {} seconds", self.config.expiration);
                    builder = builder.expiration(self.config.expiration);
                }
                
                builder.bytes(&processed_data)
                    .upload()
                    .await
            },
            ImgbbArtSource::Base64(data) => {
                trace!("Processing base64 encoded data");
                let binary_data = STANDARD.decode(data.as_bytes())
                    .map_err(|e| {
                        error!("Failed to decode base64 data: {}", e);
                        CoverArtError::provider_error("imgbb", &format!("Failed to decode base64: {}", e))
                    })?;
                
                let processed_data = self.resize_if_needed(&binary_data)?;
                let encoded = STANDARD.encode(&processed_data);
                debug!("Uploading processed base64 data to ImgBB");
                
                let mut builder = self.client.upload_builder()
                    .name(&image_name);
                
                if self.config.expiration > 0 {
                    trace!("Setting image expiration to {} seconds", self.config.expiration);
                    builder = builder.expiration(self.config.expiration);
                }
                
                builder.data(&encoded)
                    .upload()
                    .await
            }
        }.map_err(|e| {
            error!("ImgBB upload failed: {}", e);
            CoverArtError::provider_error("imgbb", &format!("Upload failed: {}", e))
        })?;

        let url = if let Some(data) = response.data {
            data.url.or(data.display_url)
        } else {
            None
        };

        match url {
            Some(ref url) => {
                info!("Successfully uploaded image to ImgBB");
                trace!("ImgBB provided URL: {}", url);
            },
            None => {
                warn!("ImgBB upload succeeded but no URL was returned");
            }
        }

        Ok(url)
    }
}

/// Internal representation of art source data
#[derive(Debug)]
enum ImgbbArtSource<'a> {
    /// Raw binary data (using Cow to avoid unnecessary copying)
    Bytes(Cow<'a, [u8]>),
    /// Base64 encoded data (without data:image prefix)
    Base64(Cow<'a, str>),
}

#[async_trait]
impl CoverArtProvider for ImgbbProvider {
    fn name(&self) -> &'static str {
        "imgbb"
    }
    
    fn supports_source_type(&self, source: &ExternalArtSource) -> bool {
        trace!("Checking source type support for ImgBB provider");
        let supported = matches!(source, 
            ExternalArtSource::Bytes(_) | 
            ExternalArtSource::Base64(_)
        );
        if !supported {
            trace!("Source type not supported by ImgBB provider");
        }
        supported
    }
    
    async fn process(&self, source: ExternalArtSource, metadata: &Metadata) -> Result<Option<CoverResult>, CoverArtError> {
        if self.config.api_key.is_none() {
            warn!("ImgBB provider is disabled (no API key configured)");
            return Ok(None);
        }
        
        debug!("Processing cover art with ImgBB provider");
        let internal_source = match source {
            ExternalArtSource::Bytes(data) => {
                trace!("Converting to internal bytes source");
                ImgbbArtSource::Bytes(Cow::Owned(data))
            },
            ExternalArtSource::Base64(data) => {
                trace!("Converting to internal base64 source");
                ImgbbArtSource::Base64(Cow::Owned(data))
            },
            _ => {
                debug!("Source type not supported by ImgBB provider");
                return Ok(None);
            }
        };
        
        match self.upload_art_source(&internal_source, metadata).await? {
            Some(url) => {
                let expiration = if self.config.expiration > 0 {
                    Some(Duration::from_secs(self.config.expiration))
                } else {
                    None
                };
                
                let result = CoverResult {
                    url,
                    provider: self.name().to_string(),
                    expiration,
                };
                
                info!("ImgBB provider successfully processed cover art");
                trace!("Generated URL: {} (expires in {:?})", result.url, result.expiration);
                
                Ok(Some(result))
            },
            None => {
                warn!("ImgBB upload succeeded but no URL was returned");
                Ok(None)
            }
        }
    }
}
