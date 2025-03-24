use log::{debug, info, trace, warn, error};
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
        info!("Initializing cover art manager");
        let cover_config = config.cover_config();

        trace!("Creating cache with 24-hour TTL");
        let cache = CoverCache::new(Duration::from_secs(24 * 60 * 60))?;
        let mut providers: Vec<Box<dyn CoverArtProvider>> = Vec::new();

        debug!("Configuring cover art providers");
        for provider_name in &cover_config.provider.provider {
            trace!("Processing provider configuration: {}", provider_name);
            match provider_name.as_str() {
                "musicbrainz" => {
                    debug!("Adding MusicBrainz provider");
                    providers.push(Box::new(providers::musicbrainz::MusicbrainzProvider::new()));
                }
                "imgbb" => {
                    let imgbb_config = &cover_config.provider.imgbb;
                    if let Some(_) = &imgbb_config.api_key {
                        debug!("Adding ImgBB provider with configured API key");
                        providers.push(Box::new(providers::imgbb::ImgbbProvider::with_config(
                                imgbb_config.clone(),
                            )));
                    } else {
                        warn!("Skipping ImgBB provider - no API key configured");
                    }
                }
                unknown => {
                    warn!("Skipping unknown cover art provider: {}", unknown);
                }
            }
        }

        if providers.is_empty() {
            warn!("No cover art providers have been configured");
        } else {
            debug!("Successfully configured {} provider(s)", providers.len());
        }

        Ok(Self { providers, cache })
    }

    /// Get a cover art URL from available sources and providers
    pub async fn get_cover_art(
        &self,
        metadata: &Metadata,
    ) -> Result<Option<String>, error::CoverArtError> {
        trace!("Starting cover art retrieval process");

        // Check cache first
        if let Some(url) = self.cache.get(metadata)? {
            debug!("Found cached cover art URL");
            trace!("Using cached URL: {}", url);
            return Ok(Some(url));
        }

        trace!("No cache hit, extracting art source from metadata");
        let source = extract_from_metadata(metadata)?;

        match source {
            Some(ArtSource::DirectUrl(url)) => {
                debug!("Using direct URL from metadata");
                trace!("Direct URL: {}", url);
                self.cache.store(metadata, "direct", &url)?;
                Ok(Some(url))
            }
            Some(ArtSource::LocalFile(path)) => {
                debug!("Found local file cover art");
                trace!("Attempting to load file: {:?}", path);
                if let Some(bytes) = load_file(path).await? {
                    debug!("Successfully loaded local file, processing with providers");
                    self.process_with_providers(bytes, metadata).await
                } else {
                    warn!("Failed to load local cover art file");
                    Ok(None)
                }
            }
            Some(source) => {
                debug!("Processing cover art source with available providers");
                self.process_with_providers(source, metadata).await
            }
            None => {
                debug!("No cover art source found in metadata");
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
        trace!("Processing cover art with {} provider(s)", self.providers.len());

        for provider in &self.providers {
            if provider.supports_source_type(&source) {
                debug!("Attempting cover art retrieval with provider: {}", provider.name());

                match provider.process(source.clone(), metadata).await {
                    Ok(Some(result)) => {
                        info!("Successfully retrieved cover art from {}", provider.name());
                        trace!("Cover art URL: {}", result.url);
                        self.cache.store(metadata, &result.provider, &result.url)?;
                        return Ok(Some(result.url));
                    }
                    Ok(None) => {
                        debug!("Provider {} found no cover art", provider.name());
                    }
                    Err(e) => {
                        warn!("Provider {} failed to process cover art: {}", provider.name(), e);
                    }
                }
            } else {
                trace!("Provider {} does not support source type", provider.name());
            }
        }

        debug!("No cover art found from any provider");
        Ok(None)
    }
}

/// Clean up cache periodically
pub async fn clean_cache() -> Result<(), error::CoverArtError> {
    info!("Starting periodic cache cleanup");
    trace!("Creating cache instance for cleanup");
    let cache = CoverCache::new(Duration::from_secs(24 * 60 * 60))?;

    let cleaned = cache.clean()?;
    if cleaned > 0 {
        info!("Successfully cleaned {} expired cache entries", cleaned);
    } else {
        debug!("No expired cache entries found during cleanup");
    }

    Ok(())
}

