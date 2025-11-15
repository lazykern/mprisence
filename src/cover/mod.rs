use log::{debug, info, trace, warn};
use reqwest::StatusCode;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use url::{Host, Url};

use crate::config;
use crate::metadata::MetadataSource;

pub mod cache;
pub mod error;
pub mod providers;
pub mod sources;

use cache::CoverCache;
use error::CoverArtError;
use providers::{create_shared_client, CoverArtProvider, CoverResult};
use sources::{search_local_cover_art, ArtSource};

const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours

pub struct CoverManager {
    providers: Vec<Box<dyn CoverArtProvider>>,
    cache: CoverCache,
    config: Arc<config::ConfigManager>,
}

impl CoverManager {
    pub fn new(config: &Arc<config::ConfigManager>) -> Result<Self, CoverArtError> {
        info!("Initializing cover art manager");
        let cover_config = config.cover_config();
        let cache = CoverCache::new(CACHE_TTL)?;
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
        source: ArtSource,
        metadata_source: &MetadataSource,
    ) -> Result<Option<String>, CoverArtError> {
        // 1. Check Cache
        if let Some(url) = self.cache.get(metadata_source)? {
            debug!("Found cached cover art URL: {}", url);
            return Ok(Some(url));
        }
        trace!("No valid cache entry found.");

        // Prepare a potentially transformed source for providers
        let mut source_for_providers = source.clone();

        // 2. If we have a direct URL, decide whether to use it or transform
        if let ArtSource::Url(ref url) = source {
            if Self::is_local_or_private_url(url) {
                warn!("Detected local/private URL; will not use directly: {}", url);
                // Try to fetch bytes so providers like ImgBB can upload
                let client = create_shared_client();
                match client.get(url).send().await {
                    Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                        Ok(bytes) => {
                            debug!("Fetched image bytes from local URL ({} bytes)", bytes.len());
                            source_for_providers = ArtSource::Bytes(bytes.to_vec());
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
                self.cache.store(metadata_source, "direct", url, None)?;
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
                if let Ok(Some(art_source)) = search_local_cover_art(
                    &parent.to_path_buf(),
                    &cover_config.file_names,
                    cover_config.local_search_depth,
                ) {
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
                                    self.cache.store(
                                        metadata_source,
                                        &provider_name,
                                        &url,
                                        expiration,
                                    )?;
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
                    self.cache
                        .store(metadata_source, &provider_name, &url, expiration)?;
                    return Ok(Some(url));
                }
                Ok(None) => debug!("Provider {} found no cover art", provider.name()),
                Err(e) => warn!("Provider {} failed: {}", provider.name(), e),
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
}

pub async fn clean_cache() -> Result<(), CoverArtError> {
    info!("Starting periodic cache cleanup");
    let cache = CoverCache::new(CACHE_TTL)?;

    let cleaned = cache.clean()?;
    if cleaned > 0 {
        info!("Cleaned {} expired cache entries", cleaned);
    }

    Ok(())
}
