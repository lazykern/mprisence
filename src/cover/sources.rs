use crate::cover::error::CoverArtError;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use blake3::Hasher;
use log::{debug, info, trace, warn};
use std::collections::HashSet;
use std::fs::File;
use std::path::PathBuf;
use walkdir::WalkDir;

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
            return url.split("base64,").nth(1).map(|data| {
                debug!("Detected base64 encoded image data");
                Self::Base64(data.to_string())
            });
        }

        if url.starts_with("http://") || url.starts_with("https://") {
            debug!("Detected HTTP(S) URL");
            return Some(Self::Url(url.to_string()));
        }

        let path = if let Some(stripped) = url.strip_prefix("file://") {
            match urlencoding::decode(stripped) {
                Ok(dec) => dec.parse().ok(),
                Err(_) => return None,
            }
        } else {
            url.parse().ok()
        };

        path.map(|p| {
            debug!("Detected file path");
            Self::File(p)
        })
    }

    pub async fn materialize_bytes(&self) -> Result<Option<Vec<u8>>, CoverArtError> {
        match self {
            Self::Bytes(data) => Ok(Some(data.clone())),
            Self::Base64(data) => match STANDARD.decode(data.as_bytes()) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(e) => {
                    warn!("Failed to decode base64 art payload: {}", e);
                    Ok(None)
                }
            },
            Self::File(path) => match tokio::fs::read(path).await {
                Ok(bytes) => Ok(Some(bytes)),
                Err(e) => {
                    warn!("Failed to read art file {:?}: {}", path, e);
                    Ok(None)
                }
            },
            Self::Url(_) => Ok(None),
        }
    }

    pub fn cache_key(&self) -> Result<String, CoverArtError> {
        let mut hasher = Hasher::new();
        match self {
            Self::Url(url) => {
                hasher.update(b"mprisence-cover-url-v1\0");
                hasher.update(url.as_bytes());
            }
            Self::File(path) => {
                hasher.update(b"mprisence-cover-content-v1\0");
                hasher.update_reader(File::open(path)?)?;
            }
            Self::Base64(data) => {
                hasher.update(b"mprisence-cover-content-v1\0");
                let bytes = STANDARD
                    .decode(data.as_bytes())
                    .map_err(|e| CoverArtError::other(format!("invalid base64 cover art: {e}")))?;
                hasher.update(&bytes);
            }
            Self::Bytes(data) => {
                hasher.update(b"mprisence-cover-content-v1\0");
                hasher.update(data);
            }
        }
        Ok(hasher.finalize().to_hex().to_string())
    }
}

pub fn search_local_cover_art(
    directory: &PathBuf,
    file_names: &[String],
    max_depth: usize,
) -> Result<Option<ArtSource>, CoverArtError> {
    if !directory.exists() || !directory.is_dir() {
        debug!(
            "Directory does not exist or is not a directory: {:?}",
            directory
        );
        return Ok(None);
    }

    debug!(
        "Searching for cover art in directory: {:?} (max_depth: {})",
        directory, max_depth
    );
    trace!("Using file names: {:?}", file_names);

    let walker = WalkDir::new(directory)
        .max_depth(max_depth)
        .follow_links(true)
        .into_iter()
        .filter_entry(|e| e.file_type().is_dir() || e.file_type().is_file());

    // Use HashSet for faster lookups
    let supported_extensions: HashSet<&str> = [
        "jpg", "jpeg", "png", "bmp", "gif", "tiff", "tif", "webp", "heic",
    ]
    .iter()
    .cloned()
    .collect();

    // Convert file_names to lowercase HashSet for efficient comparison
    let target_stems: HashSet<String> = file_names.iter().map(|s| s.to_lowercase()).collect();

    for entry in walker.filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }

        let file_path = entry.path();
        if let Some(file_stem) = file_path.file_stem().and_then(|s| s.to_str()) {
            if let Some(extension) = file_path.extension().and_then(|s| s.to_str()) {
                let lower_ext = extension.to_lowercase();
                if supported_extensions.contains(lower_ext.as_str())
                    && target_stems.contains(&file_stem.to_lowercase())
                {
                    info!(
                        "Found matching local cover art file: {:?} (format: {})",
                        file_path, lower_ext
                    );
                    return Ok(Some(ArtSource::File(file_path.to_path_buf())));
                }
            }
        }
    }

    debug!(
        "No matching local cover art files found in directory: {:?}",
        directory
    );
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::ArtSource;
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use std::{fs, time::SystemTime};

    #[test]
    fn identical_content_has_same_cache_key_across_source_types() {
        let bytes = b"identical album cover".to_vec();
        let path = std::env::temp_dir().join(format!(
            "mprisence-cover-key-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, &bytes).unwrap();

        let bytes_key = ArtSource::Bytes(bytes.clone()).cache_key().unwrap();
        let file_key = ArtSource::File(path.clone()).cache_key().unwrap();
        let base64_key = ArtSource::Base64(STANDARD.encode(&bytes))
            .cache_key()
            .unwrap();

        fs::remove_file(path).unwrap();
        assert_eq!(bytes_key, file_key);
        assert_eq!(bytes_key, base64_key);
    }

    #[test]
    fn different_content_has_different_cache_keys() {
        let first = ArtSource::Bytes(b"first".to_vec()).cache_key().unwrap();
        let second = ArtSource::Bytes(b"second".to_vec()).cache_key().unwrap();
        assert_ne!(first, second);
    }

    #[test]
    fn url_cache_keys_are_separate_from_content_keys() {
        let url = ArtSource::Url("https://cdn.example.com/cover.jpg".to_string())
            .cache_key()
            .unwrap();
        let content = ArtSource::Bytes(b"https://cdn.example.com/cover.jpg".to_vec())
            .cache_key()
            .unwrap();
        assert_ne!(url, content);
    }
}
