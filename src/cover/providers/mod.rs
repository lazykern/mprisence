use async_trait::async_trait;
use mpris::Metadata;
use std::time::Duration;

use crate::cover::error::CoverArtError;
use crate::cover::sources::ArtSource;

pub mod imgbb;
pub mod musicbrainz;

/// Result from a cover art provider
pub struct CoverResult {
    /// The URL where the cover art can be accessed
    pub url: String,
    /// The name of the provider that generated this result
    pub provider: String,
    /// Optional expiration time for the URL
    pub expiration: Option<Duration>,
}

/// Trait defining the interface for cover art providers
#[async_trait]
pub trait CoverArtProvider: Send + Sync {
    /// Get the provider name
    fn name(&self) -> &'static str;
    
    /// Check if this provider supports the given source type
    fn supports_source_type(&self, source: &ArtSource) -> bool;
    
    /// Process a source to get a URL
    async fn process(
        &self, 
        source: ArtSource, 
        metadata: &Metadata
    ) -> Result<Option<CoverResult>, CoverArtError>;
}
