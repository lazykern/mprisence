use async_trait::async_trait;
use imgbb::{ImgBB, model::Response};
use log::{debug, error, info, warn};
use mpris::Metadata;
use std::{borrow::Cow, time::Duration};

use crate::cover::error::CoverArtError;
use crate::cover::sources::ArtSource as ExternalArtSource;
use super::{CoverArtProvider, CoverResult};

/// Configuration options for ImgBB provider
#[derive(Clone, Debug)]
pub struct ImgbbConfig {
    /// API key for ImgBB service
    pub api_key: String,
    /// Expiration time in seconds (0 = no expiration)
    pub expiration: Option<u64>,
    /// Default name for uploaded images (will be prefixed with artist-title if available)
    pub default_name: Option<String>,
    /// Request timeout in seconds (default: 30)
    pub timeout: Option<u64>,
    /// User agent string (default: "mprisence/1.0")
    pub user_agent: Option<String>,
}

impl Default for ImgbbConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            expiration: None,
            default_name: Some("mprisence_cover".to_string()),
            timeout: Some(30),
            user_agent: Some("mprisence/1.0".to_string()),
        }
    }
}

/// ImgBB provider for uploading cover art to ImgBB service.
/// 
/// This provider supports:
/// - Reading image data from file paths
/// - Converting base64 encoded images from metadata
/// 
/// To implement full functionality:
/// 1. Add imgbb-rs as a dependency in Cargo.toml:
///    ```toml
///    [dependencies]
///    imgbb = "0.2"
///    ```
/// 2. Uncomment the relevant code in this file
/// 3. Add error mapping from imgbb::Error to CoverArtError
#[derive(Clone)]
pub struct ImgbbProvider {
    config: ImgbbConfig,
}

impl ImgbbProvider {
    /// Create a new ImgBB provider with just an API key
    pub fn new(api_key: String) -> Self {
        debug!("Creating new ImgBB provider");
        let mut config = ImgbbConfig::default();
        config.api_key = api_key;
        Self { config }
    }

    /// Create a new ImgBB provider with full custom configuration
    pub fn with_config(config: ImgbbConfig) -> Self {
        debug!("Creating ImgBB provider with custom config");
        Self { config }
    }

    /// Create a configured ImgBB client using the builder pattern
    fn create_client(&self) -> Result<ImgBB, CoverArtError> {
        debug!("Creating ImgBB client with API key: {}", self.config.api_key);
        
        if self.config.api_key.is_empty() {
            return Err(CoverArtError::provider_error(
                "imgbb", 
                "API key not configured"
            ));
        }
        
        let mut builder = ImgBB::builder(&self.config.api_key);
        
        // Apply timeout if configured
        if let Some(timeout_secs) = self.config.timeout {
            debug!("Setting ImgBB client timeout to {} seconds", timeout_secs);
            builder = builder.timeout(Duration::from_secs(timeout_secs));
        }
        
        // Apply user agent if configured
        if let Some(user_agent) = &self.config.user_agent {
            debug!("Setting ImgBB client user agent to {}", user_agent);
            builder = builder.user_agent(user_agent);
        }
        
        // Build the client
        builder.build().map_err(|e| {
            error!("Failed to build ImgBB client: {}", e);
            CoverArtError::provider_error("imgbb", &format!("Failed to create client: {}", e))
        })
    }

    /// Generate a meaningful name for the image based on metadata
    fn generate_image_name(&self, metadata: &Metadata) -> String {
        // Try to build a name from artist and title
        let artist_part = metadata.artists()
            .and_then(|artists| artists.first().map(|s| s.to_string()));
        
        let title_part = metadata.title().map(|s| s.to_string());
        
        // Create name based on available parts
        match (artist_part, title_part) {
            (Some(artist), Some(title)) => format!("{} - {}", artist, title),
            (Some(artist), None) => artist,
            (None, Some(title)) => title,
            (None, None) => self.config.default_name.clone()
                .unwrap_or_else(|| "mprisence_cover".to_string())
        }
    }
    
    /// Extract the best URL from the ImgBB response
    fn extract_url_from_response(&self, response: Response) -> Option<String> {
        response.data.and_then(|data| {
            // Try to get URL in order of preference:
            // 1. Direct URL (best quality)
            // 2. Display URL (good for sharing)
            // 3. Image URL from image object
            data.url
                .or_else(|| data.display_url)
                .or_else(|| data.image.and_then(|img| img.url))
                .or_else(|| {
                    // Fall back to thumbnail URL if nothing else is available
                    data.thumb.and_then(|t| t.url)
                })
        }).map(|url| {
            debug!("Extracted URL from ImgBB response: {}", url);
            url
        }).or_else(|| {
            warn!("Could not extract any URL from ImgBB response");
            None
        })
    }
    
    /// Process different types of art sources and upload to ImgBB
    async fn upload_art_source<'a>(&self, source: &'a ImgbbArtSource<'a>, metadata: &Metadata) -> Result<Option<String>, CoverArtError> {
        // Create client with proper configuration
        let client = self.create_client()?;
        
        // Generate image name from metadata
        let image_name = self.generate_image_name(metadata);
        info!("Uploading to ImgBB with name: {}", image_name);
        
        // Create an uploader builder with all options
        let mut builder = client.upload_builder();
        
        // Set name and title (same value for simplicity)
        builder.name(&image_name).title(&image_name);
        
        // Set expiration if configured and non-zero
        if let Some(exp) = self.config.expiration.filter(|&exp| exp > 0) {
            debug!("Setting image expiration to {} seconds", exp);
            builder.expiration(exp);
        }
        
        // Set data based on source type
        match source {
            ImgbbArtSource::Bytes(data) => {
                info!("Uploading binary data: {} bytes", data.len());
                builder.bytes(data.as_ref());
            },
            ImgbbArtSource::Base64(data) => {
                info!("Uploading base64 data: {} bytes", data.len());
                builder.data(data.as_ref());
            }
        }
        
        // Execute upload and handle response
        match builder.upload().await {
            Ok(response) => {
                info!("Successfully uploaded image to ImgBB");
                Ok(self.extract_url_from_response(response))
            },
            Err(e) => {
                error!("Failed to upload to ImgBB: {}", e);
                Err(CoverArtError::provider_error("imgbb", &format!("Upload failed: {}", e)))
            }
        }
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
