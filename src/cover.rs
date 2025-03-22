use async_trait::async_trait;
use blake3::Hasher;
use log::{debug, error, warn};
use mpris::Metadata;
use std::{
    fmt::Display,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime},
};
use thiserror::Error;
use url::Url;
use lazy_static::lazy_static;

// Import more specific modules from musicbrainz_rs
use musicbrainz_rs::{
    entity::{
        recording::{Recording, RecordingSearchQuery},
        release::{Release, ReleaseSearchQuery},
        release_group::{
            ReleaseGroup, ReleaseGroupSearchQuery,
        },
    },
    prelude::*,
};

use crate::config;

// Create a lazy static shared HTTP client for non-MusicBrainz requests
lazy_static! {
    static ref HTTP_CLIENT: reqwest::Client = {
        reqwest::Client::builder()
            .pool_max_idle_per_host(5)
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client")
    };
}

#[derive(Error, Debug)]
pub enum CoverArtError {
    #[error("Provider error: {0}")]
    Provider(String),

    #[error("No valid cover art found")]
    NotFound,

    #[error("Cache error: {0}")]
    Cache(#[from] std::io::Error),

    #[error("URL parse error: {0}")]
    Url(#[from] url::ParseError),

    #[error("Config error: {0}")]
    Config(#[from] config::ConfigError),

    #[error("MusicBrainz error: {0}")]
    MusicBrainz(#[from] musicbrainz_rs::Error),

    #[error("Request error: {0}")]
    Request(#[from] reqwest::Error),
}

/// Represents a found cover art file or URL
#[derive(Debug, Clone)]
pub enum CoverArtSource {
    /// HTTP(S) URL that can be used directly
    HttpUrl(String),
    /// Local file that needs to be hosted
    LocalFile(PathBuf),
}

enum CacheKey {
    Album(String),    // For album-level caching
    Track(String),    // For track-specific caching
}

// Implement Display for CacheKey
impl Display for CacheKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheKey::Album(key) => write!(f, "album-{}", key),
            CacheKey::Track(key) => write!(f, "track-{}", key),
        }
    }
}

impl CacheKey {
    fn generate(metadata: &Metadata) -> Self {
        // Try album key first
        if let Some(album_key) = Self::generate_album_key(metadata) {
            CacheKey::Album(album_key)
        } else {
            // Fall back to track key
            CacheKey::Track(Self::generate_track_key(metadata))
        }
    }

    fn generate_album_key(metadata: &Metadata) -> Option<String> {
        let mut hasher = Hasher::new();
        
        let album = metadata.album_name()?;
        hasher.update(album.as_bytes());
        
        if let Some(artists) = metadata.album_artists() {
            for artist in artists {
                hasher.update(artist.as_bytes());
            }
        }
        
        Some(hasher.finalize().to_hex().to_string())
    }

    fn generate_track_key(metadata: &Metadata) -> String {
        let mut hasher = Hasher::new();

        // Add all available metadata for uniqueness
        if let Some(title) = metadata.title() {
            hasher.update(title.as_bytes());
        }

        if let Some(artists) = metadata.artists() {
            for artist in artists {
                hasher.update(artist.as_bytes());
            }
        }

        if let Some(album) = metadata.album_name() {
            hasher.update(album.as_bytes());
        }

        if let Some(track_id) = metadata.track_id() {
            hasher.update(track_id.to_string().as_bytes());
        }

        hasher.finalize().to_hex().to_string()
    }

    fn to_path(&self, cache_dir: &Path) -> PathBuf {
        cache_dir.join(self.to_string())
    }
}

/// Utility functions for finding local cover art
#[derive(Clone)]
struct LocalUtils {
    file_names: Vec<String>,
}

impl LocalUtils {
    pub fn new(file_names: Vec<String>) -> Self {
        Self { file_names }
    }

    /// Find cover art from metadata URLs or local files
    pub async fn find_cover_art(
        &self,
        metadata: &Metadata,
    ) -> Result<Option<CoverArtSource>, CoverArtError> {
        // First check metadata URLs
        if let Some(source) = self.find_art_url_in_metadata(metadata)? {
            return Ok(Some(source));
        }

        // Then check for local cover files
        if let Some(source) = self.find_local_cover_files(metadata)? {
            return Ok(Some(source));
        }

        Ok(None)
    }

    fn find_art_url_in_metadata(
        &self,
        metadata: &Metadata,
    ) -> Result<Option<CoverArtSource>, CoverArtError> {
        // If we have a URL in the metadata, check if it's usable
        if let Some(url_str) = metadata.art_url() {
            match Url::parse(url_str) {
                Ok(url) => {
                    // If it's already an HTTP URL, return it directly
                    if url.scheme() == "http" || url.scheme() == "https" {
                        return Ok(Some(CoverArtSource::HttpUrl(url_str.to_string())));
                    }

                    // If it's a file URL, return it as a local file
                    if url.scheme() == "file" {
                        if let Ok(path) = url.to_file_path() {
                            if path.exists() {
                                return Ok(Some(CoverArtSource::LocalFile(path)));
                            }
                        }
                    }
                }
                Err(_) => {
                    debug!("Art URL in metadata is not a valid URL: {}", url_str);
                }
            }
        }
        Ok(None)
    }

    fn find_local_cover_files(
        &self,
        metadata: &Metadata,
    ) -> Result<Option<CoverArtSource>, CoverArtError> {
        // Look for local cover art files
        if let Some(url) = metadata.url() {
            if let Ok(parsed_url) = Url::parse(url) {
                if parsed_url.scheme() == "file" {
                    if let Ok(file_path) = parsed_url.to_file_path() {
                        let search_dir = if file_path.is_dir() {
                            file_path
                        } else {
                            file_path.parent().unwrap_or(&file_path).to_path_buf()
                        };

                        // Search for cover art files
                        for name in &self.file_names {
                            for ext in &["jpg", "jpeg", "png", "gif"] {
                                let image_path = search_dir.join(format!("{}.{}", name, ext));
                                if image_path.exists() {
                                    debug!("Found cover art file: {:?}", image_path);
                                    return Ok(Some(CoverArtSource::LocalFile(image_path)));
                                }
                            }
                        }
                    }
                }
            }
        }
        Ok(None)
    }
}

#[async_trait]
trait CoverArtProvider {
    fn name(&self) -> &'static str;
    async fn get_cover_url(&self, metadata: &Metadata) -> Result<Option<String>, CoverArtError>;
}

/// Main manager for cover art retrieval
pub struct CoverArtManager {
    providers: Vec<Box<dyn CoverArtProvider>>,
    cache: CoverArtCache,
}

impl CoverArtManager {
    pub fn new(config: &Arc<config::ConfigManager>) -> Result<Self, CoverArtError> {
        let cover_config = config.cover_config();
        let mut providers: Vec<Box<dyn CoverArtProvider>> = Vec::new();

        for provider in &cover_config.provider.provider {
            match provider.as_str() {
                "musicbrainz" => providers.push(Box::new(MusicbrainzProvider::new())),
                "imgbb" => {
                    if let Some(api_key) = cover_config.provider.imgbb.as_ref().and_then(|c| c.api_key.as_ref()) {
                        providers.push(Box::new(ImgbbProvider::new(api_key.clone())));
                    }
                }
                unknown => warn!("Unknown cover art provider: {}", unknown),
            }
        }

        Ok(Self {
            providers,
            cache: CoverArtCache::new()?,
        })
    }

    pub async fn get_cover_art(&self, metadata: &Metadata) -> Result<Option<String>, CoverArtError> {
        // Check disk cache first
        if let Some(url) = self.cache.get(metadata)? {
            debug!("Using cached cover art URL");
            return Ok(Some(url));
        }

        // Check metadata URL
        if let Some(url) = self.check_metadata_url(metadata)? {
            self.cache.put(metadata, "metadata", &url)?;
            return Ok(Some(url));
        }

        // Try each provider
        for provider in &self.providers {
            debug!("Trying cover art provider: {}", provider.name());
            match provider.get_cover_url(metadata).await {
                Ok(Some(url)) => {
                    debug!("Got URL from {}: {}", provider.name(), url);
                    self.cache.put(metadata, provider.name(), &url)?;
                    return Ok(Some(url));
                }
                Ok(None) => continue,
                Err(e) => warn!("Error from {}: {}", provider.name(), e),
            }
        }

        Ok(None)
    }

    fn check_metadata_url(&self, metadata: &Metadata) -> Result<Option<String>, CoverArtError> {
        if let Some(url_str) = metadata.art_url() {
            let url = Url::parse(url_str)?;
            if url.scheme() == "http" || url.scheme() == "https" {
                return Ok(Some(url_str.to_string()));
            }
        }
        Ok(None)
    }
}

/// Cache for cover art URLs
struct CoverArtCache {
    cache_dir: PathBuf,
    cache_duration: Duration,
}

impl CoverArtCache {
    pub fn new() -> Result<Self, CoverArtError> {
        let cache_dir = dirs::cache_dir()
            .map(|dir| dir.join("mprisence").join("cover_art"))
            .ok_or_else(|| {
                CoverArtError::Cache(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not find cache directory",
                ))
            })?;

        // Create parent directory if it doesn't exist
        if !cache_dir.exists() {
            fs::create_dir_all(&cache_dir)?;
        }
        
        Ok(Self { 
            cache_dir,
            cache_duration: Duration::from_secs(24 * 60 * 60), // 24 hours
        })
    }

    pub fn get(&self, metadata: &Metadata) -> Result<Option<String>, CoverArtError> {
        let cache_key = CacheKey::generate(metadata);
        let cache_file = cache_key.to_path(&self.cache_dir);
        
        if let Ok((url, timestamp)) = self.read_cache_file(&cache_file) {
            if timestamp.elapsed().unwrap() < self.cache_duration {
                return Ok(Some(url));
            }
        }
        
        Ok(None)
    }

    fn read_cache_file(&self, cache_file: &Path) -> Result<(String, SystemTime), CoverArtError> {
        if !cache_file.exists() {
            return Err(CoverArtError::Cache(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Cache file not found",
            )));
        }

        let metadata = fs::metadata(cache_file)?;
        let modified = metadata.modified()?;

        let mut file = File::open(cache_file)?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;

        Ok((content, modified))
    }

    pub fn put(&self, metadata: &Metadata, _provider: &str, url: &str) -> Result<(), CoverArtError> {
        let cache_key = CacheKey::generate(metadata);
        let cache_file = cache_key.to_path(&self.cache_dir);
        
        // Create parent directory if it doesn't exist
        if let Some(parent) = cache_file.parent() {
            fs::create_dir_all(parent)?;
        }
        
        let mut file = File::create(cache_file)?;
        file.write_all(url.as_bytes())?;
        
        Ok(())
    }
}

#[derive(Clone)]
struct MusicbrainzProvider;

impl MusicbrainzProvider {
    pub fn new() -> Self {
        Self {}
    }

    // Helper to check if a cover art URL exists - uses HTTP_CLIENT for non-MusicBrainz API requests
    async fn check_cover_url(url: &str) -> bool {
        match HTTP_CLIENT.head(url).send().await {
            Ok(response) if response.status().is_success() => true,
            _ => false,
        }
    }

    // Helper to get cover art URL for an ID and type
    async fn get_cover_art_url(entity_type: &str, id: &str) -> Option<String> {
        let url = format!("https://coverartarchive.org/{}/{}/front", entity_type, id);
        if Self::check_cover_url(&url).await {
            Some(url)
        } else {
            None
        }
    }

    // Unified search function that handles both release groups and releases
    async fn search_album<S: AsRef<str>>(
        &self,
        album: S,
        artists: &[S],
    ) -> Result<Option<String>, CoverArtError> {
        // First try release groups
        let mut builder = ReleaseGroupSearchQuery::query_builder();
        builder.release_group(album.as_ref());
        if let Some(artist) = artists.first() {
            builder.and().artist(artist.as_ref());
        }

        let results = ReleaseGroup::search(builder.build()).execute().await?;

        for group in results.entities.iter().take(2) {
            // Try release group cover
            if let Some(url) = Self::get_cover_art_url("release-group", &group.id).await {
                return Ok(Some(url));
            }

            // Try covers from releases in the group
            if let Some(releases) = &group.releases {
                for release in releases.iter().take(2) {
                    if let Some(url) = Self::get_cover_art_url("release", &release.id).await {
                        return Ok(Some(url));
                    }
                }
            }
        }

        // If no release group covers found, try direct release search
        let mut builder = ReleaseSearchQuery::query_builder();
        builder.release(album.as_ref());
        if let Some(artist) = artists.first() {
            builder.and().artist(artist.as_ref());
        }

        let results = Release::search(builder.build()).execute().await?;

        for release in results.entities.iter().take(2) {
            if let Some(url) = Self::get_cover_art_url("release", &release.id).await {
                return Ok(Some(url));
            }
        }

        Ok(None)
    }

    // Search by track/recording
    async fn search_track<S: AsRef<str>>(
        &self,
        track: S,
        artists: &[S],
        duration_ms: Option<u128>,
    ) -> Result<Option<String>, CoverArtError> {
        let mut builder = RecordingSearchQuery::query_builder();
        builder.recording(track.as_ref());
        if let Some(artist) = artists.first() {
            builder.and().artist(artist.as_ref());
        }

        if let Some(duration) = duration_ms {
            builder
                .and()
                .duration(format!("[{} TO {}]", duration - 3000, duration + 3000).as_str());
        }

        let results = Recording::search(builder.build())
            .with_releases()
            .execute()
            .await?;

        for recording in results.entities.iter().take(3) {
            if let Some(releases) = &recording.releases {
                for release in releases.iter().take(2) {
                    // Try release cover
                    if let Some(url) = Self::get_cover_art_url("release", &release.id).await {
                        return Ok(Some(url));
                    }

                    // Try release group cover
                    if let Some(rg) = &release.release_group {
                        if let Some(url) = Self::get_cover_art_url("release-group", &rg.id).await {
                            return Ok(Some(url));
                        }
                    }
                }
            }
        }

        Ok(None)
    }
}

#[async_trait]
impl CoverArtProvider for MusicbrainzProvider {
    fn name(&self) -> &'static str {
        "musicbrainz"
    }

    async fn get_cover_url(&self, metadata: &Metadata) -> Result<Option<String>, CoverArtError> {
        let artists = metadata.artists().unwrap_or_default();
        let artists = artists.as_slice();
        let album_artists = metadata.album_artists().unwrap_or_default();
        let album_artists = album_artists.as_slice();

        // Try album search first if we have album metadata
        if let Some(album) = metadata.album_name() {
            let search_artists = if !album_artists.is_empty() {
                album_artists
            } else {
                artists
            };

            if !search_artists.is_empty() {
                if let Some(url) = self.search_album(album, search_artists).await? {
                    return Ok(Some(url));
                }
            }
        }

        // Fall back to track search
        if let Some(title) = metadata.title() {
            if !artists.is_empty() {
                let duration = metadata.length().map(|d| d.as_millis());
                if let Some(url) = self.search_track(title, artists, duration).await? {
                    return Ok(Some(url));
                }
            }
        }

        Ok(None)
    }
}

#[derive(Clone)]
struct ImgbbProvider {
    api_key: String,
    local_utils: LocalUtils,
}

impl ImgbbProvider {
    pub fn new(api_key: String) -> Self {
        Self { 
            api_key,
            local_utils: LocalUtils::new(vec![
                "cover".to_string(),
                "folder".to_string(),
                "album".to_string(),
            ]),
        }
    }
}

#[async_trait]
impl CoverArtProvider for ImgbbProvider {
    fn name(&self) -> &'static str {
        "imgbb"
    }

    async fn get_cover_url(&self, metadata: &Metadata) -> Result<Option<String>, CoverArtError> {
        // If API key is empty, provider is disabled
        if self.api_key.is_empty() {
            debug!("ImgBB provider is disabled (no API key configured)");
            return Ok(None);
        }

        // Try to find a local file using LocalUtils
        if let Some(source) = self.local_utils.find_cover_art(metadata).await? {
            match source {
                CoverArtSource::LocalFile(path) => {
                    debug!("ImgBB upload not implemented yet for: {:?}", path);
                    Ok(None)
                }
                CoverArtSource::HttpUrl(url) => Ok(Some(url)),
            }
        } else {
            Ok(None)
        }
    }
}
