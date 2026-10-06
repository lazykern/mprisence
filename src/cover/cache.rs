use log::{debug, error, trace, warn};
use serde::{Deserialize, Serialize};
use serde_json;
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime},
};

use crate::config::schema::CoverCacheConfig;
use crate::cover::error::CoverArtError;

const MEBIBYTE: u64 = 1024 * 1024;

/// Layout of the cache entries this build writes. Bump it when a field
/// changes meaning, and migrate older entries instead of deleting them:
/// dropping an uploaded cover's entry means uploading the cover again.
pub const CACHE_FORMAT: u32 = 1;

#[derive(Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub url: String,
    pub provider: String,
    pub expires_at: SystemTime,
    #[serde(default = "CacheEntry::default_last_validated")]
    pub last_validated: SystemTime,
    /// Image bytes written by 1.9.0 and earlier. New entries never set it;
    /// the file is deleted when the entry is renewed or removed.
    #[serde(default)]
    pub data_file: Option<String>,
    /// When the host deletes the file. `None` means it stays until removed.
    #[serde(default)]
    pub hosted_expires_at: Option<SystemTime>,
    /// Size of the uploaded file, checked against what the host serves.
    #[serde(default)]
    pub content_len: Option<u64>,
    /// 0: written by 1.9.0 or earlier (may have a `.bin` image copy).
    /// 1: URL, host expiry and served size; no image copy.
    #[serde(default)]
    pub format: u32,
}

impl CacheEntry {
    fn default_last_validated() -> SystemTime {
        SystemTime::UNIX_EPOCH
    }

    /// The URL must be checked again before it is served.
    pub fn is_stale(&self, now: SystemTime) -> bool {
        now > self.expires_at
    }

    /// When the host deletes the file. Litterbox entries written by 1.9.0 and
    /// earlier have no `hosted_expires_at`; their `expires_at` was it.
    fn hosted_deadline(&self) -> Option<SystemTime> {
        self.hosted_expires_at.or_else(|| {
            (self.format == 0 && self.provider == "litterbox").then_some(self.expires_at)
        })
    }

    /// The host has deleted the file, so the URL is gone for good.
    pub fn is_hosted_expired(&self, now: SystemTime) -> bool {
        self.hosted_deadline().is_some_and(|expires| now >= expires)
    }

    /// Losing an uploaded cover means uploading it again; direct URLs and
    /// MusicBrainz lookups are free to redo.
    pub fn is_upload(&self) -> bool {
        !matches!(self.provider.as_str(), "direct" | "musicbrainz")
    }
}

#[derive(Default)]
struct CacheUsage {
    entries: usize,
    bytes: u64,
}

pub struct CoverCache {
    cache_dir: PathBuf,
    ttl: Duration,
    max_entries: usize,
    max_size_bytes: u64,
    usage: Mutex<CacheUsage>,
}

impl CoverCache {
    fn ensure_parent_dir(path: &Path) -> Result<(), CoverArtError> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                debug!("Creating missing cache parent directory: {:?}", parent);
                fs::create_dir_all(parent).map_err(|e| {
                    error!(
                        "Failed to create cache parent directory {:?}: {}",
                        parent, e
                    );
                    e
                })?;
            }
        }
        Ok(())
    }

    fn entry_path_from_key<S: AsRef<str>>(&self, key: S) -> PathBuf {
        self.cache_dir.join(key.as_ref())
    }

    fn data_path_from_name(&self, name: &str) -> PathBuf {
        self.cache_dir.join(name)
    }

    fn read_entry_from_path(&self, path: &Path) -> Option<CacheEntry> {
        fs::read(path)
            .ok()
            .and_then(|data| serde_json::from_slice::<CacheEntry>(&data).ok())
    }

    fn remove_data_file(&self, name: &str) {
        let path = self.data_path_from_name(name);
        if path.exists() {
            let len = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            match fs::remove_file(&path) {
                Ok(_) => {
                    if len > 0 {
                        self.adjust_usage(0, -(len as i64));
                    }
                }
                Err(e) => {
                    warn!("Failed to remove cached data file {:?}: {}", path, e);
                }
            }
        }
    }

    pub fn new(config: CoverCacheConfig) -> Result<Self, CoverArtError> {
        Self::new_in(Self::get_cache_directory()?, config)
    }

    pub(crate) fn new_in(
        cache_dir: PathBuf,
        config: CoverCacheConfig,
    ) -> Result<Self, CoverArtError> {
        let ttl = Duration::from_secs(config.ttl_hours.saturating_mul(60 * 60));
        let max_size_bytes = config.max_size_mb.saturating_mul(MEBIBYTE);
        trace!(
            "Creating cover cache with TTL {}s, {} entry limit, and {} byte limit",
            ttl.as_secs(),
            config.max_entries,
            max_size_bytes
        );

        Self::ensure_directory(&cache_dir)?;
        debug!("Initialized cover cache in directory: {:?}", cache_dir);

        let cache = Self {
            cache_dir,
            ttl,
            max_entries: config.max_entries,
            max_size_bytes,
            usage: Mutex::new(CacheUsage::default()),
        };
        cache.recalculate_usage()?;

        Ok(cache)
    }

    pub fn get_cache_directory() -> Result<PathBuf, CoverArtError> {
        trace!("Determining cache directory path");
        dirs::cache_dir()
            .map(|dir| dir.join("mprisence").join("cover_art"))
            .ok_or_else(|| {
                error!("Failed to determine system cache directory");
                let err = io::Error::new(
                    io::ErrorKind::NotFound,
                    "Could not determine cache directory",
                );
                CoverArtError::from(err)
            })
    }

    pub fn ensure_directory(dir: &PathBuf) -> Result<(), CoverArtError> {
        Self::ensure_directory_with_options(dir, true)
    }

    pub fn ensure_directory_with_options(
        dir: &PathBuf,
        verify_writable: bool,
    ) -> Result<(), CoverArtError> {
        if !dir.exists() {
            debug!("Creating cache directory: {:?}", dir);
            fs::create_dir_all(dir).map_err(|e| {
                error!("Failed to create cache directory: {:?} - {}", dir, e);
                e
            })?;
        }

        if !dir.is_dir() {
            error!("Cache path exists but is not a directory: {:?}", dir);
            return Err(CoverArtError::from(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("Cache path exists but is not a directory: {:?}", dir),
            )));
        }

        if !verify_writable {
            return Ok(());
        }

        trace!("Verifying cache directory is writable");
        let test_file = dir.join(".write_test");
        match fs::write(&test_file, b"test") {
            Ok(_) => {
                if let Err(e) = fs::remove_file(&test_file) {
                    debug!("Note: Failed to remove write test file: {}", e);
                }
                trace!("Cache directory write verification successful");
                Ok(())
            }
            Err(e) => {
                error!("Cache directory is not writable: {:?} - {}", dir, e);
                Err(CoverArtError::from(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    format!("Cache directory is not writable: {:?}", dir),
                )))
            }
        }
    }

    pub fn get_by_key<S: AsRef<str>>(&self, key: S) -> Result<Option<CacheEntry>, CoverArtError> {
        let key = key.as_ref();
        trace!("Looking up cache entry with key: {}", key);
        let path = self.entry_path_from_key(key);

        if !path.exists() {
            trace!("No cache entry found");
            return Ok(None);
        }

        match fs::read(&path) {
            Ok(data) => match serde_json::from_slice::<CacheEntry>(&data) {
                Ok(entry) => {
                    if entry.is_hosted_expired(SystemTime::now()) {
                        debug!("Hosted cover art expired, removing cache entry");
                        if let Err(err) = self.remove_entry_at_path(&path) {
                            warn!("Failed to remove expired cache entry {:?}: {}", path, err);
                        }
                        return Ok(None);
                    }

                    debug!("Found valid cache entry from provider: {}", entry.provider);
                    trace!("Cached URL: {}", entry.url);
                    Ok(Some(entry))
                }
                Err(e) => {
                    warn!(
                        "Failed to deserialize cache entry, removing corrupt file: {}",
                        e
                    );
                    if let Err(err) = self.remove_entry_at_path(&path) {
                        warn!("Failed to remove corrupt cache entry {:?}: {}", path, err);
                    }
                    Ok(None)
                }
            },
            Err(e) => {
                warn!("Failed to read cache file: {}", e);
                Ok(None)
            }
        }
    }

    /// Store a cover URL. An entry that is not `verified` is stale from the
    /// start: it is used now but checked before the fast path serves it again.
    pub fn store_with_key(
        &self,
        key: &str,
        provider: &str,
        url: &str,
        provider_ttl: Option<Duration>,
        content_len: Option<u64>,
        verified: bool,
    ) -> Result<(), CoverArtError> {
        trace!("Storing cache entry with key: {}", key);
        let path = self.entry_path_from_key(key);

        let existed_before = path.exists();
        let previous_metadata_len = if existed_before {
            fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };
        if let Some(previous_data_file) = self
            .read_entry_from_path(&path)
            .and_then(|entry| entry.data_file)
        {
            self.remove_data_file(&previous_data_file);
        }

        let ttl = provider_ttl
            .map(|ttl| {
                if ttl < self.ttl {
                    debug!(
                        "Using provider TTL override: {}s (default cache TTL: {}s)",
                        ttl.as_secs(),
                        self.ttl.as_secs()
                    );
                } else {
                    trace!(
                        "Provider TTL {}s exceeds cache TTL, capping at {}s",
                        ttl.as_secs(),
                        self.ttl.as_secs()
                    );
                }
                ttl.min(self.ttl)
            })
            .unwrap_or(self.ttl);

        let now = SystemTime::now();
        let entry = CacheEntry {
            url: url.to_string(),
            provider: provider.to_string(),
            expires_at: if verified { now + ttl } else { now },
            last_validated: if verified {
                now
            } else {
                SystemTime::UNIX_EPOCH
            },
            data_file: None,
            hosted_expires_at: provider_ttl.map(|ttl| now + ttl),
            content_len,
            format: CACHE_FORMAT,
        };

        let metadata_len = self.persist_entry(&path, &entry)?;
        self.adjust_usage(
            if existed_before { 0 } else { 1 },
            metadata_len as i64 - previous_metadata_len as i64,
        );
        if self.usage_exceeds_limits() {
            self.enforce_limits()?;
        }

        debug!(
            "Successfully stored cache entry from provider: {}",
            provider
        );
        trace!("Cache entry will expire at: {:?}", entry.expires_at);

        Ok(())
    }

    /// Mark `entry` as freshly validated and push its recheck time forward,
    /// never past the moment the host deletes the file.
    pub fn renew(&self, entry: &mut CacheEntry) {
        if let Some(data_file) = entry.data_file.take() {
            self.remove_data_file(&data_file);
        }
        entry.last_validated = SystemTime::now();
        Self::recheck_after(entry, self.ttl);
        entry.format = entry.format.max(CACHE_FORMAT);
    }

    /// Serve `entry` for `delay` without validating it; used when its host
    /// could not be reached.
    pub fn postpone(&self, entry: &mut CacheEntry, delay: Duration) {
        Self::recheck_after(entry, delay.min(self.ttl));
    }

    fn recheck_after(entry: &mut CacheEntry, delay: Duration) {
        entry.hosted_expires_at = entry.hosted_deadline();
        let recheck_at = SystemTime::now() + delay;
        entry.expires_at = entry
            .hosted_expires_at
            .map_or(recheck_at, |hosted| hosted.min(recheck_at));
    }

    pub fn update_entry_with_key(
        &self,
        key: &str,
        entry: &CacheEntry,
    ) -> Result<(), CoverArtError> {
        let path = self.entry_path_from_key(key);
        trace!(
            "Refreshing cache entry validation timestamp for provider {}",
            entry.provider
        );
        let existed_before = path.exists();
        let previous_metadata_len = if existed_before {
            fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };
        let metadata_len = self.persist_entry(&path, entry)?;
        self.adjust_usage(
            if existed_before { 0 } else { 1 },
            metadata_len as i64 - previous_metadata_len as i64,
        );
        Ok(())
    }

    pub fn remove_by_key(&self, key: &str) -> Result<(), CoverArtError> {
        let path = self.entry_path_from_key(key);
        self.remove_entry_at_path(&path)
    }

    fn holds_url(&self, key: &str, url: &str) -> bool {
        self.read_entry_from_path(&self.entry_path_from_key(key))
            .is_some_and(|entry| entry.url == url)
    }

    /// Remove the entry only if it still holds `url`; a concurrent lookup may
    /// have stored a replacement since it was read.
    pub fn remove_by_key_if_url(&self, key: &str, url: &str) -> Result<(), CoverArtError> {
        if self.holds_url(key, url) {
            self.remove_by_key(key)?;
        }
        Ok(())
    }

    /// Write `entry` back only if the stored entry still holds the same URL
    /// and was not written by a newer build, whose extra fields this build
    /// would drop.
    pub fn update_entry_if_url(&self, key: &str, entry: &CacheEntry) -> Result<(), CoverArtError> {
        let stored = self.read_entry_from_path(&self.entry_path_from_key(key));
        if stored.is_some_and(|stored| stored.url == entry.url && stored.format <= CACHE_FORMAT) {
            self.update_entry_with_key(key, entry)?;
        }
        Ok(())
    }

    fn remove_entry_at_path(&self, path: &Path) -> Result<(), CoverArtError> {
        if path.exists() {
            let metadata_len = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            if let Some(existing) = self.read_entry_from_path(path) {
                if let Some(name) = existing.data_file {
                    self.remove_data_file(&name);
                }
            }
            trace!("Removing cache file {:?}", path);
            fs::remove_file(path).map_err(|e| {
                error!("Failed to remove cache entry {:?}: {}", path, e);
                e
            })?;
            self.adjust_usage(-1, -(metadata_len as i64));
        }
        Ok(())
    }

    pub fn clean(&self) -> Result<usize, CoverArtError> {
        let mut cleaned = 0;
        let now = SystemTime::now();

        trace!("Starting cache cleanup scan");
        for entry in (fs::read_dir(&self.cache_dir)?).flatten() {
            let path = entry.path();
            if path.is_dir() {
                trace!("Skipping directory: {:?}", path);
                continue;
            }

            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("bin"))
                .unwrap_or(false)
            {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    let metadata_path = self.entry_path_from_key(stem);
                    if !metadata_path.exists() {
                        debug!(
                            "Removing orphaned cached data file {:?} (missing {:?})",
                            path, metadata_path
                        );
                        if let Some(file_name) = path.file_name().and_then(|f| f.to_str()) {
                            self.remove_data_file(file_name);
                        } else {
                            let len = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                            if let Err(e) = fs::remove_file(&path) {
                                warn!("Failed to remove orphaned cache data {:?}: {}", path, e);
                            } else if len > 0 {
                                self.adjust_usage(0, -(len as i64));
                            }
                        }
                    }
                }
                continue;
            }

            trace!("Checking cache file: {:?}", path);
            if let Ok(data) = fs::read(&path) {
                if let Ok(entry) = serde_json::from_slice::<CacheEntry>(&data) {
                    if entry.is_hosted_expired(now) {
                        debug!("Removing expired cache entry: {:?}", path);
                        match self.remove_entry_at_path(&path) {
                            Ok(_) => cleaned += 1,
                            Err(_) => {
                                warn!("Failed to remove expired cache entry: {:?}", path);
                            }
                        }
                    }
                } else {
                    warn!("Removing invalid cache entry: {:?}", path);
                    match self.remove_entry_at_path(&path) {
                        Ok(_) => cleaned += 1,
                        Err(_) => warn!("Failed to cleanup invalid cache entry: {:?}", path),
                    }
                }
            }
        }

        self.enforce_limits()?;
        debug!("Cache cleanup completed, removed {} entries", cleaned);
        Ok(cleaned)
    }

    fn persist_entry(&self, path: &Path, entry: &CacheEntry) -> Result<u64, CoverArtError> {
        let data = serde_json::to_vec(entry).map_err(|e| {
            error!("Failed to serialize cache entry: {}", e);
            CoverArtError::json_error(e)
        })?;

        Self::ensure_parent_dir(path)?;

        fs::write(path, &data).map_err(|e| {
            error!("Failed to write cache entry to disk: {}", e);
            e
        })?;

        Ok(data.len() as u64)
    }

    fn enforce_limits(&self) -> Result<(), CoverArtError> {
        let mut entries = Vec::new();
        let mut total_size: u64 = 0;

        for entry in fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if !metadata.is_file() {
                continue;
            }

            let path = entry.path();
            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("bin"))
                .unwrap_or(false)
            {
                continue;
            }

            let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
            let mut len = metadata.len();
            let mut upload = false;

            if let Some(cache_entry) = self.read_entry_from_path(&path) {
                upload = cache_entry.is_upload();
                if let Some(ref name) = cache_entry.data_file {
                    if let Ok(data_metadata) = fs::metadata(self.data_path_from_name(name)) {
                        len = len.saturating_add(data_metadata.len());
                    }
                }
            }

            total_size = total_size.saturating_add(len);
            entries.push((upload, modified, path, len));
        }

        if entries.len() <= self.max_entries && total_size <= self.max_size_bytes {
            return Ok(());
        }

        // Evict entries that are free to recreate before uploads, oldest first.
        entries.sort_by_key(|entry| (entry.0, entry.1));

        let mut remaining = entries.len();
        for (_, _, path, len) in entries {
            if remaining <= self.max_entries && total_size <= self.max_size_bytes {
                break;
            }
            match self.remove_entry_at_path(&path) {
                Ok(_) => {
                    warn!("Evicted cache entry {:?}", path);
                    total_size = total_size.saturating_sub(len);
                    remaining -= 1;
                }
                Err(_) => {
                    warn!("Failed to evict cache entry {:?}", path);
                }
            }
        }

        Ok(())
    }

    fn usage_exceeds_limits(&self) -> bool {
        let usage = self.usage.lock().unwrap();
        usage.entries > self.max_entries || usage.bytes > self.max_size_bytes
    }

    fn adjust_usage(&self, entries_delta: isize, bytes_delta: i64) {
        if entries_delta == 0 && bytes_delta == 0 {
            return;
        }

        let mut usage = self.usage.lock().unwrap();
        if entries_delta >= 0 {
            usage.entries = usage.entries.saturating_add(entries_delta as usize);
        } else {
            let delta = (-entries_delta) as usize;
            usage.entries = usage.entries.saturating_sub(delta);
        }

        if bytes_delta >= 0 {
            usage.bytes = usage.bytes.saturating_add(bytes_delta as u64);
        } else {
            let delta = (-bytes_delta) as u64;
            usage.bytes = usage.bytes.saturating_sub(delta);
        }
    }

    fn recalculate_usage(&self) -> Result<(), CoverArtError> {
        let mut usage = CacheUsage::default();

        for entry in fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                continue;
            }

            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext.eq_ignore_ascii_case("bin"))
                .unwrap_or(false)
            {
                let metadata_path = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|stem| self.entry_path_from_key(stem));
                if metadata_path.as_ref().map(|p| !p.exists()).unwrap_or(true) {
                    if let Ok(meta) = fs::metadata(&path) {
                        usage.bytes = usage.bytes.saturating_add(meta.len());
                    }
                }
                continue;
            }

            usage.entries = usage.entries.saturating_add(1);
            if let Ok(meta) = fs::metadata(&path) {
                usage.bytes = usage.bytes.saturating_add(meta.len());
            }

            if let Some(cache_entry) = self.read_entry_from_path(&path) {
                if let Some(ref name) = cache_entry.data_file {
                    let data_path = self.data_path_from_name(name);
                    if let Ok(meta) = fs::metadata(&data_path) {
                        usage.bytes = usage.bytes.saturating_add(meta.len());
                    }
                }
            }
        }

        let mut lock = self.usage.lock().unwrap();
        *lock = usage;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_cache_dir() -> PathBuf {
        static NEXT_DIR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "mprisence-cover-cache-test-{}-{}-{}",
            std::process::id(),
            unique,
            NEXT_DIR.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn cache_config(max_size_mb: u64, max_entries: usize) -> CoverCacheConfig {
        CoverCacheConfig {
            max_size_mb,
            max_entries,
            ttl_hours: 24,
        }
    }

    #[test]
    fn configured_entry_limit_is_enforced() {
        let dir = temp_cache_dir();
        let cache = CoverCache::new_in(dir.clone(), cache_config(32, 1)).expect("create cache");

        cache
            .store_with_key(
                "first",
                "test",
                "https://example.com/first",
                None,
                None,
                true,
            )
            .expect("store first entry");
        cache
            .store_with_key(
                "second",
                "test",
                "https://example.com/second",
                None,
                None,
                true,
            )
            .expect("store second entry");

        let usage = cache.usage.lock().expect("read cache usage");
        assert_eq!(usage.entries, 1);
        drop(usage);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn configured_size_limit_is_enforced() {
        let dir = temp_cache_dir();
        let initial = CoverCache::new_in(dir.clone(), cache_config(32, 10)).expect("create cache");
        for key in ["first", "second"] {
            write_legacy_entry(&initial, key, 700 * 1024);
        }
        drop(initial);

        let cache = CoverCache::new_in(dir.clone(), cache_config(1, 10)).expect("reopen cache");
        cache.clean().expect("clean cache");

        let usage = cache.usage.lock().expect("read cache usage");
        assert!(usage.bytes <= MEBIBYTE);
        assert_eq!(usage.entries, 1);
        drop(usage);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn cleanup_applies_reduced_limits_to_existing_entries() {
        let dir = temp_cache_dir();
        let initial = CoverCache::new_in(dir.clone(), cache_config(32, 2)).expect("create cache");
        initial
            .store_with_key(
                "first",
                "test",
                "https://example.com/first",
                None,
                None,
                true,
            )
            .expect("store first entry");
        initial
            .store_with_key(
                "second",
                "test",
                "https://example.com/second",
                None,
                None,
                true,
            )
            .expect("store second entry");
        drop(initial);

        let reduced = CoverCache::new_in(dir.clone(), cache_config(32, 1)).expect("reopen cache");
        reduced.clean().expect("clean cache");

        let usage = reduced.usage.lock().expect("read cache usage");
        assert_eq!(usage.entries, 1);
        drop(usage);
        let _ = fs::remove_dir_all(dir);
    }

    fn write_entry(cache: &CoverCache, key: &str, entry: &CacheEntry) {
        cache
            .persist_entry(&cache.entry_path_from_key(key), entry)
            .expect("write entry");
    }

    fn entry(expires_at: SystemTime, hosted_expires_at: Option<SystemTime>) -> CacheEntry {
        CacheEntry {
            url: "https://example.com/cover.jpg".to_string(),
            provider: "test".to_string(),
            expires_at,
            last_validated: SystemTime::UNIX_EPOCH,
            data_file: None,
            hosted_expires_at,
            content_len: None,
            format: 0,
        }
    }

    /// An entry as 1.9.0 wrote it, with its image in a `.bin` file.
    fn write_legacy_entry(cache: &CoverCache, key: &str, image_len: usize) -> PathBuf {
        let bin = cache.data_path_from_name(&format!("{key}.bin"));
        fs::write(&bin, vec![0_u8; image_len]).expect("write image bytes");
        let mut legacy = entry(SystemTime::now() + Duration::from_secs(3600), None);
        legacy.data_file = Some(format!("{key}.bin"));
        write_entry(cache, key, &legacy);
        bin
    }

    fn store(cache: &CoverCache, key: &str, provider: &str, age_secs: u64) {
        cache
            .store_with_key(
                key,
                provider,
                "https://example.com/c.jpg",
                None,
                Some(1),
                true,
            )
            .expect("store entry");
        // The entry may already be evicted by its own store.
        if let Ok(file) = fs::File::options()
            .write(true)
            .open(cache.entry_path_from_key(key))
        {
            let modified = SystemTime::now() - Duration::from_secs(age_secs);
            file.set_modified(modified).expect("set mtime");
        }
    }

    fn keys(cache: &CoverCache) -> Vec<String> {
        let mut keys: Vec<_> = fs::read_dir(&cache.cache_dir)
            .unwrap()
            .flatten()
            .map(|file| file.file_name().to_string_lossy().into_owned())
            .collect();
        keys.sort();
        keys
    }

    #[test]
    fn eviction_keeps_uploads_over_entries_that_are_free_to_recreate() {
        let dir = temp_cache_dir();
        let cache = CoverCache::new_in(dir.clone(), cache_config(32, 3)).expect("create cache");
        store(&cache, "u1", "imgbb", 50);
        store(&cache, "u2", "catbox", 40);
        store(&cache, "d1", "direct", 30);
        store(&cache, "m1", "musicbrainz", 20);
        assert_eq!(
            keys(&cache),
            ["m1", "u1", "u2"],
            "oldest cheap entry goes first"
        );

        store(&cache, "u3", "litterbox", 10);
        assert_eq!(keys(&cache), ["u1", "u2", "u3"]);

        // Full of uploads: a new direct entry is the one evicted.
        store(&cache, "d2", "direct", 0);
        assert_eq!(keys(&cache), ["u1", "u2", "u3"]);

        store(&cache, "u4", "imgbb", 0);
        assert_eq!(keys(&cache), ["u2", "u3", "u4"], "then the oldest upload");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn new_entries_record_length_and_keep_no_image_bytes() {
        let dir = temp_cache_dir();
        let cache = CoverCache::new_in(dir.clone(), cache_config(32, 10)).expect("create cache");
        let bin = write_legacy_entry(&cache, "cover", 4096);

        cache
            .store_with_key(
                "cover",
                "imgbb",
                "https://example.com/new.jpg",
                None,
                Some(1234),
                true,
            )
            .expect("store entry");

        let entry = cache.get_by_key("cover").unwrap().unwrap();
        assert_eq!(entry.content_len, Some(1234));
        assert_eq!(entry.data_file, None);
        assert!(!bin.exists(), "previous entry's image bytes removed");
        assert!(!entry.is_stale(SystemTime::now()));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unverified_entry_is_stale_from_the_start() {
        let dir = temp_cache_dir();
        let cache = CoverCache::new_in(dir.clone(), cache_config(32, 10)).expect("create cache");
        cache
            .store_with_key(
                "cover",
                "imgbb",
                "https://example.com/c.jpg",
                None,
                Some(10),
                false,
            )
            .expect("store entry");

        let entry = cache.get_by_key("cover").unwrap().unwrap();
        assert!(entry.is_stale(SystemTime::now()));
        assert_eq!(entry.content_len, Some(10));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn renew_deletes_leftover_image_bytes() {
        let dir = temp_cache_dir();
        let initial = CoverCache::new_in(dir.clone(), cache_config(32, 10)).expect("create cache");
        let bin = write_legacy_entry(&initial, "cover", 4096);
        drop(initial);

        let cache = CoverCache::new_in(dir.clone(), cache_config(32, 10)).expect("reopen cache");
        let before = cache.usage.lock().unwrap().bytes;
        let mut entry = cache.get_by_key("cover").unwrap().unwrap();
        cache.renew(&mut entry);
        cache.update_entry_with_key("cover", &entry).unwrap();

        assert!(!bin.exists());
        assert_eq!(cache.get_by_key("cover").unwrap().unwrap().data_file, None);
        let after = cache.usage.lock().unwrap().bytes;
        assert!(after + 4000 < before, "{before} -> {after}");
        cache.recalculate_usage().unwrap();
        assert_eq!(
            after,
            cache.usage.lock().unwrap().bytes,
            "tracked usage matches disk"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn removal_and_write_back_skip_a_replaced_entry() {
        let dir = temp_cache_dir();
        let cache = CoverCache::new_in(dir.clone(), cache_config(32, 10)).expect("create cache");
        let mut stale = entry(SystemTime::now(), None);
        stale.url = "https://example.com/old.jpg".to_string();
        cache
            .store_with_key(
                "cover",
                "imgbb",
                "https://example.com/new.jpg",
                None,
                Some(1),
                true,
            )
            .expect("store replacement");

        cache.remove_by_key_if_url("cover", &stale.url).unwrap();
        cache.update_entry_if_url("cover", &stale).unwrap();
        assert_eq!(
            cache.get_by_key("cover").unwrap().unwrap().url,
            "https://example.com/new.jpg"
        );

        cache
            .remove_by_key_if_url("cover", "https://example.com/new.jpg")
            .unwrap();
        assert!(cache.get_by_key("cover").unwrap().is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn legacy_litterbox_entry_keeps_its_expiry_when_renewed() {
        let dir = temp_cache_dir();
        let cache = CoverCache::new_in(dir.clone(), cache_config(32, 10)).expect("create cache");
        let deadline = SystemTime::now() + Duration::from_secs(600);
        let mut legacy = entry(deadline, None);
        legacy.provider = "litterbox".to_string();

        cache.renew(&mut legacy);
        assert_eq!(legacy.hosted_expires_at, Some(deadline));
        assert_eq!(legacy.expires_at, deadline);

        let mut catbox = entry(deadline, None);
        catbox.provider = "catbox".to_string();
        cache.renew(&mut catbox);
        assert_eq!(catbox.hosted_expires_at, None, "Catbox keeps files");
        assert!(catbox.expires_at > deadline);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn postpone_is_capped_by_hosted_expiry() {
        let dir = temp_cache_dir();
        let cache = CoverCache::new_in(dir.clone(), cache_config(32, 10)).expect("create cache");
        let hosted = SystemTime::now() + Duration::from_secs(60);
        let mut expiring = entry(SystemTime::UNIX_EPOCH, Some(hosted));
        cache.postpone(&mut expiring, Duration::from_secs(3600));
        assert_eq!(expiring.expires_at, hosted);
        assert_eq!(
            expiring.last_validated,
            SystemTime::UNIX_EPOCH,
            "not validated"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn entries_record_their_format() {
        let dir = temp_cache_dir();
        let cache = CoverCache::new_in(dir.clone(), cache_config(32, 10)).expect("create cache");
        write_legacy_entry(&cache, "old", 16);
        let mut old = cache.get_by_key("old").unwrap().unwrap();
        assert_eq!(old.format, 0, "1.9.0 entries read as format 0");

        cache
            .store_with_key(
                "new",
                "imgbb",
                "https://example.com/n.jpg",
                None,
                Some(1),
                true,
            )
            .expect("store entry");
        assert_eq!(
            cache.get_by_key("new").unwrap().unwrap().format,
            CACHE_FORMAT
        );

        cache.renew(&mut old);
        assert_eq!(old.format, CACHE_FORMAT, "renewing upgrades an old entry");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn entries_from_a_newer_build_are_never_rewritten() {
        let dir = temp_cache_dir();
        let cache = CoverCache::new_in(dir.clone(), cache_config(32, 10)).expect("create cache");
        let path = cache.entry_path_from_key("future");
        let written = br#"{"url":"https://example.com/f.jpg","provider":"imgbb","expires_at":{"secs_since_epoch":1,"nanos_since_epoch":0},"format":99,"field_from_the_future":true}"#;
        fs::write(&path, written).unwrap();

        let mut entry = cache.get_by_key("future").unwrap().unwrap();
        assert_eq!(entry.format, 99);
        cache.renew(&mut entry);
        assert_eq!(entry.format, 99, "never downgraded");
        cache.update_entry_if_url("future", &entry).unwrap();
        assert_eq!(fs::read(&path).unwrap(), written, "file untouched");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn current_litterbox_entries_are_not_guessed_at() {
        let deadline = SystemTime::now() - Duration::from_secs(60);
        let mut current = entry(deadline, None);
        current.provider = "litterbox".to_string();
        current.format = CACHE_FORMAT;
        assert!(!current.is_hosted_expired(SystemTime::now()));

        current.format = 0;
        assert!(current.is_hosted_expired(SystemTime::now()));
    }

    #[test]
    fn clean_removes_orphaned_image_bytes() {
        let dir = temp_cache_dir();
        let cache = CoverCache::new_in(dir.clone(), cache_config(32, 10)).expect("create cache");
        let orphan = dir.join("gone.bin");
        fs::write(&orphan, [0_u8; 16]).unwrap();

        cache.clean().expect("clean cache");
        assert!(!orphan.exists());
        let _ = fs::remove_dir_all(dir);
    }

    /// `cargo test --lib clean_scales -- --ignored --nocapture`
    #[test]
    #[ignore = "timing report"]
    fn clean_scales_to_the_default_entry_limit() {
        let dir = temp_cache_dir();
        let cache =
            CoverCache::new_in(dir.clone(), CoverCacheConfig::default()).expect("create cache");
        let template = entry(SystemTime::now() + Duration::from_secs(3600), None);
        for i in 0..CoverCacheConfig::default().max_entries {
            write_entry(&cache, &format!("entry-{i}"), &template);
        }
        drop(cache);

        let started = std::time::Instant::now();
        let cache =
            CoverCache::new_in(dir.clone(), CoverCacheConfig::default()).expect("reopen cache");
        let opened = started.elapsed();
        cache.clean().expect("clean cache");
        println!(
            "{} entries: open {:?}, clean {:?}",
            CoverCacheConfig::default().max_entries,
            opened,
            started.elapsed() - opened
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn entries_without_hosted_expiry_still_deserialize() {
        let legacy = br#"{"url":"https://example.com/a.jpg","provider":"imgbb","expires_at":{"secs_since_epoch":1,"nanos_since_epoch":0}}"#;
        let entry: CacheEntry = serde_json::from_slice(legacy).expect("legacy entry");
        assert_eq!(entry.hosted_expires_at, None);
        assert!(entry.is_stale(SystemTime::now()));
        assert!(!entry.is_hosted_expired(SystemTime::now()));
    }

    #[test]
    fn stale_entries_are_kept_until_the_host_deletes_the_file() {
        let dir = temp_cache_dir();
        let cache = CoverCache::new_in(dir.clone(), cache_config(32, 10)).expect("create cache");
        let past = SystemTime::now() - Duration::from_secs(3600);
        let future = SystemTime::now() + Duration::from_secs(3600);
        write_entry(&cache, "permanent", &entry(past, None));
        write_entry(&cache, "hosted-alive", &entry(past, Some(future)));
        write_entry(&cache, "hosted-gone", &entry(past, Some(past)));

        assert_eq!(cache.clean().expect("clean cache"), 1);
        assert!(cache.get_by_key("permanent").unwrap().is_some());
        assert!(cache.get_by_key("hosted-alive").unwrap().is_some());
        assert!(cache.get_by_key("hosted-gone").unwrap().is_none());

        write_entry(&cache, "hosted-gone", &entry(past, Some(past)));
        assert!(cache.get_by_key("hosted-gone").unwrap().is_none());
        assert!(!cache.entry_path_from_key("hosted-gone").exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn renew_never_extends_past_hosted_expiry() {
        let dir = temp_cache_dir();
        let cache = CoverCache::new_in(dir.clone(), cache_config(32, 10)).expect("create cache");
        let past = SystemTime::now() - Duration::from_secs(3600);

        let mut permanent = entry(past, None);
        cache.renew(&mut permanent);
        assert!(permanent.expires_at > SystemTime::now() + Duration::from_secs(23 * 3600));
        assert!(permanent.last_validated > past);

        let hosted = SystemTime::now() + Duration::from_secs(600);
        let mut expiring = entry(past, Some(hosted));
        cache.renew(&mut expiring);
        assert_eq!(expiring.expires_at, hosted);
        let _ = fs::remove_dir_all(dir);
    }
}
