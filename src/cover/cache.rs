use blake3::Hasher;
use log::{debug, error, trace, warn};
use serde::{Deserialize, Serialize};
use serde_json;
use std::{
    fs, io,
    path::PathBuf,
    time::{Duration, SystemTime},
};

use crate::cover::error::CoverArtError;
use crate::metadata::MetadataSource;

#[derive(Serialize, Deserialize)]
pub struct CacheEntry {
    pub url: String,
    pub provider: String,
    pub expires_at: SystemTime,
}

pub struct CoverCache {
    cache_dir: PathBuf,
    ttl: Duration,
}

impl CoverCache {
    pub fn new(ttl: Duration) -> Result<Self, CoverArtError> {
        trace!(
            "Creating new cover cache instance with TTL: {}s",
            ttl.as_secs()
        );
        let cache_dir = Self::get_cache_directory()?;

        Self::ensure_directory(&cache_dir)?;
        debug!("Initialized cover cache in directory: {:?}", cache_dir);

        Ok(Self { cache_dir, ttl })
    }

    pub fn get_cache_directory() -> Result<PathBuf, CoverArtError> {
        trace!("Determining cache directory path");
        dirs::cache_dir()
            .map(|dir| dir.join("mprisence").join("cover_art"))
            .ok_or_else(|| {
                error!("Failed to determine system cache directory");
                let err = io::Error::new(
                    io::ErrorKind::NotFound,
                    "Could not determine cache directory",
                );
                CoverArtError::from(err)
            })
    }

    pub fn ensure_directory(dir: &PathBuf) -> Result<(), CoverArtError> {
        Self::ensure_directory_with_options(dir, true)
    }

    pub fn ensure_directory_with_options(
        dir: &PathBuf,
        verify_writable: bool,
    ) -> Result<(), CoverArtError> {
        if !dir.exists() {
            debug!("Creating cache directory: {:?}", dir);
            fs::create_dir_all(dir).map_err(|e| {
                error!("Failed to create cache directory: {:?} - {}", dir, e);
                e
            })?;
        }

        if !dir.is_dir() {
            error!("Cache path exists but is not a directory: {:?}", dir);
            return Err(CoverArtError::from(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("Cache path exists but is not a directory: {:?}", dir),
            )));
        }

        if !verify_writable {
            return Ok(());
        }

        trace!("Verifying cache directory is writable");
        let test_file = dir.join(".write_test");
        match fs::write(&test_file, b"test") {
            Ok(_) => {
                if let Err(e) = fs::remove_file(&test_file) {
                    debug!("Note: Failed to remove write test file: {}", e);
                }
                trace!("Cache directory write verification successful");
                Ok(())
            }
            Err(e) => {
                error!("Cache directory is not writable: {:?} - {}", dir, e);
                Err(CoverArtError::from(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("Cache directory is not writable: {:?}", dir),
                )))
            }
        }
    }

    pub fn get(&self, metadata_source: &MetadataSource) -> Result<Option<String>, CoverArtError> {
        let key = self.generate_key(metadata_source);
        trace!("Looking up cache entry with key: {}", key);
        let path = self.cache_dir.join(key);

        if !path.exists() {
            trace!("No cache entry found");
            return Ok(None);
        }

        match fs::read(&path) {
            Ok(data) => match serde_json::from_slice::<CacheEntry>(&data) {
                Ok(entry) => {
                    let now = SystemTime::now();
                    if now > entry.expires_at {
                        debug!("Cache entry expired, removing file");
                        let _ = fs::remove_file(&path);
                        return Ok(None);
                    }

                    debug!("Found valid cache entry from provider: {}", entry.provider);
                    trace!("Cached URL: {}", entry.url);
                    Ok(Some(entry.url))
                }
                Err(e) => {
                    warn!(
                        "Failed to deserialize cache entry, removing corrupt file: {}",
                        e
                    );
                    let _ = fs::remove_file(&path);
                    Ok(None)
                }
            },
            Err(e) => {
                warn!("Failed to read cache file: {}", e);
                Ok(None)
            }
        }
    }

    pub fn store(
        &self,
        metadata_source: &MetadataSource,
        provider: &str,
        url: &str,
    ) -> Result<(), CoverArtError> {
        let key = self.generate_key(metadata_source);
        trace!("Storing cache entry with key: {}", key);
        let path = self.cache_dir.join(key);

        let entry = CacheEntry {
            url: url.to_string(),
            provider: provider.to_string(),
            expires_at: SystemTime::now() + self.ttl,
        };

        let data = serde_json::to_vec(&entry).map_err(|e| {
            error!("Failed to serialize cache entry: {}", e);
            CoverArtError::json_error(e)
        })?;

        fs::write(&path, data).map_err(|e| {
            error!("Failed to write cache entry to disk: {}", e);
            e
        })?;

        debug!(
            "Successfully stored cache entry from provider: {}",
            provider
        );
        trace!("Cache entry will expire at: {:?}", entry.expires_at);

        Ok(())
    }

    pub fn clean(&self) -> Result<usize, CoverArtError> {
        let mut cleaned = 0;
        let now = SystemTime::now();

        trace!("Starting cache cleanup scan");
        for entry in fs::read_dir(&self.cache_dir)? {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.is_dir() {
                    trace!("Skipping directory: {:?}", path);
                    continue;
                }

                trace!("Checking cache file: {:?}", path);
                if let Ok(data) = fs::read(&path) {
                    if let Ok(entry) = serde_json::from_slice::<CacheEntry>(&data) {
                        if now > entry.expires_at {
                            debug!("Removing expired cache entry: {:?}", path);
                            if fs::remove_file(&path).is_ok() {
                                cleaned += 1;
                            } else {
                                warn!("Failed to remove expired cache file: {:?}", path);
                            }
                        }
                    } else {
                        warn!("Removing invalid cache entry: {:?}", path);
                        if fs::remove_file(&path).is_ok() {
                            cleaned += 1;
                        }
                    }
                }
            }
        }

        debug!("Cache cleanup completed, removed {} entries", cleaned);
        Ok(cleaned)
    }

    fn generate_key(&self, metadata_source: &MetadataSource) -> String {
        trace!("Generating cache key from metadata source");
        let mut hasher = Hasher::new();
        let mut key_components = Vec::new();

        // 1. Title
        if let Some(title) = metadata_source.title() {
            if !title.is_empty() {
                key_components.push(format!("title:{}", title));
            }
        }

        // 2. Track Artists (sorted)
        if let Some(artists) = metadata_source.artists() {
            if !artists.is_empty() {
                let mut sorted_artists = artists.clone();
                sorted_artists.sort_unstable();
                key_components.push(format!("artists:{}", sorted_artists.join("|")));
            }
        }

        // 3. Album (if non-empty)
        if let Some(album) = metadata_source.album() {
            if !album.is_empty() {
                key_components.push(format!("album:{}", album));
                // 4. Album Artists (if available, non-empty, and different from track artists)
                if let Some(album_artists) = metadata_source.album_artists() {
                    if !album_artists.is_empty()
                        && Some(&album_artists) != metadata_source.artists().as_ref()
                    {
                        let mut sorted_album_artists = album_artists.clone();
                        sorted_album_artists.sort_unstable();
                        key_components
                            .push(format!("album_artists:{}", sorted_album_artists.join("|")));
                    }
                }
            }
        }

        // 5. URL (as fallback or additional differentiator)
        if let Some(url) = metadata_source.url() {
            if !url.is_empty() {
                key_components.push(format!("url:{}", url));
            }
        }

        // If somehow still empty, use a default
        if key_components.is_empty() {
            warn!("Could not generate meaningful cache key components, using default key");
            key_components.push("default_mprisence_key".to_string());
        }

        let combined_key_data = key_components.join("||"); // Use a distinct separator
        trace!("Hashing data for key: {}", combined_key_data);
        hasher.update(combined_key_data.as_bytes());

        let key = hasher.finalize().to_hex().to_string();
        trace!("Generated cache key: {}", key);
        key
    }
}
