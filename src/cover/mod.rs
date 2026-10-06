use log::{debug, info, trace, warn};
use reqwest::{header, StatusCode};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::Semaphore;
use tokio::task::spawn_blocking;
use tokio_util::sync::CancellationToken;
use url::{Host, Url};

use crate::config;
use crate::metadata::MetadataSource;

pub mod cache;
pub mod error;
mod image;
pub mod providers;
pub mod sources;

#[cfg(test)]
mod duplicate_upload_tests;

use cache::{CacheEntry, CoverCache};
use error::CoverArtError;
use providers::{create_shared_client, CacheKeyScope, CoverArtProvider, CoverResult};
use sources::{search_local_cover_art, ArtSource};

const CACHE_VALIDATION_INTERVAL: Duration = Duration::from_secs(60 * 60); // 1 hour
/// How long a cover that could not be resolved is left alone, so repeated
/// presence updates do not retry a failing host on every push.
const FAILURE_BACKOFF: Duration = Duration::from_secs(10 * 60);
/// Uploads and URL checks run while the upload slot is held, so neither may
/// hang: the provider clients have no timeout of their own.
const URL_CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const UPLOAD_TIMEOUT: Duration = Duration::from_secs(120);
const FRESH_UPLOAD_RECHECK: Duration = Duration::from_secs(2);
/// Slowest upload rate still allowed to finish: large raw uploads
/// (`normalize_uploads = false`) get longer than `UPLOAD_TIMEOUT`.
const MIN_UPLOAD_BYTES_PER_SEC: u64 = 16 * 1024;
/// How long a cached URL whose host could not be reached is served before it
/// is checked again.
const UNKNOWN_RECHECK: Duration = Duration::from_secs(60 * 60);

#[derive(Debug, Clone, Copy)]
struct Timeouts {
    url_check: Duration,
    upload: Duration,
    /// Wait before probing a fresh upload again when it looked gone: a CDN
    /// can briefly answer 404 for a file it has just accepted.
    fresh_upload_recheck: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            url_check: URL_CHECK_TIMEOUT,
            upload: UPLOAD_TIMEOUT,
            fresh_upload_recheck: FRESH_UPLOAD_RECHECK,
        }
    }
}

/// What a URL check may conclude from the served size.
#[derive(Debug, Clone, Copy)]
enum Expect {
    /// Direct URLs and MusicBrainz results: some servers wrongly send
    /// `Content-Length: 0` on HEAD, so the size says nothing.
    Any,
    /// An uploaded cover: an empty file is a broken upload (Catbox). `len` is
    /// the size recorded right after the upload, for diagnostics.
    Upload { len: Option<u64> },
}

/// Check a URL right after its upload. A CDN can briefly answer 404 for a file
/// it has just accepted, so a "gone" result is probed once more after a wait.
async fn check_fresh_url(url: &str, expect: Expect, timeouts: Timeouts) -> (UrlCheck, Option<u64>) {
    let first = CoverManager::check_cover_url(url, expect, timeouts.url_check).await;
    if first.0 != UrlCheck::Gone {
        return first;
    }
    debug!("Fresh cover art at {} looks gone; probing again", url);
    tokio::time::sleep(timeouts.fresh_upload_recheck).await;
    CoverManager::check_cover_url(url, expect, timeouts.url_check).await
}

/// Time allowed for one upload of `size` bytes.
fn upload_deadline(base: Duration, size: Option<u64>) -> Duration {
    let needed = Duration::from_secs(size.unwrap_or(0) / MIN_UPLOAD_BYTES_PER_SEC);
    base.max(needed)
}

/// One upload queue for the whole process, so a config reload (which builds a
/// new `CoverManager`) cannot run a second upload of the same cover.
fn shared_upload_slots() -> Arc<Semaphore> {
    static UPLOAD_SLOTS: OnceLock<Arc<Semaphore>> = OnceLock::new();
    Arc::clone(UPLOAD_SLOTS.get_or_init(|| Arc::new(Semaphore::new(1))))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DirectUrlPolicy {
    allow_direct: bool,
    reason: &'static str,
}

enum CacheLookup {
    Hit(String),
    Miss,
}

#[cfg(test)]
impl CacheLookup {
    fn is_hit(&self) -> bool {
        matches!(self, Self::Hit(_))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UrlCheck {
    Alive,
    /// 404 or 410, an empty file, or a file whose size changed.
    Gone,
    /// Network error, rate limit, server error or access denied. Says
    /// nothing about the file, so a cached URL is kept and checked later.
    Unknown,
}

impl DirectUrlPolicy {
    const fn allow() -> Self {
        Self {
            allow_direct: true,
            reason: "public_url",
        }
    }

    const fn deny(reason: &'static str) -> Self {
        Self {
            allow_direct: false,
            reason,
        }
    }
}

pub struct CoverManager {
    providers: Vec<Arc<dyn CoverArtProvider>>,
    cache: Arc<CoverCache>,
    config: Arc<config::ConfigManager>,
    artwork_slots: Arc<Semaphore>,
    upload_slots: Arc<Semaphore>,
    failures: Mutex<HashMap<String, Instant>>,
    failure_backoff: Duration,
    timeouts: Timeouts,
}

impl CoverManager {
    pub fn new(config: &Arc<config::ConfigManager>) -> Result<Self, CoverArtError> {
        info!("Initializing cover art manager");
        let cover_config = config.cover_config();
        let cache = CoverCache::new(cover_config.cache)?;
        cache.clean()?;
        let cache = Arc::new(cache);
        let artwork_slots = Arc::new(Semaphore::new(1));
        let mut providers: Vec<Arc<dyn CoverArtProvider>> = Vec::new();

        for provider_name in &cover_config.provider.provider {
            match provider_name.as_str() {
                "musicbrainz" => {
                    debug!("Adding MusicBrainz provider");
                    providers.push(Arc::new(
                        providers::musicbrainz::MusicbrainzProvider::with_config(
                            cover_config.provider.musicbrainz.clone(),
                        ),
                    ));
                }
                "imgbb" => {
                    if cover_config.provider.imgbb.api_key.is_some() {
                        debug!("Adding ImgBB provider");
                        providers.push(Arc::new(providers::imgbb::ImgbbProvider::with_config(
                            cover_config.provider.imgbb.clone(),
                        )));
                    } else {
                        warn!("Skipping ImgBB provider - no API key configured");
                    }
                }
                "catbox" => {
                    debug!("Adding Catbox provider");
                    providers.push(Arc::new(providers::catbox::CatboxProvider::with_config(
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
            artwork_slots,
            upload_slots: shared_upload_slots(),
            failures: Mutex::default(),
            failure_backoff: FAILURE_BACKOFF,
            timeouts: Timeouts::default(),
        })
    }

    fn is_local_or_private_url(url_str: &str) -> bool {
        if let Ok(parsed) = Url::parse(url_str) {
            if let Some(host) = parsed.host() {
                match host {
                    Host::Domain(d) => {
                        let dl = d.to_ascii_lowercase();
                        if dl == "localhost"
                            || dl.ends_with(".localhost")
                            || dl.ends_with(".local")
                            || dl.ends_with(".localdomain")
                            || dl.ends_with(".home.arpa")
                            || dl.ends_with(".lan")
                        {
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

    fn auth_query_reason(parsed: &Url) -> Option<&'static str> {
        let mut has_subsonic_user = false;
        let mut has_subsonic_password = false;
        let mut has_subsonic_token = false;
        let mut has_subsonic_salt = false;

        for (key, _) in parsed.query_pairs() {
            let key = key.to_ascii_lowercase();
            match key.as_str() {
                "u" => has_subsonic_user = true,
                "p" => has_subsonic_password = true,
                "t" => has_subsonic_token = true,
                "s" => has_subsonic_salt = true,
                "apikey"
                | "api_key"
                | "token"
                | "auth"
                | "authorization"
                | "x-emby-token"
                | "x-emby-authorization" => {
                    return Some("auth_query_param");
                }
                _ => {}
            }
        }

        if has_subsonic_user && has_subsonic_password {
            return Some("subsonic_password_query");
        }

        if has_subsonic_token && has_subsonic_salt {
            return Some("subsonic_token_query");
        }

        None
    }

    /// Public wrapper: checks if a URL can be used directly as cover art
    /// in Discord's Rich Presence without needing upload to ImgBB/Catbox.
    /// Bridge-provided URLs (YouTube CDN, SoundCloud CDN) typically pass.
    pub fn is_direct_url_allowed(url_str: &str) -> bool {
        Self::direct_url_policy(url_str).allow_direct
    }

    fn direct_url_policy(url_str: &str) -> DirectUrlPolicy {
        let parsed = match Url::parse(url_str) {
            Ok(url) => url,
            Err(_) => return DirectUrlPolicy::deny("invalid_url"),
        };

        let scheme = parsed.scheme();
        if scheme != "http" && scheme != "https" {
            return DirectUrlPolicy::deny("unsupported_scheme");
        }

        if Self::is_local_or_private_url(url_str) {
            return DirectUrlPolicy::deny("local_or_private_host");
        }

        let path = parsed.path().to_ascii_lowercase();
        if path.contains("/rest/getcoverart") {
            return DirectUrlPolicy::deny("subsonic_cover_endpoint");
        }

        if path.contains("/items/") && path.contains("/images") {
            return DirectUrlPolicy::deny("jellyfin_image_endpoint");
        }

        if let Some(reason) = Self::auth_query_reason(&parsed) {
            return DirectUrlPolicy::deny(reason);
        }

        DirectUrlPolicy::allow()
    }

    /// Synchronous, in-process cache-only lookup. No HTTP, no validation.
    /// Used by the presence fast path so Discord receives a track update
    /// without waiting for slow providers when a usable cached URL exists.
    /// Returns `None` on miss, deserialize error, or stale entry - the
    /// async `get_cover_art` path will revalidate / re-fetch as needed.
    /// Pass `read_cache: false` to skip lookup.
    pub fn try_cached_cover_art(
        &self,
        metadata_source: &MetadataSource,
        source: Option<&ArtSource>,
        read_cache: bool,
    ) -> Option<String> {
        if !read_cache {
            trace!("Skipping cached cover art lookup (read_cache=false)");
            return None;
        }

        for cache_key in self.cache_keys(source, metadata_source) {
            let Some(entry) = self.cache.get_by_key(&cache_key).ok().flatten() else {
                continue;
            };
            if entry.is_stale(SystemTime::now()) {
                continue;
            }

            if entry.url.len() > 512
                || !(entry.url.starts_with("https://") || entry.url.starts_with("http://"))
            {
                warn!(
                    "Discarding cached cover art entry with malformed URL (provider: {}, len: {})",
                    entry.provider,
                    entry.url.len()
                );
                continue;
            }

            if entry.provider.eq_ignore_ascii_case("direct")
                && !Self::direct_url_policy(&entry.url).allow_direct
            {
                continue;
            }

            if Self::is_legacy_catbox_image_url(&entry) {
                continue;
            }

            return Some(entry.url);
        }

        None
    }

    pub async fn get_cover_art(
        &self,
        source: Option<ArtSource>,
        metadata_source: &Arc<MetadataSource>,
        read_cache: bool,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, CoverArtError> {
        if cancel.is_cancelled() {
            debug!("Cover art fetch cancelled before start");
            return Ok(None);
        }

        // 1. Check Cache
        let cache_keys = self.cache_keys(source.as_ref(), metadata_source);
        if read_cache {
            for cache_key in &cache_keys {
                if let CacheLookup::Hit(url) = self.lookup_cached_cover(cache_key).await? {
                    return Ok(Some(url));
                }
            }
        } else {
            trace!("Skipping cover cache lookup (read_cache=false)");
        }
        trace!("No valid cache entry found.");

        let cover_key = cache_keys.first().cloned().unwrap_or_default();
        if read_cache && self.failed_recently(&cover_key) {
            debug!("Cover art failed recently; not retrying yet");
            return Ok(None);
        }

        let found = self
            .fetch_cover_art(source, metadata_source, read_cache, cancel)
            .await;
        match &found {
            Ok(Some(_)) => self.forget_failure(&cover_key),
            Ok(None) if !cancel.is_cancelled() => self.remember_failure(cover_key),
            _ => {}
        }
        found
    }

    fn failed_recently(&self, cover_key: &str) -> bool {
        let failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        failures
            .get(cover_key)
            .is_some_and(|failed_at| failed_at.elapsed() < self.failure_backoff)
    }

    fn remember_failure(&self, cover_key: String) {
        let mut failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        failures.retain(|_, failed_at| failed_at.elapsed() < self.failure_backoff);
        failures.insert(cover_key, Instant::now());
    }

    fn forget_failure(&self, cover_key: &str) {
        let mut failures = self.failures.lock().unwrap_or_else(|e| e.into_inner());
        failures.remove(cover_key);
    }

    async fn fetch_cover_art(
        &self,
        mut source: Option<ArtSource>,
        metadata_source: &Arc<MetadataSource>,
        read_cache: bool,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, CoverArtError> {
        let legacy_cache_key = metadata_source.cache_key();

        if source.is_none() {
            debug!(
                "No art source found in metadata; trying local search and metadata-only providers"
            );
        }

        // Prepare a potentially transformed source for providers
        let mut source_for_providers = None;

        // 2. If we have a direct URL, decide whether to use it or transform
        if let Some(ArtSource::Url(ref url)) = source.as_ref() {
            let policy = Self::direct_url_policy(url);

            if !policy.allow_direct {
                info!(
                    "Direct cover art URL is not eligible for Discord (reason: {}); attempting provider upload: {}",
                    policy.reason, url
                );
                // Try to fetch bytes so providers can upload
                if cancel.is_cancelled() {
                    debug!("Cover art fetch cancelled before URL download");
                    return Ok(None);
                }
                let client = create_shared_client();
                match client.get(url).send().await {
                    Ok(resp) if resp.status().is_success() => match resp.bytes().await {
                        Ok(bytes) => {
                            debug!(
                                "Fetched image bytes from source URL ({} bytes)",
                                bytes.len()
                            );
                            source_for_providers = Some(ArtSource::Bytes(bytes.to_vec()));
                        }
                        Err(e) => {
                            warn!("Failed to read bytes from source URL response: {}", e);
                        }
                    },
                    Ok(resp) => {
                        warn!(
                            "Source URL fetch returned non-success status: {}",
                            resp.status()
                        );
                    }
                    Err(e) => {
                        warn!("Failed to fetch source URL: {}", e);
                    }
                }
            } else if self.validate_cover_url(url).await {
                debug!("Using direct URL from source: {}", url);
                let cache_key = source
                    .as_ref()
                    .and_then(Self::source_cache_key)
                    .unwrap_or_else(|| legacy_cache_key.to_string());
                Self::cache_store_entry(
                    self.cache.clone(),
                    &cache_key,
                    "direct",
                    url,
                    None,
                    None,
                    true,
                )
                .await?;
                return Ok(Some(url.clone()));
            } else {
                warn!(
                    "Direct cover art URL {} failed validation; trying configured providers",
                    url
                );
            }
        }
        if source_for_providers.is_none() {
            source_for_providers = source.take();
        }

        // 3. Prefer a shared local cover file before opening embedded artwork.
        let mut local_cover_key = None;
        if let Some(path) = metadata_source.local_file_path() {
            if let Some(parent) = path.parent() {
                debug!("Attempting to find local cover art in: {:?}", parent);
                let cover_config = self.config.cover_config();
                let file_names = cover_config.file_names.clone();
                let search_root = parent.to_path_buf();
                let search_depth = cover_config.local_search_depth;
                let local_art_result = spawn_blocking(move || {
                    search_local_cover_art(&search_root, &file_names, search_depth)
                })
                .await
                .map_err(|e| {
                    CoverArtError::other(format!("Local cover art search failed: {}", e))
                })?;
                let local_art = local_art_result?;

                if let Some(art_source) = local_art {
                    local_cover_key = Self::source_cache_key(&art_source);
                    if let Some(url) = self
                        .try_providers(Some(&art_source), metadata_source, read_cache, cancel)
                        .await?
                    {
                        return Ok(Some(url));
                    }
                }
            }
        }

        // 4. Try an explicit MPRIS source, unless it is the local cover file
        // that just failed (players often report that file as their art).
        let same_as_local_cover = local_cover_key.is_some()
            && source_for_providers
                .as_ref()
                .and_then(Self::source_cache_key)
                == local_cover_key;
        if same_as_local_cover {
            debug!("MPRIS artwork is the local cover file that already failed; skipping");
        } else if source_for_providers.is_some() {
            if let Some(url) = self
                .try_providers(
                    source_for_providers.as_ref(),
                    metadata_source,
                    read_cache,
                    cancel,
                )
                .await?
            {
                return Ok(Some(url));
            }
        }

        // 5. Embedded artwork is the expensive fallback. It is parsed only after
        // direct, cached, and shared local sources have failed.
        if let Some(embedded_art) = self.load_embedded_art(metadata_source, cancel).await? {
            return self
                .try_providers(Some(&embedded_art), metadata_source, read_cache, cancel)
                .await;
        }

        // 6. Providers such as MusicBrainz can still resolve artwork from tags.
        if source_for_providers.is_none() {
            return self
                .try_providers(None, metadata_source, read_cache, cancel)
                .await;
        }

        Ok(None)
    }

    async fn load_embedded_art(
        &self,
        metadata_source: &MetadataSource,
        cancel: &CancellationToken,
    ) -> Result<Option<ArtSource>, CoverArtError> {
        if let Some(source) = metadata_source.embedded_art_from_loaded_tag() {
            return Ok(Some(source));
        }
        let Some(path) = metadata_source.local_file_path() else {
            return Ok(None);
        };
        if cancel.is_cancelled() {
            return Ok(None);
        }

        let artwork_slot = tokio::select! {
            permit = self.artwork_slots.clone().acquire_owned() => permit.map_err(|_| {
                CoverArtError::other("Artwork processing queue closed")
            })?,
            _ = cancel.cancelled() => return Ok(None),
        };
        let display_path = path.clone();
        let result = spawn_blocking(move || {
            let _artwork_slot = artwork_slot;
            MetadataSource::embedded_art_from_path(&path)
        })
        .await
        .map_err(|e| CoverArtError::other(format!("Embedded art task failed: {e}")))?;
        match result {
            Ok(source) => Ok(source),
            Err(err) => {
                warn!(
                    "Failed to read embedded cover art from {:?}: {}",
                    display_path, err
                );
                Ok(None)
            }
        }
    }

    async fn try_providers(
        &self,
        source: Option<&ArtSource>,
        metadata_source: &Arc<MetadataSource>,
        read_cache: bool,
        cancel: &CancellationToken,
    ) -> Result<Option<String>, CoverArtError> {
        let dummy = ArtSource::Url(String::new());
        let source_cache_key = source.and_then(Self::source_cache_key);
        let normalize_uploads = self.config.cover_config().normalize_uploads;
        let mut normalized_source: Option<Option<ArtSource>> = None;

        for provider in &self.providers {
            let supported = match source {
                Some(s) => provider.supports_source_type(s),
                None => provider.supports_metadata_only(),
            };
            if !supported {
                trace!(
                    "Provider {} does not support this source type",
                    provider.name()
                );
                continue;
            }

            let cache_key = Self::provider_cache_key(
                provider.as_ref(),
                source_cache_key.as_deref(),
                metadata_source,
                normalize_uploads,
            );
            if read_cache {
                if let CacheLookup::Hit(url) = self.lookup_cached_cover(&cache_key).await? {
                    return Ok(Some(url));
                }
            }

            let process_source = match provider.cache_key_scope() {
                CacheKeyScope::Source => {
                    if normalized_source.is_none() {
                        normalized_source = Some(match source {
                            Some(source) => {
                                match self.normalize_upload_source(source, cancel).await {
                                    Ok(source) => source,
                                    Err(err) => {
                                        warn!("Skipping invalid upload artwork: {err}");
                                        None
                                    }
                                }
                            }
                            None => None,
                        });
                    }
                    normalized_source.as_ref().and_then(Option::as_ref)
                }
                CacheKeyScope::Metadata => source.or(Some(&dummy)),
            };
            let Some(process_source) = process_source else {
                trace!(
                    "Provider {} has no valid artwork source to upload",
                    provider.name()
                );
                continue;
            };

            debug!("Attempting cover art retrieval with {}", provider.name());
            let retrieval = Self::retrieve_and_cache(
                Arc::clone(provider),
                Arc::clone(&self.cache),
                process_source.clone(),
                Arc::clone(metadata_source),
                cache_key.clone(),
                cancel.clone(),
                self.timeouts,
            );
            let retrieved = if provider.cache_key_scope() == CacheKeyScope::Source {
                let upload_slot = tokio::select! {
                    permit = self.upload_slots.clone().acquire_owned() => permit.map_err(|_| {
                        CoverArtError::other("Cover upload queue closed")
                    })?,
                    _ = cancel.cancelled() => return Ok(None),
                };
                // Another lookup may have uploaded this cover while we waited.
                if read_cache {
                    if let CacheLookup::Hit(url) = self.lookup_cached_cover(&cache_key).await? {
                        return Ok(Some(url));
                    }
                }
                // The host keeps an upload even if this lookup is cancelled or
                // dropped, so its URL is cached by a detached task. The slot is
                // held until then so a queued lookup finds the URL.
                tokio::spawn(async move {
                    let _upload_slot = upload_slot;
                    retrieval.await
                })
                .await
                .map_err(|e| CoverArtError::other(format!("Cover upload task failed: {e}")))?
            } else {
                retrieval.await
            };
            if let Some(url) = retrieved? {
                return Ok(Some(url));
            }
        }

        debug!("No cover art found from any source");
        Ok(None)
    }

    async fn retrieve_and_cache(
        provider: Arc<dyn CoverArtProvider>,
        cache: Arc<CoverCache>,
        source: ArtSource,
        metadata_source: Arc<MetadataSource>,
        cache_key: String,
        cancel: CancellationToken,
        timeouts: Timeouts,
    ) -> Result<Option<String>, CoverArtError> {
        let size = match &source {
            ArtSource::Bytes(bytes) => Some(bytes.len() as u64),
            ArtSource::Base64(data) => Some(data.len() as u64 / 4 * 3),
            ArtSource::File(path) => tokio::fs::metadata(path).await.ok().map(|meta| meta.len()),
            ArtSource::Url(_) => None,
        };
        let deadline = upload_deadline(timeouts.upload, size);
        let processed = tokio::time::timeout(
            deadline,
            provider.process_borrowed(&source, &metadata_source, &cancel),
        )
        .await;
        let result = match processed {
            Err(_) => {
                warn!(
                    "Provider {} did not answer within {:?}; giving up",
                    provider.name(),
                    deadline
                );
                return Ok(None);
            }
            Ok(Ok(Some(result))) => result,
            Ok(Ok(None)) => {
                debug!("Provider {} found no cover art", provider.name());
                return Ok(None);
            }
            Ok(Err(e)) => {
                warn!("Provider {} failed: {}", provider.name(), e);
                return Ok(None);
            }
        };
        let CoverResult {
            url,
            provider: provider_name,
            expiration,
        } = result;
        let upload = provider.cache_key_scope() == CacheKeyScope::Source;
        let expect = if upload {
            Expect::Upload { len: None }
        } else {
            Expect::Any
        };
        // Record the size the host serves now, for later diagnostics. Providers
        // may alter the bytes they send, so the source size is not it.
        let (check, served_len) = check_fresh_url(&url, expect, timeouts).await;
        let served_len = served_len.filter(|_| upload);
        let verified = match check {
            UrlCheck::Alive => true,
            UrlCheck::Unknown => {
                warn!(
                    "Could not verify cover art from {} yet; using it and checking it on the next lookup",
                    provider_name
                );
                false
            }
            UrlCheck::Gone => {
                warn!(
                    "Provider {} returned unusable cover art, skipping",
                    provider_name
                );
                return Ok(None);
            }
        };
        info!("Successfully retrieved cover art from {}", provider_name);
        // The host has the file either way; losing its URL to a cache write
        // error would only make the next presence update upload it again.
        if let Err(e) = Self::cache_store_entry(
            cache,
            &cache_key,
            &provider_name,
            &url,
            expiration,
            served_len,
            verified,
        )
        .await
        {
            warn!(
                "Failed to cache cover art URL from {}: {}",
                provider_name, e
            );
        }
        Ok(Some(url))
    }

    async fn normalize_upload_source(
        &self,
        source: &ArtSource,
        cancel: &CancellationToken,
    ) -> Result<Option<ArtSource>, CoverArtError> {
        if !self.config.cover_config().normalize_uploads {
            debug!("Upload normalization disabled; uploading artwork unchanged");
            return Ok(Some(source.clone()));
        }

        let Some(bytes) = source.materialize_bytes().await? else {
            return Ok(None);
        };
        if cancel.is_cancelled() {
            return Ok(None);
        }

        let artwork_slot = tokio::select! {
            permit = self.artwork_slots.clone().acquire_owned() => permit.map_err(|_| {
                CoverArtError::other("Artwork processing queue closed")
            })?,
            _ = cancel.cancelled() => return Ok(None),
        };
        let original_len = bytes.len();
        let normalized = spawn_blocking(move || {
            let _artwork_slot = artwork_slot;
            image::normalize_upload_bytes(&bytes)
        })
        .await
        .map_err(|e| CoverArtError::other(format!("Artwork normalization task failed: {e}")))??;
        debug!(
            "Normalized upload artwork from {} to {} bytes",
            original_len,
            normalized.len()
        );

        Ok(Some(ArtSource::Bytes(normalized)))
    }

    fn source_cache_key(source: &ArtSource) -> Option<String> {
        match source.cache_key() {
            Ok(key) => Some(key),
            Err(err) => {
                warn!("Failed to generate content cache key: {}", err);
                None
            }
        }
    }

    fn provider_cache_key(
        provider: &dyn CoverArtProvider,
        source_cache_key: Option<&str>,
        metadata_source: &MetadataSource,
        normalize_uploads: bool,
    ) -> String {
        match provider.cache_key_scope() {
            CacheKeyScope::Source => {
                let key = source_cache_key
                    .map(str::to_string)
                    .unwrap_or_else(|| metadata_source.cache_key().to_string());
                // Uploaded bytes differ between normalized and raw mode, so the
                // hosted URL does too. Keep the two in separate cache entries.
                if normalize_uploads {
                    key
                } else {
                    format!("{key}-raw")
                }
            }
            CacheKeyScope::Metadata => metadata_source.cover_cache_key().to_string(),
        }
    }

    fn cache_keys(
        &self,
        source: Option<&ArtSource>,
        metadata_source: &MetadataSource,
    ) -> Vec<String> {
        let source_cache_key = source.and_then(Self::source_cache_key);
        let normalize_uploads = self.config.cover_config().normalize_uploads;
        let mut keys = Vec::new();

        if matches!(source, Some(ArtSource::Url(url)) if Self::direct_url_policy(url).allow_direct)
        {
            if let Some(key) = source_cache_key.clone() {
                keys.push(key);
            }
        }

        for provider in &self.providers {
            let supported = match source {
                Some(source) => provider.supports_source_type(source),
                None => provider.supports_metadata_only(),
            };
            if !supported {
                continue;
            }

            let key = Self::provider_cache_key(
                provider.as_ref(),
                source_cache_key.as_deref(),
                metadata_source,
                normalize_uploads,
            );
            if !keys.contains(&key) {
                keys.push(key);
            }
        }

        let legacy_key = metadata_source.cache_key().to_string();
        if !keys.contains(&legacy_key) {
            keys.push(legacy_key);
        }
        keys
    }

    async fn lookup_cached_cover(&self, cache_key: &str) -> Result<CacheLookup, CoverArtError> {
        let Some(mut entry) = self.cache_get_entry(cache_key).await? else {
            return Ok(CacheLookup::Miss);
        };
        let url = entry.url.clone();
        let mut drop_reason = None;

        if url.len() > 512 || !(url.starts_with("https://") || url.starts_with("http://")) {
            drop_reason = Some("malformed_url");
        } else if Self::is_legacy_catbox_image_url(&entry) {
            drop_reason = Some("legacy_catbox_img_url");
        } else if entry.provider.eq_ignore_ascii_case("direct") {
            let policy = Self::direct_url_policy(&url);
            if !policy.allow_direct {
                drop_reason = Some(policy.reason);
            }
        }

        let needs_validation = entry.is_stale(SystemTime::now())
            || entry
                .last_validated
                .elapsed()
                .map(|elapsed| elapsed >= CACHE_VALIDATION_INTERVAL)
                .unwrap_or(true);

        if drop_reason.is_none() {
            let expect = if entry.is_upload() {
                Expect::Upload {
                    len: entry.content_len,
                }
            } else {
                Expect::Any
            };
            let (check, served_len) = if needs_validation {
                Self::check_cover_url(&url, expect, self.timeouts.url_check).await
            } else {
                (UrlCheck::Alive, None)
            };
            if needs_validation && check != UrlCheck::Gone {
                if check == UrlCheck::Alive {
                    self.cache.renew(&mut entry);
                    if entry.is_upload() {
                        entry.content_len = entry.content_len.or(served_len);
                    }
                } else {
                    // Serve it from the fast path for a while instead of
                    // checking (and waiting) on every play.
                    self.cache.postpone(&mut entry, UNKNOWN_RECHECK);
                }
                self.cache_update_entry(cache_key, &entry).await?;
            }
            if check != UrlCheck::Gone {
                debug!(
                    "Serving cached cover art (provider: {}, recheck: {:?})",
                    entry.provider,
                    needs_validation.then_some(check)
                );
                return Ok(CacheLookup::Hit(url));
            }
        }

        let reason = drop_reason.unwrap_or("url_gone");
        warn!(
            "Cached cover art URL {} is no longer eligible (provider: {}, reason: {}); removing entry",
            url, entry.provider, reason
        );
        self.cache_remove_entry(cache_key, &url).await?;
        Ok(CacheLookup::Miss)
    }

    fn is_legacy_catbox_image_url(entry: &CacheEntry) -> bool {
        matches!(entry.provider.as_str(), "catbox" | "litterbox")
            && Url::parse(&entry.url)
                .is_ok_and(|url| url.path().to_ascii_lowercase().ends_with(".img"))
    }

    async fn validate_cover_url(&self, url: &str) -> bool {
        Self::check_cover_url(url, Expect::Any, self.timeouts.url_check)
            .await
            .0
            == UrlCheck::Alive
    }

    /// Check that `url` still serves the cover. Returns the served size when
    /// the response states it.
    async fn check_cover_url(
        url: &str,
        expect: Expect,
        timeout: Duration,
    ) -> (UrlCheck, Option<u64>) {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            trace!("Skipping validation for non-HTTP cover art URL: {}", url);
            return (UrlCheck::Alive, None);
        }

        let client = create_shared_client();

        // HEAD first: a GET for a Catbox file that does not exist never answers.
        match client.head(url).timeout(timeout).send().await {
            Ok(resp) if resp.status().is_success() => {
                trace!("HEAD validation succeeded for cover art URL: {}", url);
                // Catbox answers every HEAD with `Content-Length: 0`, even for
                // files that exist, so an upload's empty HEAD is confirmed with
                // a one-byte GET.
                if matches!(expect, Expect::Upload { .. })
                    && !resp.headers().contains_key(header::CONTENT_ENCODING)
                    && Self::content_length_header(&resp) == Some(0)
                {
                    return Self::check_with_ranged_get(&client, url, expect, timeout).await;
                }
                return Self::judge_served_size(url, &resp, expect);
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

        match client.get(url).timeout(timeout).send().await {
            Ok(resp) if resp.status().is_success() => {
                trace!("GET validation succeeded for cover art URL: {}", url);
                Self::judge_served_size(url, &resp, expect)
            }
            Ok(resp) => Self::judge_failed_status(url, resp.status()),
            Err(e) => Self::unverifiable(url, e),
        }
    }

    async fn check_with_ranged_get(
        client: &reqwest::Client,
        url: &str,
        expect: Expect,
        timeout: Duration,
    ) -> (UrlCheck, Option<u64>) {
        let response = client
            .get(url)
            .header(header::RANGE, "bytes=0-0")
            .timeout(timeout)
            .send()
            .await;
        match response {
            Ok(resp) if resp.status() == StatusCode::PARTIAL_CONTENT => {
                match Self::content_range_size(&resp) {
                    Some(size) => Self::judge_len(url, Some(size), expect),
                    None => Self::judge_first_chunk(url, resp, expect).await,
                }
            }
            // Only `Content-Range: bytes */0` says the file is empty.
            Ok(resp) if resp.status() == StatusCode::RANGE_NOT_SATISFIABLE => {
                if Self::content_range_size(&resp) == Some(0) {
                    Self::judge_len(url, Some(0), expect)
                } else {
                    Self::judge_failed_status(url, resp.status())
                }
            }
            // The server ignored the range: use the stated length, or look at
            // the first chunk of the body without waiting for the rest.
            Ok(resp) if resp.status().is_success() => {
                if resp.headers().contains_key(header::CONTENT_ENCODING)
                    || Self::content_length_header(&resp).is_some()
                {
                    Self::judge_served_size(url, &resp, expect)
                } else {
                    Self::judge_first_chunk(url, resp, expect).await
                }
            }
            Ok(resp) => Self::judge_failed_status(url, resp.status()),
            Err(e) => Self::unverifiable(url, e),
        }
    }

    /// The size after the slash in `Content-Range: bytes 0-0/<size>` or
    /// `bytes */<size>`; `None` when it is missing or `*`.
    fn content_range_size(resp: &reqwest::Response) -> Option<u64> {
        resp.headers()
            .get(header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.rsplit('/').next())
            .and_then(|size| size.parse::<u64>().ok())
    }

    /// Read at most one chunk: enough to tell an empty body from a file.
    async fn judge_first_chunk(
        url: &str,
        mut resp: reqwest::Response,
        expect: Expect,
    ) -> (UrlCheck, Option<u64>) {
        match resp.chunk().await {
            Ok(Some(_)) => (UrlCheck::Alive, None),
            Ok(None) => Self::judge_len(url, Some(0), expect),
            Err(e) => Self::unverifiable(url, e),
        }
    }

    fn judge_failed_status(url: &str, status: StatusCode) -> (UrlCheck, Option<u64>) {
        if matches!(status, StatusCode::NOT_FOUND | StatusCode::GONE) {
            warn!("Cover art URL is gone (status {}): {}", status, url);
            return (UrlCheck::Gone, None);
        }
        warn!(
            "Could not verify cover art URL (status {}): {}",
            status, url
        );
        (UrlCheck::Unknown, None)
    }

    fn unverifiable(url: &str, error: reqwest::Error) -> (UrlCheck, Option<u64>) {
        warn!(
            "Could not verify cover art URL {}: {}",
            url,
            error.without_url()
        );
        (UrlCheck::Unknown, None)
    }

    /// `Response::content_length` is the body size hint, which is 0 for HEAD.
    fn content_length_header(resp: &reqwest::Response) -> Option<u64> {
        resp.headers()
            .get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
    }

    fn judge_served_size(
        url: &str,
        resp: &reqwest::Response,
        expect: Expect,
    ) -> (UrlCheck, Option<u64>) {
        // A compressed length is not the file's size.
        if resp.headers().contains_key(header::CONTENT_ENCODING) {
            return (UrlCheck::Alive, None);
        }
        Self::judge_len(url, Self::content_length_header(resp), expect)
    }

    fn judge_len(url: &str, served_len: Option<u64>, expect: Expect) -> (UrlCheck, Option<u64>) {
        let Expect::Upload { len: recorded_len } = expect else {
            return (UrlCheck::Alive, served_len);
        };
        match (served_len, recorded_len) {
            (Some(0), _) => {
                warn!("Cover art URL serves an empty file: {}", url);
                (UrlCheck::Gone, served_len)
            }
            // Logged only: dropping the entry would upload the cover again
            // whenever a host re-optimises its files.
            (Some(served), Some(recorded)) if served != recorded => {
                warn!(
                    "Cover art URL serves {} bytes, {} when uploaded: {}",
                    served, recorded, url
                );
                (UrlCheck::Alive, served_len)
            }
            _ => (UrlCheck::Alive, served_len),
        }
    }

    async fn run_blocking<F, T>(context: &'static str, f: F) -> Result<T, CoverArtError>
    where
        F: FnOnce() -> Result<T, CoverArtError> + Send + 'static,
        T: Send + 'static,
    {
        spawn_blocking(f)
            .await
            .map_err(|e| CoverArtError::other(format!("Cache {context} task failed: {e}")))?
    }

    async fn cache_get_entry(&self, key: &str) -> Result<Option<CacheEntry>, CoverArtError> {
        let cache = self.cache.clone();
        let key = key.to_string();
        Self::run_blocking("lookup", move || cache.get_by_key(&key)).await
    }

    /// Writes `entry` back unless another lookup replaced its URL meanwhile.
    async fn cache_update_entry(&self, key: &str, entry: &CacheEntry) -> Result<(), CoverArtError> {
        let cache = self.cache.clone();
        let key = key.to_string();
        let entry = entry.clone();
        Self::run_blocking("update", move || cache.update_entry_if_url(&key, &entry)).await
    }

    async fn cache_store_entry(
        cache: Arc<CoverCache>,
        key: &str,
        provider: &str,
        url: &str,
        expiration: Option<Duration>,
        content_len: Option<u64>,
        verified: bool,
    ) -> Result<(), CoverArtError> {
        let (key, provider, url) = (key.to_string(), provider.to_string(), url.to_string());
        Self::run_blocking("store", move || {
            cache.store_with_key(&key, &provider, &url, expiration, content_len, verified)
        })
        .await
    }

    /// Removes the entry unless another lookup replaced its URL meanwhile.
    async fn cache_remove_entry(&self, key: &str, url: &str) -> Result<(), CoverArtError> {
        let cache = self.cache.clone();
        let (key, url) = (key.to_string(), url.to_string());
        Self::run_blocking("remove", move || cache.remove_by_key_if_url(&key, &url)).await
    }
}

pub async fn clean_cache() -> Result<(), CoverArtError> {
    info!("Starting periodic cache cleanup");
    let cache = CoverCache::new(config::get_config().cover_config().cache)?;
    let cleaned_result = spawn_blocking(move || cache.clean())
        .await
        .map_err(|e| CoverArtError::other(format!("Cache cleanup task failed: {}", e)))?;
    let cleaned = cleaned_result?;
    if cleaned > 0 {
        info!("Cleaned {} expired cache entries", cleaned);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{CacheEntry, CoverManager};
    use std::time::SystemTime;

    #[test]
    fn raw_and_normalized_uploads_use_distinct_cache_keys() {
        use super::providers::catbox::CatboxProvider;
        use crate::config::schema::CatboxConfig;
        use crate::metadata::MetadataSource;

        let provider = CatboxProvider::with_config(CatboxConfig::default());
        let metadata = MetadataSource::new(None, None);

        let normalized =
            CoverManager::provider_cache_key(&provider, Some("abc123"), &metadata, true);
        let raw = CoverManager::provider_cache_key(&provider, Some("abc123"), &metadata, false);

        assert_eq!(normalized, "abc123");
        assert_eq!(raw, "abc123-raw");
        assert_ne!(normalized, raw);
    }

    #[test]
    fn large_uploads_get_more_time() {
        use super::{upload_deadline, UPLOAD_TIMEOUT};
        use std::time::Duration;
        assert_eq!(
            upload_deadline(UPLOAD_TIMEOUT, Some(65 * 1024)),
            UPLOAD_TIMEOUT
        );
        assert_eq!(upload_deadline(UPLOAD_TIMEOUT, None), UPLOAD_TIMEOUT);
        assert_eq!(
            upload_deadline(UPLOAD_TIMEOUT, Some(8 * 1024 * 1024)),
            Duration::from_secs(512)
        );
    }

    #[test]
    fn config_reloads_share_one_upload_queue() {
        assert!(std::sync::Arc::ptr_eq(
            &super::shared_upload_slots(),
            &super::shared_upload_slots()
        ));
    }

    #[test]
    fn rejects_only_legacy_catbox_image_urls() {
        let mut entry = CacheEntry {
            url: "https://files.catbox.moe/cover.img".to_string(),
            provider: "catbox".to_string(),
            expires_at: SystemTime::now(),
            last_validated: SystemTime::now(),
            data_file: Some("cover.bin".to_string()),
            hosted_expires_at: None,
            content_len: None,
            format: 0,
        };

        assert!(CoverManager::is_legacy_catbox_image_url(&entry));

        entry.provider = "litterbox".to_string();
        assert!(CoverManager::is_legacy_catbox_image_url(&entry));

        entry.url = "https://files.catbox.moe/cover.jpg".to_string();
        assert!(!CoverManager::is_legacy_catbox_image_url(&entry));

        entry.url = "https://files.catbox.moe/cover.img".to_string();
        entry.provider = "imgbb".to_string();
        assert!(!CoverManager::is_legacy_catbox_image_url(&entry));
    }

    #[test]
    fn denies_local_private_hosts_for_direct_usage() {
        let policy = CoverManager::direct_url_policy("http://192.168.1.20:4533/cover.jpg");
        assert!(!policy.allow_direct);
        assert_eq!(policy.reason, "local_or_private_host");
    }

    #[test]
    fn denies_subsonic_cover_endpoints_for_direct_usage() {
        let policy = CoverManager::direct_url_policy(
            "https://music.example.com/rest/getCoverArt.view?id=123",
        );
        assert!(!policy.allow_direct);
        assert_eq!(policy.reason, "subsonic_cover_endpoint");
    }

    #[test]
    fn denies_jellyfin_image_endpoints_for_direct_usage() {
        let policy = CoverManager::direct_url_policy(
            "https://media.example.com/Items/abcd1234/Images/Primary?tag=abcdef",
        );
        assert!(!policy.allow_direct);
        assert_eq!(policy.reason, "jellyfin_image_endpoint");
    }

    #[test]
    fn denies_auth_like_query_params_for_direct_usage() {
        let policy =
            CoverManager::direct_url_policy("https://cdn.example.com/cover.jpg?api_key=secret");
        assert!(!policy.allow_direct);
        assert_eq!(policy.reason, "auth_query_param");

        let subsonic_policy =
            CoverManager::direct_url_policy("https://cdn.example.com/cover.jpg?t=token&s=salt");
        assert!(!subsonic_policy.allow_direct);
        assert_eq!(subsonic_policy.reason, "subsonic_token_query");
    }

    #[test]
    fn allows_plain_public_urls_for_direct_usage() {
        let policy = CoverManager::direct_url_policy("https://cdn.example.com/cover.jpg");
        assert!(policy.allow_direct);
        assert_eq!(policy.reason, "public_url");
    }
}
