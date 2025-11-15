use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use catbox::{file, litter};
use log::{debug, info, trace, warn};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs;

use crate::config::schema::CatboxConfig;
use crate::cover::error::CoverArtError;
use crate::cover::sources::ArtSource;
use crate::metadata::MetadataSource;

use super::{CoverArtProvider, CoverResult};

pub struct CatboxProvider {
    config: CatboxConfig,
}

impl CatboxProvider {
    pub fn with_config(config: CatboxConfig) -> Self {
        info!("Initializing Catbox provider");
        Self { config }
    }

    fn provider_label(&self) -> &'static str {
        if self.config.use_litter {
            "litterbox"
        } else {
            "catbox"
        }
    }

    fn litter_time(&self) -> u8 {
        match self.config.litter_hours {
            1 => 1,
            12 => 12,
            24 => 24,
            72 => 72,
            invalid => {
                warn!(
                    "Invalid litter duration ({invalid}h). Falling back to 24 hours (allowed: 1, 12, 24, 72)."
                );
                24
            }
        }
    }

    async fn upload_from_path(&self, path: &Path) -> Result<String, CoverArtError> {
        let path_str = path.to_string_lossy().to_string();
        trace!(
            "Uploading cover art via {} from {:?}",
            self.provider_label(),
            path
        );

        if self.config.use_litter {
            litter::upload(&path_str, self.litter_time())
                .await
                .map_err(|e| CoverArtError::provider_error(self.provider_label(), &format!("{e}")))
        } else {
            let hash = self.config.user_hash.clone();
            file::from_file(path_str.clone(), hash)
                .await
                .map_err(|e| CoverArtError::provider_error("catbox", &format!("{e}")))
        }
    }

    async fn upload_from_bytes(&self, data: &[u8]) -> Result<String, CoverArtError> {
        let temp_path = Self::temp_file_path();
        trace!(
            "Writing {} bytes to temporary file for Catbox upload: {:?}",
            data.len(),
            temp_path
        );

        fs::write(&temp_path, data).await.map_err(|e| {
            CoverArtError::provider_error(self.provider_label(), &format!("write failed: {e}"))
        })?;

        let result = self.upload_from_path(&temp_path).await;
        if let Err(e) = fs::remove_file(&temp_path).await {
            debug!(
                "Failed to remove temporary Catbox upload file {:?}: {}",
                temp_path, e
            );
        }
        result
    }

    fn temp_file_path() -> PathBuf {
        let mut path = std::env::temp_dir();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|dur| dur.as_nanos())
            .unwrap_or(0);
        let pid = std::process::id();
        path.push(format!("mprisence-catbox-{pid}-{timestamp}.img"));
        path
    }

    fn base64_to_bytes(&self, data: &str) -> Result<Vec<u8>, CoverArtError> {
        STANDARD
            .decode(data.as_bytes())
            .map_err(|e| CoverArtError::provider_error(self.provider_label(), &format!("{e}")))
    }
}

#[async_trait]
impl CoverArtProvider for CatboxProvider {
    fn name(&self) -> &'static str {
        self.provider_label()
    }

    fn supports_source_type(&self, source: &ArtSource) -> bool {
        matches!(
            source,
            ArtSource::File(_) | ArtSource::Bytes(_) | ArtSource::Base64(_)
        )
    }

    async fn process(
        &self,
        source: ArtSource,
        _metadata_source: &MetadataSource,
    ) -> Result<Option<CoverResult>, CoverArtError> {
        debug!("Processing cover art with {} provider", self.name());

        let url = match source {
            ArtSource::File(path) => Some(self.upload_from_path(&path).await?),
            ArtSource::Bytes(data) => Some(self.upload_from_bytes(&data).await?),
            ArtSource::Base64(data) => {
                let bytes = self.base64_to_bytes(&data)?;
                Some(self.upload_from_bytes(&bytes).await?)
            }
            ArtSource::Url(_) => return Ok(None),
        };

        if let Some(ref url) = url {
            info!("{} provided hosted cover art: {}", self.name(), url);
        }

        let expiration = if self.config.use_litter {
            Some(Duration::from_secs(self.litter_time() as u64 * 60 * 60))
        } else {
            None
        };

        Ok(url.map(|url| CoverResult {
            url,
            provider: self.name().to_string(),
            expiration,
        }))
    }
}
