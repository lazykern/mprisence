use log::{debug, info, trace, warn};
use mpris::Metadata;
use std::sync::Arc;
use std::time::Duration;

use crate::config;

pub mod cache;
pub mod error;
pub mod providers;
pub mod sources;

use cache::CoverCache;
use error::CoverArtError;
use providers::CoverArtProvider;
use sources::ArtSource;

const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60); // 24 hours

/// Main manager for cover art retrieval
pub struct CoverManager {
    providers: Vec<Box<dyn CoverArtProvider>>,
    cache: CoverCache,
}

impl CoverManager {
    pub fn new(config: &Arc<config::ConfigManager>) -> Result<Self, CoverArtError> {
        info!("Initializing cover art manager");
        let cover_config = config.cover_config();
        let cache = CoverCache::new(CACHE_TTL)?;
        let mut providers: Vec<Box<dyn CoverArtProvider>> = Vec::new();

        // Initialize configured providers
        for provider_name in &cover_config.provider.provider {
            match provider_name.as_str() {
                "musicbrainz" => {
                    debug!("Adding MusicBrainz provider");
                    providers.push(Box::new(providers::musicbrainz::MusicbrainzProvider::new()));
                }
                "imgbb" => {
                    if let Some(_) = &cover_config.provider.imgbb.api_key {
                        debug!("Adding ImgBB provider");
                        providers.push(Box::new(providers::imgbb::ImgbbProvider::with_config(
                            cover_config.provider.imgbb.clone(),
                        )));
                    } else {
                        warn!("Skipping ImgBB provider - no API key configured");
                    }
                }
                unknown => warn!("Skipping unknown provider: {}", unknown),
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
        source: ArtSource,
        metadata: &Metadata,
    ) -> Result<Option<String>, CoverArtError> {
        if let Some(url) = self.cache.get(metadata)? {
            debug!("Found cached cover art URL");
            return Ok(Some(url));
        }

        if let ArtSource::Url(url) = &source {
            debug!("Using direct URL from source");
            self.cache.store(metadata, "direct", url)?;
            return Ok(Some(url.clone()));
        }

        for provider in &self.providers {
            if !provider.supports_source_type(&source) {
                trace!("Provider {} does not support source type", provider.name());
                continue;
            }

            debug!("Attempting cover art retrieval with {}", provider.name());
            match provider.process(source.clone(), metadata).await {
                Ok(Some(result)) => {
                    info!("Successfully retrieved cover art from {}", provider.name());
                    self.cache.store(metadata, &result.provider, &result.url)?;
                    return Ok(Some(result.url));
                }
                Ok(None) => debug!("Provider {} found no cover art", provider.name()),
                Err(e) => warn!("Provider {} failed: {}", provider.name(), e),
            }
        }

        debug!("No cover art found from any provider");
        Ok(None)
    }
}

/// Clean up cache periodically
pub async fn clean_cache() -> Result<(), CoverArtError> {
    info!("Starting periodic cache cleanup");
    let cache = CoverCache::new(CACHE_TTL)?;
    
    let cleaned = cache.clean()?;
    if cleaned > 0 {
        info!("Cleaned {} expired cache entries", cleaned);
    }

    Ok(())
}

