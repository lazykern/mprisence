use blake3::Hasher;
use log::{debug, error, trace, warn};
use serde::{Deserialize, Serialize};
use serde_json;
use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime},
};

use crate::cover::error::CoverArtError;
use crate::metadata::MetadataSource;

const MAX_CACHE_ENTRIES: usize = 1024;
const MAX_CACHE_SIZE_BYTES: u64 = 32 * 1024 * 1024; // 32 MB soft limit
pub const MAX_CACHED_IMAGE_BYTES: usize = 8 * 1024 * 1024; // 8 MB per entry cap

#[derive(Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub url: String,
    pub provider: String,
    pub expires_at: SystemTime,
    #[serde(default = "CacheEntry::default_last_validated")]
    pub last_validated: SystemTime,
    #[serde(default)]
    pub data_file: Option<String>,
}

impl CacheEntry {
    fn default_last_validated() -> SystemTime {
        SystemTime::UNIX_EPOCH
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
    usage: Mutex<CacheUsage>,
}

impl CoverCache {
    fn ensure_parent_dir(path: &Path) -> Result<(), CoverArtError> {
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                debug!("Creating missing cache parent directory: {:?}", parent);
                fs::create_dir_all(parent).map_err(|e| {
                    error!("Failed to create cache parent directory {:?}: {}", parent, e);
                    e
                })?;
            }
        }
        Ok(())
    }

    fn entry_path_from_key<S: AsRef<str>>(&self, key: S) -> PathBuf {
        self.cache_dir.join(key.as_ref())
    }

    fn data_file_name<S: AsRef<str>>(key: S) -> String {
        format!("{}.bin", key.as_ref())
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

    fn persist_bytes(
        &self,
        key: &str,
        data: &[u8],
    ) -> Result<Option<(String, u64)>, CoverArtError> {
        if data.is_empty() {
            trace!("Skipping empty byte payload for cache key {}", key);
            return Ok(None);
        }

        if data.len() > MAX_CACHED_IMAGE_BYTES {
            warn!(
                "Cached payload for key {} exceeds limit ({} bytes > {} bytes)",
                key,
                data.len(),
                MAX_CACHED_IMAGE_BYTES
            );
            return Ok(None);
        }

        let file_name = Self::data_file_name(key);
        let path = self.data_path_from_name(&file_name);
        trace!(
            "Persisting {} cached bytes for key {} at {:?}",
            data.len(),
            key,
            path
        );

        Self::ensure_parent_dir(&path)?;

        if let Err(e) = fs::write(&path, data) {
            error!("Failed to write cached bytes for key {}: {}", key, e);
            return Err(CoverArtError::from(e));
        }

        Ok(Some((file_name, data.len() as u64)))
    }

    pub fn load_bytes(&self, entry: &CacheEntry) -> Result<Option<Vec<u8>>, CoverArtError> {
        if let Some(ref name) = entry.data_file {
            let path = self.data_path_from_name(name);
            trace!("Loading cached bytes from {:?}", path);
            return match fs::read(&path) {
                Ok(data) => {
                    if data.len() > MAX_CACHED_IMAGE_BYTES {
                        warn!(
                            "Cached payload {:?} exceeds limit ({} bytes > {} bytes); discarding",
                            path,
                            data.len(),
                            MAX_CACHED_IMAGE_BYTES
                        );
                        // Remove the corrupt data file to avoid repeatedly reading it
                        self.remove_data_file(name);
                        Ok(None)
                    } else {
                        Ok(Some(data))
                    }
                }
                Err(e) => {
                    warn!("Failed to read cached bytes {:?}: {}", path, e);
                    Ok(None)
                }
            };
        }
        Ok(None)
    }

    pub fn new(ttl: Duration) -> Result<Self, CoverArtError> {
        trace!(
            "Creating new cover cache instance with TTL: {}s",
            ttl.as_secs()
        );
        let cache_dir = Self::get_cache_directory()?;

        Self::ensure_directory(&cache_dir)?;
        debug!("Initialized cover cache in directory: {:?}", cache_dir);

        let cache = Self {
            cache_dir,
            ttl,
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
                    let now = SystemTime::now();
                    if now > entry.expires_at {
                        debug!("Cache entry expired, removing file");
                        let _ = fs::remove_file(&path);
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
                    let _ = fs::remove_file(&path);
                    Ok(None)
                }
            },
            Err(e) => {
                warn!("Failed to read cache file: {}", e);
                Ok(None)
            }
        }
    }

    pub fn store_with_key(
        &self,
        key: &str,
        provider: &str,
        url: &str,
        provider_ttl: Option<Duration>,
        cached_bytes: Option<&[u8]>,
    ) -> Result<(), CoverArtError> {
        trace!("Storing cache entry with key: {}", key);
        let path = self.entry_path_from_key(key);

        let existed_before = path.exists();
        let previous_metadata_len = if existed_before {
            fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
        } else {
            0
        };

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

        let data_file = if let Some(bytes) = cached_bytes {
            match self.persist_bytes(key, bytes)? {
                Some((file_name, len)) => {
                    self.adjust_usage(0, len as i64);
                    Some(file_name)
                }
                None => None,
            }
        } else if let Some(existing) = self.read_entry_from_path(&path) {
            if let Some(name) = existing.data_file {
                self.remove_data_file(&name);
            }
            None
        } else {
            None
        };

        let entry = CacheEntry {
            url: url.to_string(),
            provider: provider.to_string(),
            expires_at: SystemTime::now() + ttl,
            last_validated: SystemTime::now(),
            data_file: data_file.clone(),
        };

        let metadata_len = match self.persist_entry(&path, &entry) {
            Ok(len) => len,
            Err(e) => {
                if let Some(ref name) = data_file {
                    self.remove_data_file(name);
                }
                return Err(e);
            }
        };
        self.update_usage_after_write(existed_before, previous_metadata_len, metadata_len);
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
        self.update_usage_after_write(existed_before, previous_metadata_len, metadata_len);
        Ok(())
    }

    pub fn remove_by_key(&self, key: &str) -> Result<(), CoverArtError> {
        let path = self.entry_path_from_key(key);
        self.remove_entry_at_path(&path)
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
        for entry in fs::read_dir(&self.cache_dir)? {
            if let Ok(entry) = entry {
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
                        if now > entry.expires_at {
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
        }

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

    fn update_usage_after_write(
        &self,
        existed_before: bool,
        previous_metadata_len: u64,
        new_metadata_len: u64,
    ) {
        let entry_delta = if existed_before { 0 } else { 1 };
        let bytes_delta = new_metadata_len as i64 - previous_metadata_len as i64;
        self.adjust_usage(entry_delta, bytes_delta);
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

            if let Some(cache_entry) = self.read_entry_from_path(&path) {
                if let Some(ref name) = cache_entry.data_file {
                    if let Ok(data_metadata) = fs::metadata(self.data_path_from_name(name)) {
                        len = len.saturating_add(data_metadata.len());
                    }
                }
            }

            total_size = total_size.saturating_add(len);
            entries.push((path, modified, len));
        }

        if entries.len() <= MAX_CACHE_ENTRIES && total_size <= MAX_CACHE_SIZE_BYTES {
            return Ok(());
        }

        entries.sort_by(|a, b| a.1.cmp(&b.1));

        while entries.len() > MAX_CACHE_ENTRIES || total_size > MAX_CACHE_SIZE_BYTES {
            if entries.is_empty() {
                break;
            }

            let (path, _, len) = entries.remove(0);
            match self.remove_entry_at_path(&path) {
                Ok(_) => {
                    warn!("Evicted cache entry {:?}", path);
                    total_size = total_size.saturating_sub(len);
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
        usage.entries > MAX_CACHE_ENTRIES || usage.bytes > MAX_CACHE_SIZE_BYTES
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

    pub fn generate_key(metadata_source: &MetadataSource) -> String {
        trace!("Generating cache key from metadata source");
        let mut hasher = Hasher::new();
        let mut key_components = Vec::new();

        // 1. Title
        if let Some(title) = metadata_source.title() {
            if !title.is_empty() {
                key_components.push(format!("title:{}", title));
            }
        }

        // 2. Track Artists (sorted)
        if let Some(artists) = metadata_source.artists() {
            if !artists.is_empty() {
                let mut sorted_artists = artists.clone();
                sorted_artists.sort_unstable();
                key_components.push(format!("artists:{}", sorted_artists.join("|")));
            }
        }

        // 3. Album (if non-empty)
        if let Some(album) = metadata_source.album() {
            if !album.is_empty() {
                key_components.push(format!("album:{}", album));
                // 4. Album Artists (if available, non-empty, and different from track artists)
                if let Some(album_artists) = metadata_source.album_artists() {
                    if !album_artists.is_empty()
                        && Some(&album_artists) != metadata_source.artists().as_ref()
                    {
                        let mut sorted_album_artists = album_artists.clone();
                        sorted_album_artists.sort_unstable();
                        key_components
                            .push(format!("album_artists:{}", sorted_album_artists.join("|")));
                    }
                }
            }
        }

        // 5. URL (as fallback or additional differentiator)
        if let Some(url) = metadata_source.url() {
            if !url.is_empty() {
                key_components.push(format!("url:{}", url));
            }
        }

        // 6. Track ID if provided by the player
        if let Some(track_id) = metadata_source.track_id() {
            if !track_id.is_empty() {
                key_components.push(format!("track_id:{}", track_id));
            }
        }

        // If somehow still empty, use a default
        if key_components.is_empty() {
            warn!("Could not generate meaningful cache key components, using default key");
            key_components.push("default_mprisence_key".to_string());
        }

        let combined_key_data = key_components.join("||"); // Use a distinct separator
        trace!("Hashing data for key: {}", combined_key_data);
        hasher.update(combined_key_data.as_bytes());

        let key = hasher.finalize().to_hex().to_string();
        trace!("Generated cache key: {}", key);
        key
    }
}
