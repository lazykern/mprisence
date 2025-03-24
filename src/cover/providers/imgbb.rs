use async_trait::async_trait;
use log::{debug, info, warn};
use mpris::Metadata;
use std::{borrow::Cow, time::Duration, sync::Arc};
use image::{GenericImageView, ExtendedColorType};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use imgbb::ImgBB;

use crate::cover::error::CoverArtError;
use crate::cover::sources::ArtSource as ExternalArtSource;
use super::{CoverArtProvider, CoverResult};

// Discord Rich Presence optimized constants
const THUMBNAIL_SIZE: u32 = 500; // Discord displays ~300-400px
const JPEG_QUALITY: u8 = 85;

/// Configuration options for ImgBB provider
#[derive(Clone, Debug)]
pub struct ImgbbConfig {
    /// API key for ImgBB service
    pub api_key: String,
    /// Expiration time in seconds (0 = no expiration)
    pub expiration: Option<u64>,
}

impl Default for ImgbbConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            expiration: None,
        }
    }
}
pub struct ImgbbProvider {
    config: ImgbbConfig,
    client: Arc<ImgBB>,
}

impl ImgbbProvider {
    pub fn with_config(config: ImgbbConfig) -> Self {
        debug!("Creating ImgBB provider with custom config");
        Self { 
            client: Arc::new(ImgBB::new(config.api_key.clone())),
            config,
        }
    }

    /// Generate a meaningful name for the image based on metadata
    fn generate_image_name(&self, metadata: &Metadata) -> String {
        let artist_part = metadata.artists()
            .and_then(|artists| artists.first().map(|s| s.to_string()));
        
        let title_part = metadata.title().map(|s| s.to_string());
        
        match (artist_part, title_part) {
            (Some(artist), Some(title)) => format!("{} - {}", artist, title),
            (Some(artist), None) => artist,
            (None, Some(title)) => title,
            (None, None) => "mprisence_cover".to_string()
        }
    }
    
    /// Resize image if it exceeds Discord Rich Presence optimal size.
    /// Uses Lanczos3 algorithm for high quality resizing and optimal JPEG compression.
    fn resize_if_needed(&self, image_data: &[u8]) -> Result<Vec<u8>, CoverArtError> {
        let img = image::load_from_memory(image_data)
            .map_err(|e| CoverArtError::provider_error("imgbb", &format!("Failed to load image: {}", e)))?;

        let (width, height) = img.dimensions();
        
        if width <= THUMBNAIL_SIZE && height <= THUMBNAIL_SIZE {
            debug!("Image size {}x{} within Discord Rich Presence limits", width, height);
            return Ok(image_data.to_vec());
        }

        info!("Resizing image from {}x{} to fit Discord Rich Presence size ({}px)",
            width, height, THUMBNAIL_SIZE);

        let ratio = f64::min(
            THUMBNAIL_SIZE as f64 / width as f64,
            THUMBNAIL_SIZE as f64 / height as f64
        );
        
        let new_width = (width as f64 * ratio).floor() as u32;
        let new_height = (height as f64 * ratio).floor() as u32;

        let resized = img.resize_exact(new_width, new_height, image::imageops::FilterType::Lanczos3);
        let rgb_image = resized.to_rgb8();

        let mut buffer = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buffer, JPEG_QUALITY);
        encoder.encode(
            rgb_image.as_raw(),
            new_width,
            new_height,
            ExtendedColorType::Rgb8
        ).map_err(|e| CoverArtError::provider_error("imgbb", &format!("Failed to encode resized image: {}", e)))?;

        debug!("Resized image to {}x{} for Discord Rich Presence, new size: {} bytes", 
            new_width, new_height, buffer.len());
        Ok(buffer)
    }

    /// Process different types of art sources and upload to ImgBB
    async fn upload_art_source<'a>(&self, source: &'a ImgbbArtSource<'a>, metadata: &Metadata) -> Result<Option<String>, CoverArtError> {
        if self.config.api_key.is_empty() {
            return Err(CoverArtError::provider_error(
                "imgbb", 
                "API key not configured"
            ));
        }

        let image_name = self.generate_image_name(metadata);
        info!("Uploading to ImgBB with name: {}", image_name);

        // Process and upload image based on source type
        let response = match source {
            ImgbbArtSource::Bytes(data) => {
                info!("Processing binary data: {} bytes", data.len());
                let processed_data = self.resize_if_needed(data)?;
                info!("Uploading processed data: {} bytes with name: {}", processed_data.len(), image_name);
                
                // Configure uploader with all options
                let mut builder = self.client.upload_builder()
                    .name(&image_name);
                
                if let Some(exp) = self.config.expiration.filter(|&exp| exp > 0) {
                    builder = builder.expiration(exp);
                }
                
                // Set the data and upload
                builder.bytes(&processed_data)
                    .upload()
                    .await
            },
            ImgbbArtSource::Base64(data) => {
                info!("Processing base64 data");
                let binary_data = STANDARD.decode(data.as_bytes())
                    .map_err(|e| CoverArtError::provider_error("imgbb", &format!("Failed to decode base64: {}", e)))?;
                
                let processed_data = self.resize_if_needed(&binary_data)?;
                let encoded = STANDARD.encode(&processed_data);
                info!("Uploading processed base64 data with name: {}", image_name);
                
                // Configure uploader with all options
                let mut builder = self.client.upload_builder()
                    .name(&image_name);
                
                if let Some(exp) = self.config.expiration.filter(|&exp| exp > 0) {
                    builder = builder.expiration(exp);
                }
                
                // Set the data and upload
                builder.data(&encoded)
                    .upload()
                    .await
            }
        }.map_err(|e| CoverArtError::provider_error("imgbb", &format!("Upload failed: {}", e)))?;

        // Extract URL from response
        let url = if let Some(data) = response.data {
            data.url.or(data.display_url)
        } else {
            None
        };

        if url.is_some() {
            info!("Successfully uploaded image to ImgBB");
        } else {
            warn!("ImgBB upload successful but no URL was returned");
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
        // ImgBB works with binary data and base64 encoded images
        matches!(source, 
            ExternalArtSource::Bytes(_) | 
            ExternalArtSource::Base64(_)
        )
    }
    
    async fn process(&self, source: ExternalArtSource, metadata: &Metadata) -> Result<Option<CoverResult>, CoverArtError> {
        // Skip processing if API key is not configured
        if self.config.api_key.is_empty() {
            warn!("ImgBB provider is disabled (no API key configured)");
            return Ok(None);
        }
        
        // Convert external source to our internal representation
        let internal_source = match source {
            ExternalArtSource::Bytes(data) => {
                ImgbbArtSource::Bytes(Cow::Owned(data))
            },
            ExternalArtSource::Base64(data) => {
                ImgbbArtSource::Base64(Cow::Owned(data))
            },
            _ => {
                debug!("Source type not supported by ImgBB provider");
                return Ok(None);
            }
        };
        
        // Upload the image and get URL
        match self.upload_art_source(&internal_source, metadata).await? {
            Some(url) => {
                // Calculate expiration from configuration
                let expiration = self.config.expiration
                    .filter(|&exp| exp > 0)
                    .map(|seconds| Duration::from_secs(seconds));
                
                // Create result with URL and expiration
                let result = CoverResult {
                    url,
                    provider: self.name().to_string(),
                    expiration,
                };
                
                info!("ImgBB provider generated URL: {} (expires: {:?})", 
                    result.url, result.expiration);
                
                Ok(Some(result))
            },
            None => {
                warn!("ImgBB upload successful but no URL was returned");
                Ok(None)
            }
        }
    }
}
