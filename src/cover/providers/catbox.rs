use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use catbox::{file, litter};
use image::{imageops::FilterType, ImageFormat};
use log::{debug, info, trace, warn};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::fs;
use tokio::task::spawn_blocking;

use crate::config::schema::CatboxConfig;
use crate::cover::error::CoverArtError;
use crate::cover::sources::ArtSource;
use crate::metadata::MetadataSource;
use tokio_util::sync::CancellationToken;

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

        let raw = if self.config.use_litter {
            litter::upload(&path_str, self.litter_time())
                .await
                .map_err(|e| {
                    CoverArtError::provider_error(self.provider_label(), &format!("{e}"))
                })?
        } else {
            let hash = self.config.user_hash.clone();
            file::from_file(path_str.clone(), hash)
                .await
                .map_err(|e| CoverArtError::provider_error("catbox", &format!("{e}")))?
        };
        Self::validate_upload_response(self.provider_label(), &raw)?;
        Ok(raw)
    }

    /// The `catbox` crate returns the raw HTTP response body unchecked, so a
    /// 504/5xx HTML error page or any other non-URL payload would be stored
    /// as the cover-art URL and pushed to Discord. Reject anything that
    /// doesn't look like a small HTTPS URL.
    fn validate_upload_response(provider: &'static str, raw: &str) -> Result<(), CoverArtError> {
        const MAX_URL_LEN: usize = 512;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(CoverArtError::provider_error(
                provider,
                "upload returned empty body",
            ));
        }
        if trimmed.len() > MAX_URL_LEN {
            return Err(CoverArtError::provider_error(
                provider,
                &format!(
                    "upload returned non-URL payload ({} bytes); likely an HTML error page",
                    trimmed.len()
                ),
            ));
        }
        if !(trimmed.starts_with("https://") || trimmed.starts_with("http://")) {
            return Err(CoverArtError::provider_error(
                provider,
                &format!(
                    "upload returned non-URL response: '{}'",
                    trimmed.chars().take(80).collect::<String>()
                ),
            ));
        }
        if url::Url::parse(trimmed).is_err() {
            return Err(CoverArtError::provider_error(
                provider,
                &format!(
                    "upload returned unparseable URL: '{}'",
                    trimmed.chars().take(80).collect::<String>()
                ),
            ));
        }
        Ok(())
    }

    async fn upload_from_bytes(
        &self,
        data: &[u8],
        cancel: &CancellationToken,
    ) -> Result<String, CoverArtError> {
        let prepared = self.prepare_upload_bytes(data.to_vec(), cancel).await?;
        let temp_path = Self::temp_file_path();
        trace!(
            "Writing {} bytes to temporary file for Catbox upload: {:?}",
            prepared.len(),
            temp_path
        );

        fs::write(&temp_path, &prepared).await.map_err(|e| {
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

    /// Decode + resize if larger than `MAX_DIM` on either axis or oversized
    /// in bytes. Skips work for small images. Re-encodes as JPEG q=85 because
    /// album covers don't need alpha and JPEG shrinks far better than PNG.
    /// CPU-bound; runs on a blocking task so the async runtime stays free.
    async fn prepare_upload_bytes(
        &self,
        bytes: Vec<u8>,
        cancel: &CancellationToken,
    ) -> Result<Vec<u8>, CoverArtError> {
        const MAX_DIM: u32 = 512;
        const MAX_BYTES: usize = 256 * 1024;

        if bytes.len() <= MAX_BYTES {
            // Cheap path: trust small files — quick check would have to decode
            // anyway, and album cover thumbnails are commonly already small.
            return Ok(bytes);
        }

        let provider = self.provider_label();
        let original_len = bytes.len();

        if cancel.is_cancelled() {
            debug!("{} image resize cancelled", provider);
            return Err(CoverArtError::other("cancelled"));
        }

        spawn_blocking(move || -> Result<Vec<u8>, CoverArtError> {
            let img = image::load_from_memory(&bytes).map_err(|e| {
                CoverArtError::provider_error(provider, &format!("image decode failed: {e}"))
            })?;
            let (w, h) = (img.width(), img.height());

            if w <= MAX_DIM && h <= MAX_DIM {
                return Ok(bytes);
            }

            let resized = img.resize(MAX_DIM, MAX_DIM, FilterType::Lanczos3);
            let (nw, nh) = (resized.width(), resized.height());
            let mut out: Vec<u8> = Vec::with_capacity(64 * 1024);
            let mut cursor = Cursor::new(&mut out);
            resized
                .into_rgb8()
                .write_to(&mut cursor, ImageFormat::Jpeg)
                .map_err(|e| {
                    CoverArtError::provider_error(provider, &format!("image re-encode failed: {e}"))
                })?;
            debug!(
                "{} resized cover art {}x{} ({} B) -> {}x{} ({} B JPEG)",
                provider,
                w,
                h,
                original_len,
                nw,
                nh,
                out.len()
            );
            Ok(out)
        })
        .await
        .map_err(|e| {
            CoverArtError::provider_error(self.provider_label(), &format!("resize task: {e}"))
        })?
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
        cancel: &CancellationToken,
    ) -> Result<Option<CoverResult>, CoverArtError> {
        if cancel.is_cancelled() {
            debug!("{} provider cancelled before upload", self.name());
            return Ok(None);
        }
        debug!("Processing cover art with {} provider", self.name());

        let url = match source {
            ArtSource::File(path) => {
                // Route through the bytes path so resize/recompression applies
                // uniformly regardless of whether the source was a file or
                // inline bytes from MPRIS.
                let bytes = fs::read(&path).await.map_err(|e| {
                    CoverArtError::provider_error(
                        self.provider_label(),
                        &format!("read {:?}: {e}", path),
                    )
                })?;
                Some(self.upload_from_bytes(&bytes, cancel).await?)
            }
            ArtSource::Bytes(data) => Some(self.upload_from_bytes(&data, cancel).await?),
            ArtSource::Base64(data) => {
                let bytes = self.base64_to_bytes(&data)?;
                Some(self.upload_from_bytes(&bytes, cancel).await?)
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
