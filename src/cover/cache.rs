use blake3::Hasher;
use log::{debug, info, warn};
use mpris::Metadata;
use serde::{Deserialize, Serialize};
use serde_json;
use std::{
    fs, io,
    path::PathBuf,
    time::{Duration, SystemTime},
};

use crate::cover::error::CoverArtError;

/// Cache entry with expiration
#[derive(Serialize, Deserialize)]
pub struct CacheEntry {
    pub url: String,
    pub provider: String,
    pub expires_at: SystemTime,
}

/// Simple cache for cover art URLs
pub struct CoverCache {
    cache_dir: PathBuf,
    ttl: Duration,
}

impl CoverCache {
    /// Create a new cache with the specified TTL
    pub fn new(ttl: Duration) -> Result<Self, CoverArtError> {
        let cache_dir = Self::get_cache_directory()?;
        
        // Ensure cache directory exists and is accessible
        Self::ensure_directory(&cache_dir)?;
        debug!("Using cache directory: {:?}", cache_dir);

        Ok(Self { cache_dir, ttl })
    }

    /// Get the standard cache directory for cover art
    pub fn get_cache_directory() -> Result<PathBuf, CoverArtError> {
        // Get cache directory from standard location
        dirs::cache_dir()
            .map(|dir| dir.join("mprisence").join("cover_art"))
            .ok_or_else(|| {
                let err = io::Error::new(
                    io::ErrorKind::NotFound,
                    "Could not determine cache directory",
                );
                CoverArtError::from(err)
            })
    }

    /// Ensure the directory exists and is writable
    pub fn ensure_directory(dir: &PathBuf) -> Result<(), CoverArtError> {
        Self::ensure_directory_with_options(dir, true)
    }

    /// Ensure the directory exists with options for verification
    pub fn ensure_directory_with_options(dir: &PathBuf, verify_writable: bool) -> Result<(), CoverArtError> {
        // Create directory if it doesn't exist
        if !dir.exists() {
            debug!("Creating cache directory: {:?}", dir);
            fs::create_dir_all(dir).map_err(|e| {
                warn!("Failed to create cache directory: {:?} - {}", dir, e);
                e
            })?;
        }

        // Verify the directory is actually a directory (not a file)
        if !dir.is_dir() {
            return Err(CoverArtError::from(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("Cache path exists but is not a directory: {:?}", dir),
            )));
        }

        // Skip write verification if not needed
        if !verify_writable {
            return Ok(());
        }

        // Test if directory is writable by creating and removing a test file
        let test_file = dir.join(".write_test");
        match fs::write(&test_file, b"test") {
            Ok(_) => {
                // Clean up test file
                if let Err(e) = fs::remove_file(&test_file) {
                    debug!("Note: Failed to remove test file: {}", e);
                    // Not critical, continue anyway
                }
                Ok(())
            }
            Err(e) => {
                warn!("Cache directory is not writable: {:?} - {}", dir, e);
                Err(CoverArtError::from(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("Cache directory is not writable: {:?}", dir),
                )))
            }
        }
    }

    /// Get a cached URL for the metadata, if available and not expired
    pub fn get(&self, metadata: &Metadata) -> Result<Option<String>, CoverArtError> {
        let key = self.generate_key(metadata);
        let path = self.cache_dir.join(key);

        if !path.exists() {
            return Ok(None);
        }

        // Read and deserialize cache entry
        match fs::read(&path) {
            Ok(data) => match serde_json::from_slice::<CacheEntry>(&data) {
                Ok(entry) => {
                    // Check if entry has expired
                    let now = SystemTime::now();
                    if now > entry.expires_at {
                        debug!("Cache entry expired, removing");
                        let _ = fs::remove_file(&path);
                        return Ok(None);
                    }

                    info!(
                        "Found cached cover art URL from provider: {}",
                        entry.provider
                    );
                    Ok(Some(entry.url))
                }
                Err(e) => {
                    warn!("Failed to deserialize cache entry: {}", e);
                    // Remove corrupted entry
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

    /// Store a URL in the cache
    pub fn store(
        &self,
        metadata: &Metadata,
        provider: &str,
        url: &str,
    ) -> Result<(), CoverArtError> {
        let key = self.generate_key(metadata);
        let path = self.cache_dir.join(key);

        // Create cache entry
        let entry = CacheEntry {
            url: url.to_string(),
            provider: provider.to_string(),
            expires_at: SystemTime::now() + self.ttl,
        };

        // Serialize and write to file
        let data = serde_json::to_vec(&entry).map_err(|e| CoverArtError::json_error(e))?;
        fs::write(&path, data)?;
        debug!("Stored URL in cache from provider: {}", provider);

        Ok(())
    }

    /// Clean expired entries from cache
    pub fn clean(&self) -> Result<usize, CoverArtError> {
        let mut cleaned = 0;
        let now = SystemTime::now();

        // Check all files in cache directory
        for entry in fs::read_dir(&self.cache_dir)? {
            if let Ok(entry) = entry {
                let path = entry.path();
                // Skip directories
                if path.is_dir() {
                    continue;
                }

                // Try to read and parse the cache entry
                if let Ok(data) = fs::read(&path) {
                    if let Ok(entry) = serde_json::from_slice::<CacheEntry>(&data) {
                        if now > entry.expires_at {
                            debug!("Removing expired cache entry: {:?}", path);
                            if fs::remove_file(&path).is_ok() {
                                cleaned += 1;
                            }
                        }
                    } else {
                        // Invalid format, clean it
                        debug!("Removing invalid cache entry: {:?}", path);
                        if fs::remove_file(&path).is_ok() {
                            cleaned += 1;
                        }
                    }
                }
            }
        }

        Ok(cleaned)
    }

    /// Generate a cache key from metadata
    fn generate_key(&self, metadata: &Metadata) -> String {
        let mut hasher = Hasher::new();

        // Prefer album-level cache
        if let Some(album) = metadata.album_name() {
            hasher.update(b"album:");
            hasher.update(album.as_bytes());

            if let Some(artists) = metadata.album_artists() {
                for artist in artists {
                    hasher.update(artist.as_bytes());
                }
            }
        } else {
            // Fall back to track-level
            hasher.update(b"track:");

            if let Some(id) = metadata.track_id() {
                hasher.update(id.to_string().as_bytes());
            } else if let Some(title) = metadata.title() {
                hasher.update(title.as_bytes());

                if let Some(artists) = metadata.artists() {
                    for artist in artists {
                        hasher.update(artist.as_bytes());
                    }
                }
            }
        }

        hasher.finalize().to_hex().to_string()
    }
} 