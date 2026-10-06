use async_trait::async_trait;
use imgbb::model::Data;
use imgbb::ImgBB;
use log::{debug, info, trace, warn};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use super::{CoverArtProvider, CoverResult};
use crate::config::schema::ImgBBConfig;
use crate::cover::error::CoverArtError;
use crate::cover::sources::ArtSource;
use crate::metadata::MetadataSource;
use tokio_util::sync::CancellationToken;

const MIN_HOSTED_LIFETIME_SECS: u64 = 10 * 60;

pub struct ImgbbProvider {
    config: ImgBBConfig,
    client: Arc<ImgBB>,
}

impl ImgbbProvider {
    pub fn with_config(config: ImgBBConfig) -> Self {
        info!("Initializing ImgBB provider");
        let api_key = config.api_key.clone().expect("API key must be provided");
        Self {
            client: Arc::new(ImgBB::new(api_key)),
            config,
        }
    }

    fn generate_image_name(&self, metadata_source: &MetadataSource) -> String {
        let artist = metadata_source
            .artists()
            .and_then(|artists| artists.first().map(ToString::to_string))
            .unwrap_or_default();

        let title = metadata_source.title().unwrap_or_default();

        if artist.is_empty() && title.is_empty() {
            "mprisence_cover".to_string()
        } else if artist.is_empty() {
            title
        } else if title.is_empty() {
            artist
        } else {
            format!("{} - {}", artist, title)
        }
    }

    /// How long the hosted file has left. ImgBB answers a repeat upload of the
    /// same bytes with the existing image and its original upload time, so the
    /// remaining lifetime can be shorter than the configured expiration. It is
    /// never less than ten minutes (or the expiration, if shorter): with the
    /// local clock ahead, or an image near its deadline, the cache would
    /// otherwise drop the URL and upload the cover again right away.
    fn hosted_lifetime(&self, data: Option<&Data>) -> Option<Duration> {
        let expiration = data
            .and_then(|data| data.expiration)
            .unwrap_or(self.config.expiration);
        if expiration == 0 {
            return None;
        }
        let Some(uploaded) = data.and_then(|data| data.time) else {
            return Some(Duration::from_secs(expiration));
        };
        let deleted_at = UNIX_EPOCH + Duration::from_secs(uploaded.saturating_add(expiration));
        let remaining = deleted_at
            .duration_since(SystemTime::now())
            .unwrap_or(Duration::ZERO);
        Some(remaining.max(Duration::from_secs(
            expiration.min(MIN_HOSTED_LIFETIME_SECS),
        )))
    }

    async fn process_source(
        &self,
        source: &ArtSource,
        metadata_source: &MetadataSource,
        cancel: &CancellationToken,
    ) -> Result<Option<CoverResult>, CoverArtError> {
        if cancel.is_cancelled() {
            debug!("ImgBB provider cancelled before upload");
            return Ok(None);
        }
        if self.config.api_key.is_none() {
            warn!("ImgBB provider is disabled (no API key configured)");
            return Ok(None);
        }

        debug!("Processing cover art with ImgBB provider");
        let image_name = self.generate_image_name(metadata_source);
        let mut builder = self.client.upload_builder().name(&image_name);

        if self.config.expiration > 0 {
            trace!(
                "Setting image expiration to {} seconds",
                self.config.expiration
            );
            builder = builder.expiration(self.config.expiration);
        }

        let response = match source {
            ArtSource::Base64(data) => builder.data(data),
            ArtSource::Bytes(data) => builder.bytes(data),
            ArtSource::File(path) => {
                if cancel.is_cancelled() {
                    debug!("ImgBB cancelled before file read");
                    return Ok(None);
                }
                let data = tokio::fs::read(path).await.map_err(|e| {
                    CoverArtError::provider_error("imgbb", &format!("Failed to read file: {}", e))
                })?;
                builder.bytes(&data)
            }
            ArtSource::Url(_) => return Ok(None),
        }
        .upload()
        .await
        .map_err(|e| {
            // The request URL carries the API key as a query parameter.
            let e = match e {
                imgbb::Error::ReqwestError(err) => imgbb::Error::ReqwestError(err.without_url()),
                other => other,
            };
            CoverArtError::provider_error("imgbb", &format!("Upload failed: {}", e))
        })?;

        let expiration = self.hosted_lifetime(response.data.as_ref());
        let url = response.data.and_then(|data| data.url.or(data.display_url));

        match &url {
            Some(url) => {
                info!("Successfully uploaded image to ImgBB");
                trace!("ImgBB provided URL: {}", url);
            }
            None => warn!("ImgBB upload succeeded but no URL was returned"),
        }

        Ok(url.map(|url| CoverResult {
            url,
            provider: "imgbb".to_string(),
            expiration,
        }))
    }
}

#[async_trait]
impl CoverArtProvider for ImgbbProvider {
    fn name(&self) -> &'static str {
        "imgbb"
    }

    fn supports_source_type(&self, source: &ArtSource) -> bool {
        matches!(
            source,
            ArtSource::Base64(_) | ArtSource::File(_) | ArtSource::Bytes(_)
        )
    }

    async fn process(
        &self,
        source: ArtSource,
        metadata_source: &MetadataSource,
        cancel: &CancellationToken,
    ) -> Result<Option<CoverResult>, CoverArtError> {
        self.process_source(&source, metadata_source, cancel).await
    }

    async fn process_borrowed(
        &self,
        source: &ArtSource,
        metadata_source: &MetadataSource,
        cancel: &CancellationToken,
    ) -> Result<Option<CoverResult>, CoverArtError> {
        self.process_source(source, metadata_source, cancel).await
    }
}

#[cfg(test)]
mod tests {
    use super::ImgbbProvider;
    use crate::config::schema::ImgBBConfig;
    use imgbb::model::Data;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    fn provider(expiration: u64) -> ImgbbProvider {
        ImgbbProvider::with_config(ImgBBConfig {
            api_key: Some("test".to_string()),
            expiration,
        })
    }

    fn data(time: u64, expiration: u64) -> Data {
        serde_json::from_value(serde_json::json!({ "time": time, "expiration": expiration }))
            .unwrap()
    }

    #[test]
    fn permanent_upload_has_no_hosted_lifetime() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(provider(0).hosted_lifetime(Some(&data(now, 0))), None);
        assert_eq!(provider(0).hosted_lifetime(None), None);
    }

    #[test]
    fn repeat_upload_keeps_original_deadline() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let lifetime = provider(86400)
            .hosted_lifetime(Some(&data(now - 3600, 86400)))
            .unwrap();
        assert!(lifetime <= Duration::from_secs(82800));
        assert!(lifetime > Duration::from_secs(82700));
    }

    #[test]
    fn nearly_expired_repeat_upload_gets_ten_minutes() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let lifetime = provider(86400)
            .hosted_lifetime(Some(&data(now - 86395, 86400)))
            .unwrap();
        assert_eq!(lifetime, Duration::from_secs(600));
    }

    #[test]
    fn clock_far_ahead_still_gets_ten_minutes() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let lifetime = provider(86400)
            .hosted_lifetime(Some(&data(now - 200_000, 86400)))
            .unwrap();
        assert_eq!(lifetime, Duration::from_secs(600));
    }

    #[test]
    fn short_expiration_is_not_stretched() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let lifetime = provider(120)
            .hosted_lifetime(Some(&data(now - 500, 120)))
            .unwrap();
        assert_eq!(lifetime, Duration::from_secs(120));
    }

    #[test]
    fn missing_upload_time_falls_back_to_configured_expiration() {
        assert_eq!(
            provider(600).hosted_lifetime(None),
            Some(Duration::from_secs(600))
        );
    }
}
