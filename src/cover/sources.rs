use log::{debug, info, warn, trace};
use mpris::Metadata;
use std::path::PathBuf;
use url::Url;
use base64::{Engine as _, engine::general_purpose::STANDARD};

use crate::cover::error::CoverArtError;

#[derive(Debug, Clone)]
pub enum ArtSource {
    Url(String),
    File(PathBuf),
    Base64(String),
    Bytes(Vec<u8>),
}

#[allow(dead_code)]
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

#[allow(dead_code)]
fn extract_base64(url_str: &str) -> Result<Option<ArtSource>, CoverArtError> {
    let image_type = url_str
        .strip_prefix("data:image/")
        .and_then(|s| s.split(';').next())
        .unwrap_or("unknown");
    
    info!("Found base64 encoded {} image in metadata", image_type);
    
    if let Some(base64_data) = url_str.split(";base64,").nth(1) {
        debug!("Successfully extracted base64 data ({} bytes)", base64_data.len());
        Ok(Some(ArtSource::Base64(base64_data.to_string())))
    } else {
        warn!("Malformed base64 image URL");
        Ok(None)
    }
}

#[allow(dead_code)]
fn extract_url(url_str: &str) -> Result<Option<ArtSource>, CoverArtError> {
    match Url::parse(url_str) {
        Ok(url) => {
            match url.scheme() {
                "http" | "https" => {
                    debug!("Found direct HTTP(S) URL: {}", url_str);
                    Ok(Some(ArtSource::Url(url_str.to_string())))
                }
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

#[allow(dead_code)]
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
    pub fn from_art_url(url: &str) -> Option<Self> {
        trace!("Converting art URL to source: {}", url);

        if url.starts_with("data:image/") && url.contains("base64,") {
            return url.split("base64,").nth(1)
                .map(|data| {
                    debug!("Detected base64 encoded image data");
                    Self::Base64(data.to_string())
                });
        }

        if url.starts_with("http://") || url.starts_with("https://") {
            debug!("Detected HTTP(S) URL");
            return Some(Self::Url(url.to_string()));
        }

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

    #[allow(dead_code)]
    pub fn from_bytes(data: Vec<u8>) -> Self {
        trace!("Creating art source from {} bytes", data.len());
        Self::Bytes(data)
    }

    #[allow(dead_code)]
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