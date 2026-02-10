use log::{debug, info, trace, warn};
use reqwest::StatusCode;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::task::{spawn_blocking, JoinError};
use url::{Host, Url};

use crate::config;
use crate::metadata::MetadataSource;

pub mod cache;
pub mod error;
pub mod providers;
pub mod sources;

use cache::{CacheEntry, CoverCache, MAX_CACHED_IMAGE_BYTES};
use error::CoverArtError;
use providers::{create_shared_client, CoverArtProvider, CoverResult};
use sources::{search_local_cover_art, ArtSource};

const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours
const CACHE_VALIDATION_INTERVAL: Duration = Duration::from_secs(60 * 60); // 1 hour

pub struct CoverManager {
    providers: Vec<Box<dyn CoverArtProvider>>,
    cache: Arc<CoverCache>,
    config: Arc<config::ConfigManager>,
}

impl CoverManager {
    pub fn new(config: &Arc<config::ConfigManager>) -> Result<Self, CoverArtError> {
        info!("Initializing cover art manager");
        let cover_config = config.cover_config();
        let cache = Arc::new(CoverCache::new(CACHE_TTL)?);
        let mut providers: Vec<Box<dyn CoverArtProvider>> = Vec::new();

        for provider_name in &cover_config.provider.provider {
            match provider_name.as_str() {
                "musicbrainz" => {
                    debug!("Adding MusicBrainz provider");
                    providers.push(Box::new(
                        providers::musicbrainz::MusicbrainzProvider::with_config(
                            cover_config.provider.musicbrainz.clone(),
                        ),
                    ));
                }
                "imgbb" => {
                    if cover_config.provider.imgbb.api_key.is_some() {
                        debug!("Adding ImgBB provider");
                        providers.push(Box::new(providers::imgbb::ImgbbProvider::with_config(
                            cover_config.provider.imgbb.clone(),
                        )));
                    } else {
                        warn!("Skipping ImgBB provider - no API key configured");
                    }
                }
                "catbox" => {
                    debug!("Adding Catbox provider");
                    providers.push(Box::new(providers::catbox::CatboxProvider::with_config(
                        cover_config.provider.catbox.clone(),
                    )));
                }
                unknown => warn!("Skipping unknown provider: {}", unknown),
            }
        }

        if providers.is_empty() {
            warn!("No cover art providers configured");
        }

        Ok(Self {
            providers,
            cache,
            config: config.clone(),
        })
    }

    fn is_local_or_private_url(url_str: &str) -> bool {
        if let Ok(parsed) = Url::parse(url_str) {
            if let Some(host) = parsed.host() {
                match host {
                    Host::Domain(d) => {
                        let dl = d.to_ascii_lowercase();
                        if dl == "localhost" || dl.ends_with(".localhost") {
                            return true;
                        }
                    }
                    Host::Ipv4(ip) => {
                        if ip.is_loopback()
                            || ip.is_private()
                            || ip.is_link_local()
                            || ip.is_unspecified()
                        {
                            return true;
                        }
                    }
                    Host::Ipv6(ip) => {
                        if ip.is_loopback()
                            || ip.is_unique_local()
                            || ip.is_unicast_link_local()
                            || ip.is_unspecified()
                        {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    pub async fn get_cover_art(
        &self,
        source: Option<ArtSource>,
        metadata_source: &MetadataSource,
    ) -> Result<Option<String>, CoverArtError> {
        let cache_key = CoverCache::generate_key(metadata_source);
        let recovered_cache_bytes: Option<Vec<u8>>;

        // 1. Check Cache
        if let Some(mut entry) = self.cache_get_entry(&cache_key).await? {
            let url = entry.url.clone();
            let needs_validation = entry
                .last_validated
                .elapsed()
                .map(|elapsed| elapsed >= CACHE_VALIDATION_INTERVAL)
                .unwrap_or(true);

            if !needs_validation || Self::validate_cover_url(&url).await {
                if needs_validation {
                    entry.last_validated = SystemTime::now();
                    self.cache_update_entry(&cache_key, &entry).await?;
                }
                debug!(
                    "Serving cached cover art (provider: {}, validated: {})",
                    entry.provider, !needs_validation
                );
                return Ok(Some(url));
            } else {
                warn!(
                    "Cached cover art URL {} failed validation; removing entry",
                    url
                );
                recovered_cache_bytes = self.cache_load_bytes(entry).await?;
                self.cache_remove_entry(&cache_key).await?;
            }
        } else {
            recovered_cache_bytes = None;
        }
        trace!("No valid cache entry found.");

        if source.is_none() {
            debug!(
                "No art source found in metadata; trying local search and metadata-only providers"
            );
        }

        // Prepare a potentially transformed source for providers
        let mut source_for_providers = match &recovered_cache_bytes {
            Some(bytes) => Some(ArtSource::Bytes(bytes.clone())),
            None => source.clone(),
        };

        // 2. If we have a direct URL, decide whether to use it or transform
        if let Some(ArtSource::Url(ref url)) = source.as_ref() {
            if Self::is_local_or_private_url(url) {
                warn!("Detected local/private URL; will not use directly: {}", url);
                // Try to fetch bytes so providers like ImgBB can upload
                let client = create_shared_client();
                match client.get(url).send().await {
                    Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                        Ok(bytes) => {
                            debug!("Fetched image bytes from local URL ({} bytes)", bytes.len());
                            source_for_providers = Some(ArtSource::Bytes(bytes.to_vec()));
                        }
                        Err(e) => {
                            warn!("Failed to read bytes from local URL response: {}", e);
                        }
                    },
                    Ok(resp) => {
                        warn!(
                            "Local URL fetch returned non-success status: {}",
                            resp.status()
                        );
                    }
                    Err(e) => {
                        warn!("Failed to fetch local URL: {}", e);
                    }
                }
            } else if Self::validate_cover_url(url).await {
                debug!("Using direct URL from source: {}", url);
                let cache_payload = match source.as_ref() {
                    Some(source) => Self::prepare_cache_payload(source, url).await?,
                    None => None,
                };
                self.cache_store_entry(&cache_key, "direct", url, None, cache_payload)
                    .await?;
                return Ok(Some(url.clone()));
            } else {
                warn!(
                    "Direct cover art URL {} failed validation; trying configured providers",
                    url
                );
            }
        }

        // 3. Try to find local cover art if we have a file path
        if let Some(path) = metadata_source.url().and_then(|url| {
            if url.starts_with("file://") {
                match urlencoding::decode(&url[7..]) {
                    Ok(dec) => Some(PathBuf::from(dec.into_owned())),
                    Err(_) => return None,
                }
            } else {
                None
            }
        }) {
            if let Some(parent) = path.parent() {
                debug!("Attempting to find local cover art in: {:?}", parent);
                let cover_config = self.config.cover_config();
                let file_names = cover_config.file_names.clone();
                let search_root = parent.to_path_buf();
                let search_depth = cover_config.local_search_depth;
                let local_art_result = spawn_blocking(move || {
                    search_local_cover_art(&search_root, &file_names, search_depth)
                })
                .await
                .map_err(|e| {
                    CoverArtError::other(format!("Local cover art search failed: {}", e))
                })?;
                let local_art = local_art_result?;

                if let Some(art_source) = local_art {
                    // Process the found local cover art through providers
                    for provider in &self.providers {
                        if provider.supports_source_type(&art_source) {
                            debug!("Processing local cover art with {}", provider.name());
                            match provider.process(art_source.clone(), metadata_source).await {
                                Ok(Some(result)) => {
                                    let CoverResult {
                                        url,
                                        provider: provider_name,
                                        expiration,
                                    } = result;
                                    if !Self::validate_cover_url(&url).await {
                                        warn!(
                                            "Provider {} returned invalid cover art URL, skipping",
                                            provider_name
                                        );
                                        continue;
                                    }
                                    info!(
                                        "Successfully processed local cover art with {}",
                                        provider_name
                                    );
                                    let cache_payload =
                                        Self::prepare_cache_payload(&art_source, &url).await?;
                                    self.cache_store_entry(
                                        &cache_key,
                                        &provider_name,
                                        &url,
                                        expiration,
                                        cache_payload,
                                    )
                                    .await?;
                                    return Ok(Some(url));
                                }
                                Ok(None) => debug!(
                                    "Provider {} could not process local cover art",
                                    provider.name()
                                ),
                                Err(e) => warn!(
                                    "Provider {} failed to process local cover art: {}",
                                    provider.name(),
                                    e
                                ),
                            }
                        }
                    }
                }
            }
        }

        // 4. Try configured providers with the prepared source
        if let Some(source_for_providers) = source_for_providers {
            for provider in &self.providers {
                if !provider.supports_source_type(&source_for_providers) {
                    trace!("Provider {} does not support source type", provider.name());
                    continue;
                }

                debug!("Attempting cover art retrieval with {}", provider.name());
                match provider
                    .process(source_for_providers.clone(), metadata_source)
                    .await
                {
                    Ok(Some(result)) => {
                        let CoverResult {
                            url,
                            provider: provider_name,
                            expiration,
                        } = result;
                        if !Self::validate_cover_url(&url).await {
                            warn!(
                                "Provider {} returned invalid cover art URL, skipping",
                                provider_name
                            );
                            continue;
                        }
                        info!("Successfully retrieved cover art from {}", provider_name);
                        let cache_payload =
                            Self::prepare_cache_payload(&source_for_providers, &url).await?;
                        self.cache_store_entry(
                            &cache_key,
                            &provider_name,
                            &url,
                            expiration,
                            cache_payload,
                        )
                        .await?;
                        return Ok(Some(url));
                    }
                    Ok(None) => debug!("Provider {} found no cover art", provider.name()),
                    Err(e) => warn!("Provider {} failed: {}", provider.name(), e),
                }
            }
        } else {
            let metadata_only_source = ArtSource::Url(String::new());
            for provider in &self.providers {
                if !provider.supports_metadata_only() {
                    trace!(
                        "Provider {} does not support metadata-only lookup",
                        provider.name()
                    );
                    continue;
                }

                debug!(
                    "Attempting cover art retrieval with {} using metadata only",
                    provider.name()
                );
                match provider
                    .process(metadata_only_source.clone(), metadata_source)
                    .await
                {
                    Ok(Some(result)) => {
                        let CoverResult {
                            url,
                            provider: provider_name,
                            expiration,
                        } = result;
                        if !Self::validate_cover_url(&url).await {
                            warn!(
                                "Provider {} returned invalid cover art URL, skipping",
                                provider_name
                            );
                            continue;
                        }
                        info!("Successfully retrieved cover art from {}", provider_name);
                        let cache_payload = Self::prepare_cache_payload(
                            &ArtSource::Url(url.clone()),
                            &url,
                        )
                        .await?;
                        self.cache_store_entry(
                            &cache_key,
                            &provider_name,
                            &url,
                            expiration,
                            cache_payload,
                        )
                        .await?;
                        return Ok(Some(url));
                    }
                    Ok(None) => debug!("Provider {} found no cover art", provider.name()),
                    Err(e) => warn!("Provider {} failed: {}", provider.name(), e),
                }
            }
        }

        debug!("No cover art found from any source");
        Ok(None)
    }

    async fn validate_cover_url(url: &str) -> bool {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            trace!("Skipping validation for non-HTTP cover art URL: {}", url);
            return true;
        }

        let client = create_shared_client();

        match client.head(url).send().await {
            Ok(resp) if resp.status().is_success() => {
                trace!("HEAD validation succeeded for cover art URL: {}", url);
                return true;
            }
            Ok(resp) if resp.status() == StatusCode::METHOD_NOT_ALLOWED => {
                trace!("HEAD not allowed for {}, falling back to GET probe", url);
            }
            Ok(resp) => {
                debug!(
                    "HEAD validation failed for {} (status {}), attempting GET probe",
                    url,
                    resp.status()
                );
            }
            Err(e) => {
                debug!(
                    "HEAD validation request failed for {}: {}. Attempting GET probe",
                    url, e
                );
            }
        }

        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => {
                trace!("GET validation succeeded for cover art URL: {}", url);
                true
            }
            Ok(resp) => {
                warn!(
                    "Cover art URL validation failed (status {}) for {}",
                    resp.status(),
                    url
                );
                false
            }
            Err(e) => {
                warn!("Cover art URL validation failed for {}: {}", url, e);
                false
            }
        }
    }

    async fn cache_get_entry(&self, key: &str) -> Result<Option<CacheEntry>, CoverArtError> {
        let cache = self.cache.clone();
        let key = key.to_string();
        let result = spawn_blocking(move || cache.get_by_key(&key))
            .await
            .map_err(|e| Self::cache_task_error("lookup", e))?;
        result
    }

    async fn cache_update_entry(&self, key: &str, entry: &CacheEntry) -> Result<(), CoverArtError> {
        let cache = self.cache.clone();
        let key = key.to_string();
        let entry_clone = entry.clone();
        let result = spawn_blocking(move || cache.update_entry_with_key(&key, &entry_clone))
            .await
            .map_err(|e| Self::cache_task_error("update", e))?;
        result
    }

    async fn cache_store_entry(
        &self,
        key: &str,
        provider: &str,
        url: &str,
        expiration: Option<Duration>,
        cached_bytes: Option<Vec<u8>>,
    ) -> Result<(), CoverArtError> {
        let cache = self.cache.clone();
        let key = key.to_string();
        let provider = provider.to_string();
        let url = url.to_string();
        let result = spawn_blocking(move || {
            let bytes = cached_bytes.as_deref();
            cache.store_with_key(&key, &provider, &url, expiration, bytes)
        })
        .await
        .map_err(|e| Self::cache_task_error("store", e))?;
        result
    }

    async fn cache_remove_entry(&self, key: &str) -> Result<(), CoverArtError> {
        let cache = self.cache.clone();
        let key = key.to_string();
        let result = spawn_blocking(move || cache.remove_by_key(&key))
            .await
            .map_err(|e| Self::cache_task_error("remove", e))?;
        result
    }

    async fn cache_load_bytes(&self, entry: CacheEntry) -> Result<Option<Vec<u8>>, CoverArtError> {
        let cache = self.cache.clone();
        let result = spawn_blocking(move || cache.load_bytes(&entry))
            .await
            .map_err(|e| Self::cache_task_error("load-bytes", e))?;
        result
    }

    fn cache_task_error(context: &str, err: JoinError) -> CoverArtError {
        CoverArtError::other(format!("Cache {context} task failed: {err}"))
    }

    async fn prepare_cache_payload(
        source: &ArtSource,
        url: &str,
    ) -> Result<Option<Vec<u8>>, CoverArtError> {
        if let Some(bytes) = source.materialize_bytes().await? {
            return Ok(Some(bytes));
        }
        Self::materialize_remote_bytes(url).await
    }

    async fn materialize_remote_bytes(url: &str) -> Result<Option<Vec<u8>>, CoverArtError> {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            trace!("Skipping remote byte fetch for non-HTTP URL: {}", url);
            return Ok(None);
        }

        let client = create_shared_client();
        match client.get(url).send().await {
            Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                Ok(bytes) => {
                    if bytes.len() > MAX_CACHED_IMAGE_BYTES {
                        warn!(
                            "Fetched cover art ({}) exceeds cache byte limit ({} bytes > {} bytes)",
                            url,
                            bytes.len(),
                            MAX_CACHED_IMAGE_BYTES
                        );
                        return Ok(None);
                    }
                    Ok(Some(bytes.to_vec()))
                }
                Err(e) => {
                    warn!("Failed to read bytes for {}: {}", url, e);
                    Ok(None)
                }
            },
            Ok(resp) => {
                warn!(
                    "Skipping byte cache for {} due to HTTP status {}",
                    url,
                    resp.status()
                );
                Ok(None)
            }
            Err(e) => {
                warn!("Failed to download {} for caching: {}", url, e);
                Ok(None)
            }
        }
    }
}

pub async fn clean_cache() -> Result<(), CoverArtError> {
    info!("Starting periodic cache cleanup");
    let cache = CoverCache::new(CACHE_TTL)?;
    let cleaned_result = spawn_blocking(move || cache.clean())
        .await
        .map_err(|e| CoverArtError::other(format!("Cache cleanup task failed: {}", e)))?;
    let cleaned = cleaned_result?;
    if cleaned > 0 {
        info!("Cleaned {} expired cache entries", cleaned);
    }

    Ok(())
}
