use log::{debug, info, warn, trace};
use mpris::Metadata;
use std::path::PathBuf;
use url::Url;
use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::cover::error::CoverArtError;

/// Source of art data with different representations
#[derive(Debug, Clone)]
pub enum ArtSource {
    /// Direct HTTP(S) URL for Discord
    Url(String),
    /// Local file path
    File(PathBuf),
    /// Base64 encoded image data
    Base64(String),
    /// Raw bytes of image data
    Bytes(Vec<u8>),
}

/// Extract the art source from metadata
pub fn extract_from_metadata(metadata: &Metadata) -> Result<Option<ArtSource>, CoverArtError> {
    if let Some(url_str) = metadata.art_url() {
        debug!("Found art URL in metadata: {}", url_str);
        
        // Check if base64 encoded image
        if url_str.starts_with("data:image/") && url_str.contains(";base64,") {
            extract_base64(url_str)
        } else {
            extract_url(url_str)
        }
    } else {
        debug!("No art URL in metadata");
        Ok(None)
    }
}

/// Extract base64 encoded image data
fn extract_base64(url_str: &str) -> Result<Option<ArtSource>, CoverArtError> {
    // Extract image type for better logging
    let image_type = url_str
        .strip_prefix("data:image/")
        .and_then(|s| s.split(';').next())
        .unwrap_or("unknown");
    
    info!("Found base64 encoded {} image in metadata", image_type);
    
    // Extract base64 data after the comma
    if let Some(base64_data) = url_str.split(";base64,").nth(1) {
        debug!("Successfully extracted base64 data ({} bytes)", base64_data.len());
        Ok(Some(ArtSource::Base64(base64_data.to_string())))
    } else {
        warn!("Malformed base64 image URL");
        Ok(None)
    }
}

/// Extract file path or direct URL
fn extract_url(url_str: &str) -> Result<Option<ArtSource>, CoverArtError> {
    match Url::parse(url_str) {
        Ok(url) => {
            match url.scheme() {
                // HTTP(S) URLs can be used directly
                "http" | "https" => {
                    debug!("Found direct HTTP(S) URL: {}", url_str);
                    Ok(Some(ArtSource::Url(url_str.to_string())))
                }
                // File URLs need to be converted to paths
                "file" => {
                    debug!("Found file URL: {}", url_str);
                    if let Ok(path) = url.to_file_path() {
                        if path.exists() {
                            debug!("File exists at path: {:?}", path);
                            Ok(Some(ArtSource::File(path)))
                        } else {
                            warn!("File does not exist: {:?}", path);
                            Ok(None)
                        }
                    } else {
                        warn!("Invalid file path in URL: {}", url_str);
                        Ok(None)
                    }
                }
                // Other schemes are not supported
                scheme => {
                    debug!("Unsupported URL scheme: {}", scheme);
                    Ok(None)
                }
            }
        }
        Err(e) => {
            debug!("Invalid URL: {} ({})", url_str, e);
            Ok(None)
        }
    }
}

/// Load a file into bytes
pub async fn load_file(path: PathBuf) -> Result<Option<ArtSource>, CoverArtError> {
    match tokio::fs::read(&path).await {
        Ok(data) => {
            info!("Successfully read file: {:?} ({} bytes)", path, data.len());
            Ok(Some(ArtSource::Bytes(data)))
        }
        Err(e) => {
            warn!("Failed to read file: {:?} ({})", path, e);
            Ok(None)
        }
    }
}

impl ArtSource {
    /// Convert a string that may be a URL, file path, or base64 data into an ArtSource
    pub fn from_art_url(url: &str) -> Option<Self> {
        trace!("Converting art URL to source: {}", url);

        // Handle base64 data URLs
        if url.starts_with("data:image/") && url.contains("base64,") {
            return url.split("base64,").nth(1)
                .map(|data| {
                    debug!("Detected base64 encoded image data");
                    Self::Base64(data.to_string())
                });
        }

        // Handle HTTP(S) URLs
        if url.starts_with("http://") || url.starts_with("https://") {
            debug!("Detected HTTP(S) URL");
            return Some(Self::Url(url.to_string()));
        }

        // Handle file URLs and paths
        let path = if url.starts_with("file://") {
            url[7..].parse().ok()
        } else {
            url.parse().ok()
        };

        path.map(|p| {
            debug!("Detected file path");
            Self::File(p)
        })
    }

    /// Create an ArtSource from raw bytes
    pub fn from_bytes(data: Vec<u8>) -> Self {
        trace!("Creating art source from {} bytes", data.len());
        Self::Bytes(data)
    }

    /// Convert the art source to base64 if possible
    pub fn to_base64(&self) -> Option<String> {
        match self {
            Self::Base64(data) => Some(data.clone()),
            Self::Bytes(data) => {
                trace!("Converting bytes to base64");
                Some(STANDARD.encode(data))
            }
            _ => None
        }
    }
} 