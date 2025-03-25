use async_trait::async_trait;
use mpris::Metadata;
use reqwest::{Client, header};
use std::time::Duration;

use crate::cover::error::CoverArtError;
use crate::cover::sources::ArtSource;

pub mod imgbb;
pub mod musicbrainz;

const USER_AGENT: &str = concat!(
    env!("CARGO_PKG_NAME"),
    "/",
    env!("CARGO_PKG_VERSION"),
    " ( ",
    env!("CARGO_PKG_REPOSITORY"),
    " )"
);

/// Result from a cover art provider
#[derive(Debug)]
pub struct CoverResult {
    /// The URL where the cover art can be accessed
    pub url: String,
    /// The name of the provider that generated this result
    pub provider: String,
    /// Optional expiration time for the URL
    pub expiration: Option<Duration>,
}

/// Create a new shared HTTP client with memory-optimized configuration
pub fn create_shared_client() -> Client {
    let mut headers = header::HeaderMap::with_capacity(2);
    headers.insert(
        header::USER_AGENT,
        header::HeaderValue::from_static(USER_AGENT),
    );
    headers.insert(
        header::ACCEPT,
        header::HeaderValue::from_static("application/json"),
    );

    Client::builder()
        // Set reasonable timeouts
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        // Optimize connection pool
        .pool_max_idle_per_host(0)
        .pool_idle_timeout(Some(Duration::from_secs(30)))
        // Minimize memory usage
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .tcp_nodelay(true)
        .http2_initial_stream_window_size(Some(65535))
        .http2_initial_connection_window_size(Some(131072))
        // Set default headers
        .default_headers(headers)
        .build()
        .expect("Failed to create HTTP client")
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
