use log::{debug, info, warn};
use mpris::Metadata;
use std::{sync::Arc, time::Duration};

use crate::config;

pub mod cache;
pub mod error;
pub mod providers;
pub mod sources;

use cache::CoverCache;
use providers::CoverArtProvider;
use sources::{extract_from_metadata, load_file, ArtSource};

/// Main manager for cover art retrieval
pub struct CoverManager {
    providers: Vec<Box<dyn CoverArtProvider>>,
    cache: CoverCache,
}

impl CoverManager {
    pub fn new(config: &Arc<config::ConfigManager>) -> Result<Self, error::CoverArtError> {
        debug!("Creating CoverArtManager");
        let cover_config = config.cover_config();

        // Initialize cache with 24-hour TTL by default
        let cache = CoverCache::new(Duration::from_secs(24 * 60 * 60))?;

        // Initialize providers based on config
        let mut providers: Vec<Box<dyn CoverArtProvider>> = Vec::new();

        for provider_name in &cover_config.provider.provider {
            debug!("Adding provider: {}", provider_name);
            match provider_name.as_str() {
                "musicbrainz" => {
                    providers.push(Box::new(providers::musicbrainz::MusicbrainzProvider::new()));
                }
                "imgbb" => {
                    if let Some(imgbb_config) = &cover_config.provider.imgbb {
                        if let Some(api_key) = &imgbb_config.api_key {
                            // Create ImgBB provider with full configuration
                            let provider_config = providers::imgbb::ImgbbConfig {
                                api_key: api_key.clone(),
                                expiration: imgbb_config.expiration,
                                default_name: imgbb_config.default_name.clone(),
                                // Add default timeout and user agent
                                timeout: Some(30), // 30 seconds timeout
                                user_agent: Some(format!(
                                    "mprisence/{}",
                                    env!("CARGO_PKG_VERSION")
                                )),
                            };

                            providers.push(Box::new(providers::imgbb::ImgbbProvider::with_config(
                                provider_config,
                            )));

                            debug!("Added ImgBB provider with API key");
                        } else {
                            warn!("ImgBB provider is disabled (no API key configured)");
                        }
                    } else {
                        warn!("ImgBB provider is disabled (no configuration)");
                    }
                }
                unknown => warn!("Unknown cover art provider: {}", unknown),
            }
        }

        if providers.is_empty() {
            warn!("No cover art providers configured");
        }

        Ok(Self { providers, cache })
    }

    /// Get a cover art URL from available sources and providers
    pub async fn get_cover_art(
        &self,
        metadata: &Metadata,
    ) -> Result<Option<String>, error::CoverArtError> {
        debug!("Starting cover art retrieval process");

        // First check cache
        if let Some(url) = self.cache.get(metadata)? {
            debug!("Using cached URL: {}", url);
            return Ok(Some(url));
        }

        // Extract source from metadata
        let source = extract_from_metadata(metadata)?;

        match source {
            Some(ArtSource::DirectUrl(url)) => {
                // Direct URL can be used as-is
                info!("Using direct URL from metadata");
                // Cache the direct URL
                self.cache.store(metadata, "direct", &url)?;
                Ok(Some(url))
            }
            Some(ArtSource::LocalFile(path)) => {
                // Load file into memory
                info!("Found local file, loading content");
                if let Some(bytes) = load_file(path).await? {
                    // Process with providers
                    self.process_with_providers(bytes, metadata).await
                } else {
                    info!("Failed to load local file");
                    Ok(None)
                }
            }
            Some(source) => {
                // Process with appropriate providers
                self.process_with_providers(source, metadata).await
            }
            None => {
                info!("No cover art source found in metadata");
                Ok(None)
            }
        }
    }

    /// Process a source through available providers
    async fn process_with_providers(
        &self,
        source: ArtSource,
        metadata: &Metadata,
    ) -> Result<Option<String>, error::CoverArtError> {
        for provider in &self.providers {
            if provider.supports_source_type(&source) {
                info!("Trying provider: {}", provider.name());

                match provider.process(source.clone(), metadata).await {
                    Ok(Some(result)) => {
                        info!("Got URL from provider: {}", provider.name());
                        // Cache the result
                        self.cache.store(metadata, &result.provider, &result.url)?;
                        return Ok(Some(result.url));
                    }
                    Ok(None) => {
                        debug!("Provider {} found no cover art", provider.name());
                    }
                    Err(e) => {
                        warn!("Error from provider {}: {}", provider.name(), e);
                    }
                }
            }
        }

        info!("No cover art found from any provider");
        Ok(None)
    }
}

/// Clean up cache periodically
pub async fn clean_cache() -> Result<(), error::CoverArtError> {
    info!("Starting cache cleanup");

    // Create cache instance with 24-hour TTL (same as CoverArtManager)
    let cache = CoverCache::new(Duration::from_secs(24 * 60 * 60))?;

    let cleaned = cache.clean()?;
    if cleaned > 0 {
        info!("Cleaned {} expired cache entries", cleaned);
    } else {
        debug!("No expired cache entries found");
    }

    Ok(())
}
