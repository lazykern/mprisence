use async_trait::async_trait;
use reqwest::{header, Client};
use std::time::Duration;

use crate::cover::error::CoverArtError;
use crate::cover::sources::ArtSource;
use crate::metadata::MetadataSource;

pub mod catbox;
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

#[derive(Debug)]
pub struct CoverResult {
    pub url: String,
    pub provider: String,
    #[allow(dead_code)]
    pub expiration: Option<Duration>,
}

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
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(0)
        .pool_idle_timeout(Some(Duration::from_secs(30)))
        .tcp_keepalive(Some(Duration::from_secs(60)))
        .tcp_nodelay(true)
        .http2_initial_stream_window_size(Some(65535))
        .http2_initial_connection_window_size(Some(131072))
        .default_headers(headers)
        .build()
        .expect("Failed to create HTTP client")
}

#[async_trait]
pub trait CoverArtProvider: Send + Sync {
    fn name(&self) -> &'static str;

    fn supports_source_type(&self, source: &ArtSource) -> bool;

    fn supports_metadata_only(&self) -> bool {
        false
    }

    async fn process(
        &self,
        source: ArtSource,
        metadata_source: &MetadataSource,
    ) -> Result<Option<CoverResult>, CoverArtError>;
}
