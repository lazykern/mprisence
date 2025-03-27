use log::{debug, info, warn, trace};
use std::path::PathBuf;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use walkdir::WalkDir;
use std::collections::HashSet;

use crate::cover::error::CoverArtError;

#[derive(Debug, Clone)]
pub enum ArtSource {
    Url(String),
    File(PathBuf),
    Base64(String),
    Bytes(Vec<u8>),
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

pub fn search_local_cover_art(
    directory: &PathBuf,
    file_names: &[String],
    max_depth: usize,
) -> Result<Option<ArtSource>, CoverArtError> {
    if !directory.exists() || !directory.is_dir() {
        debug!("Directory does not exist or is not a directory: {:?}", directory);
        return Ok(None);
    }

    debug!("Searching for cover art in directory: {:?} (max_depth: {})", directory, max_depth);
    trace!("Using file names: {:?}", file_names);

    let walker = WalkDir::new(directory)
        .max_depth(max_depth)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| {
            // Only traverse into directories or consider files
            e.file_type().is_dir() || e.file_type().is_file()
        });

    // Use HashSet for faster lookups
    let supported_extensions: HashSet<&str> = [
        "jpg", "jpeg", "png", "bmp", "gif",
        "tiff", "tif", "webp", "heic"
    ].iter().cloned().collect();

    // Convert file_names to lowercase HashSet for efficient comparison
    let target_stems: HashSet<String> = file_names.iter()
        .map(|s| s.to_lowercase())
        .collect();

    for entry in walker.filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }

        let file_path = entry.path();
        if let Some(file_stem) = file_path.file_stem().and_then(|s| s.to_str()) {
            if let Some(extension) = file_path.extension().and_then(|s| s.to_str()) {
                let lower_ext = extension.to_lowercase();
                // Check extension first
                if supported_extensions.contains(lower_ext.as_str()) {
                    // Then check stem
                    if target_stems.contains(&file_stem.to_lowercase()) {
                        info!("Found matching local cover art file: {:?} (format: {})", file_path, lower_ext);
                        return Ok(Some(ArtSource::File(file_path.to_path_buf())));
                    }
                }
            }
        }
    }

    debug!("No matching local cover art files found in directory: {:?}", directory);
    Ok(None)
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