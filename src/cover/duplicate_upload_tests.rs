//! Regression tests for #109: a cover is uploaded once and its URL reused for
//! as long as the host keeps the file. Hosts such as ImgBB store every upload
//! that falls outside their own duplicate window as a new image.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use image::{codecs::jpeg::JpegEncoder, DynamicImage, Rgb, RgbImage};
use mpris::{Metadata, MetadataValue};
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;

use super::cache::{CacheEntry, CoverCache};
use super::error::CoverArtError;
use super::providers::catbox::CatboxProvider;
use super::providers::imgbb::ImgbbProvider;
use super::providers::{CoverArtProvider, CoverResult};
use super::sources::ArtSource;
use super::{CoverManager, Timeouts};
use crate::config::schema::{CatboxConfig, Config, CoverCacheConfig, ImgBBConfig};
use crate::config::ConfigManager;
use crate::metadata::MetadataSource;

/// What the fake CDN answers for one hosted file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Served {
    /// 200 with this many bytes; honours `Range: bytes=0-0`.
    File(usize),
    /// A broken upload: 200 with 0 bytes to HEAD and GET alike.
    Empty,
    /// An empty file that answers a range request with 416 and
    /// `Content-Range: bytes */0`.
    EmptyRefusingRanges,
    /// A 416 that does not say the file is empty.
    RangeRefusedWithoutSize,
    /// HEAD says 0; GET ignores the range and sends an empty chunked body.
    ChunkedEmpty,
    /// HEAD says 0; GET ignores the range, sends one chunk, then stalls.
    ChunkedFile,
    /// A Catbox file that does not exist: HEAD says 200 with 0 bytes, and a
    /// GET never answers.
    Vanished,
    /// 404, like a deleted ImgBB image.
    Missing,
    /// 200 with `Content-Encoding: gzip`, so the length is not the file's.
    Compressed(usize),
    /// The connection is dropped.
    Unreachable,
    /// The connection is accepted but never answered.
    Hangs,
}

/// How a fake host stores an upload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stored {
    Intact,
    Empty,
    Truncated,
    Unreachable,
    Hangs,
    /// 404 for a moment after the upload, then served normally.
    LateVisible,
}

struct Cdn {
    base: String,
    files: Mutex<HashMap<String, Served>>,
    network_down: AtomicBool,
    gets: AtomicUsize,
    /// Catbox: every HEAD says `Content-Length: 0`, even for real files.
    head_reports_zero: AtomicBool,
}

impl Cdn {
    fn spawn() -> Arc<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let cdn = Arc::new(Self {
            base: format!("http://{}", listener.local_addr().unwrap()),
            files: Mutex::default(),
            network_down: AtomicBool::new(false),
            gets: AtomicUsize::new(0),
            head_reports_zero: AtomicBool::new(false),
        });
        let server = Arc::clone(&cdn);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let server = Arc::clone(&server);
                std::thread::spawn(move || server.answer(stream));
            }
        });
        cdn
    }

    fn answer(&self, mut stream: TcpStream) {
        if self.network_down.load(SeqCst) {
            return;
        }
        let mut buf = [0_u8; 4096];
        let n = stream.read(&mut buf).unwrap_or(0);
        let request = String::from_utf8_lossy(&buf[..n]);
        let mut words = request.split_whitespace();
        let (method, path) = (words.next().unwrap_or(""), words.next().unwrap_or(""));
        let is_get = method == "GET";
        if is_get {
            self.gets.fetch_add(1, SeqCst);
        }
        let ranged = request
            .to_ascii_lowercase()
            .contains("\r\nrange: bytes=0-0");
        let head_zero = self.head_reports_zero.load(SeqCst);
        let served = self.served(path);
        if is_get && matches!(served, Served::ChunkedEmpty | Served::ChunkedFile) {
            let head = "HTTP/1.1 200 OK\r\nContent-Type: image/jpeg\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n";
            let _ = stream.write_all(head.as_bytes());
            if served == Served::ChunkedEmpty {
                let _ = stream.write_all(b"0\r\n\r\n");
            } else {
                let _ = stream.write_all(b"5\r\nhello\r\n");
                let _ = stream.flush();
                std::thread::sleep(Duration::from_secs(60));
            }
            return;
        }
        let (status, headers, body_len) = match served {
            Served::File(len) if is_get && ranged && len > 0 => (
                "206 Partial Content",
                format!("Content-Range: bytes 0-0/{len}\r\n"),
                1,
            ),
            Served::File(_) if !is_get && head_zero => ("200 OK", String::new(), 0),
            Served::File(len) => ("200 OK", String::new(), len),
            Served::Empty => ("200 OK", String::new(), 0),
            Served::EmptyRefusingRanges if is_get && ranged => (
                "416 Range Not Satisfiable",
                "Content-Range: bytes */0\r\n".to_string(),
                0,
            ),
            Served::RangeRefusedWithoutSize if is_get && ranged => {
                ("416 Range Not Satisfiable", String::new(), 0)
            }
            Served::EmptyRefusingRanges
            | Served::RangeRefusedWithoutSize
            | Served::ChunkedEmpty
            | Served::ChunkedFile => ("200 OK", String::new(), 0),
            Served::Vanished if is_get => {
                std::thread::sleep(Duration::from_secs(60));
                return;
            }
            Served::Vanished => ("200 OK", String::new(), 0),
            Served::Missing => ("404 Not Found", String::new(), 0),
            Served::Compressed(len) => ("200 OK", "Content-Encoding: gzip\r\n".to_string(), len),
            Served::Unreachable => return,
            Served::Hangs => {
                std::thread::sleep(Duration::from_secs(60));
                return;
            }
        };
        let head = format!(
            "HTTP/1.1 {status}\r\nContent-Type: image/jpeg\r\n{headers}Content-Length: {body_len}\r\nConnection: close\r\n\r\n"
        );
        let _ = stream.write_all(head.as_bytes());
        if is_get {
            let _ = stream.write_all(&vec![0_u8; body_len]);
        }
    }

    fn served(&self, path: &str) -> Served {
        self.files
            .lock()
            .unwrap()
            .get(path)
            .copied()
            .unwrap_or(Served::Missing)
    }

    fn set(&self, url: &str, served: Served) {
        let path = url.strip_prefix(&self.base).unwrap_or(url).to_string();
        self.files.lock().unwrap().insert(path, served);
    }

    fn gets(&self) -> usize {
        self.gets.load(SeqCst)
    }
}

/// Permanent host (ImgBB with expiration = 0, or Catbox). Counts every
/// upload the server receives and publishes it on the fake CDN.
struct FakeHost {
    name: &'static str,
    cdn: Arc<Cdn>,
    uploads: Arc<AtomicUsize>,
    latency: Duration,
    lifetime: Option<Duration>,
    stored: Mutex<Stored>,
}

impl FakeHost {
    fn new(cdn: &Arc<Cdn>, uploads: &Arc<AtomicUsize>) -> Self {
        Self {
            name: "fakehost",
            cdn: Arc::clone(cdn),
            uploads: Arc::clone(uploads),
            latency: Duration::ZERO,
            lifetime: None,
            stored: Mutex::new(Stored::Intact),
        }
    }
}

#[async_trait]
impl CoverArtProvider for FakeHost {
    fn name(&self) -> &'static str {
        self.name
    }

    fn supports_source_type(&self, source: &ArtSource) -> bool {
        !matches!(source, ArtSource::Url(_))
    }

    async fn process(
        &self,
        source: ArtSource,
        _metadata: &MetadataSource,
        _cancel: &CancellationToken,
    ) -> Result<Option<CoverResult>, CoverArtError> {
        if self.cdn.network_down.load(SeqCst) {
            return Err(CoverArtError::provider_error(
                "fakehost",
                "network unreachable",
            ));
        }
        let len = source.materialize_bytes().await?.unwrap().len();
        // The server has the file once the request body is sent.
        let n = self.uploads.fetch_add(1, SeqCst) + 1;
        let url = format!("{}/{}-{n}.jpg", self.cdn.base, self.name);
        let served = match *self.stored.lock().unwrap() {
            Stored::Intact => Served::File(len),
            Stored::Empty => Served::Empty,
            Stored::Truncated => Served::File(len / 2),
            Stored::Unreachable => Served::Unreachable,
            Stored::Hangs => Served::Hangs,
            Stored::LateVisible => {
                let (cdn, url) = (Arc::clone(&self.cdn), url.clone());
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(150));
                    cdn.set(&url, Served::File(len));
                });
                Served::Missing
            }
        };
        self.cdn.set(&url, served);
        tokio::time::sleep(self.latency).await;
        Ok(Some(CoverResult {
            url,
            provider: self.name.to_string(),
            expiration: self.lifetime,
        }))
    }
}

struct Harness {
    manager: CoverManager,
    cdn: Arc<Cdn>,
    uploads: Arc<AtomicUsize>,
    cache_dir: PathBuf,
}

impl Harness {
    fn new(latency: Duration) -> Self {
        Self::with_host(|host| host.latency = latency)
    }

    fn with_host(setup: impl FnOnce(&mut FakeHost)) -> Self {
        let cdn = Cdn::spawn();
        let uploads = Arc::new(AtomicUsize::new(0));
        let mut host = FakeHost::new(&cdn, &uploads);
        setup(&mut host);
        Self::with_providers(vec![Arc::new(host)], cdn, uploads)
    }

    fn with_providers(
        providers: Vec<Arc<dyn CoverArtProvider>>,
        cdn: Arc<Cdn>,
        uploads: Arc<AtomicUsize>,
    ) -> Self {
        static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let cache_dir = std::env::temp_dir().join(format!(
            "mprisence-duplicate-upload-{}-{nanos}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, SeqCst)
        ));
        let cache = CoverCache::new_in(cache_dir.clone(), CoverCacheConfig::default()).unwrap();
        let manager = CoverManager {
            providers,
            cache: Arc::new(cache),
            config: Arc::new(ConfigManager::new_with_config(Config::default())),
            artwork_slots: Arc::new(Semaphore::new(1)),
            upload_slots: Arc::new(Semaphore::new(1)),
            failures: Mutex::default(),
            failure_backoff: super::FAILURE_BACKOFF,
            // The fake CDN answers at once; keep hanging cases short.
            timeouts: Timeouts {
                url_check: Duration::from_secs(1),
                upload: Duration::from_secs(2),
                fresh_upload_recheck: Duration::from_millis(400),
            },
        };
        Self {
            manager,
            cdn,
            uploads,
            cache_dir,
        }
    }

    fn uploads(&self) -> usize {
        self.uploads.load(SeqCst)
    }

    /// One track of the album starts playing (presence background fetch).
    async fn play(&self, cancel: &CancellationToken) -> Option<String> {
        self.play_cover(album_cover(), cancel).await
    }

    async fn play_cover(&self, cover: ArtSource, cancel: &CancellationToken) -> Option<String> {
        self.manager
            .get_cover_art(
                Some(cover),
                &Arc::new(MetadataSource::new(None, None)),
                true,
                cancel,
            )
            .await
            .unwrap()
    }

    /// What the presence fast path would push without any HTTP.
    fn fast_path(&self) -> Option<String> {
        self.manager.try_cached_cover_art(
            &MetadataSource::new(None, None),
            Some(&album_cover()),
            true,
        )
    }

    fn entry_files(&self) -> Vec<PathBuf> {
        std::fs::read_dir(&self.cache_dir)
            .unwrap()
            .flatten()
            .map(|file| file.path())
            .filter(|path| path.extension().is_none())
            .collect()
    }

    fn bin_files(&self) -> usize {
        std::fs::read_dir(&self.cache_dir)
            .unwrap()
            .flatten()
            .filter(|file| file.path().extension().is_some_and(|ext| ext == "bin"))
            .count()
    }

    fn entries_json(&self) -> Vec<serde_json::Value> {
        self.entry_files()
            .iter()
            .filter_map(|path| serde_json::from_slice(&std::fs::read(path).ok()?).ok())
            .collect()
    }

    /// Time travel: edit every cache entry on disk.
    fn age_entries(&self, edit: impl Fn(&mut CacheEntry)) {
        for path in self.entry_files() {
            let Ok(mut entry) =
                serde_json::from_slice::<CacheEntry>(&std::fs::read(&path).unwrap())
            else {
                continue;
            };
            edit(&mut entry);
            std::fs::write(&path, serde_json::to_vec(&entry).unwrap()).unwrap();
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&self.cache_dir, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::remove_dir_all(&self.cache_dir);
    }
}

fn album_cover() -> ArtSource {
    cover([180, 40, 90])
}

fn cover(rgb: [u8; 3]) -> ArtSource {
    let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb(rgb)));
    let mut jpeg = Vec::new();
    JpegEncoder::new_with_quality(&mut jpeg, 90)
        .encode_image(&image)
        .unwrap();
    ArtSource::Bytes(jpeg)
}

fn hours_ago(hours: u64) -> SystemTime {
    SystemTime::now() - Duration::from_secs(hours * 3600)
}

fn secs(time: SystemTime) -> serde_json::Value {
    let since = time.duration_since(UNIX_EPOCH).unwrap();
    serde_json::json!({ "secs_since_epoch": since.as_secs(), "nanos_since_epoch": 0 })
}

/// `ttl_hours` only schedules a recheck; a hosted file that is still alive is
/// reused instead of uploaded again.
#[tokio::test]
async fn cover_replayed_next_day_reuses_the_live_upload() {
    let h = Harness::new(Duration::ZERO);
    let day1 = h.play(&CancellationToken::new()).await;
    assert!(day1.is_some());
    assert_eq!(h.play(&CancellationToken::new()).await, day1);
    assert_eq!(h.uploads(), 1);

    // 25 hours later. The CDN still serves day1's URL.
    h.age_entries(|entry| {
        entry.expires_at = hours_ago(1);
        entry.last_validated = hours_ago(25);
    });
    assert_eq!(h.fast_path(), None, "stale entry must be rechecked first");
    let day2 = h.play(&CancellationToken::new()).await;

    assert_eq!(
        h.uploads(),
        1,
        "identical cover re-uploaded although {day1:?} is still alive (now {day2:?})"
    );
    assert_eq!(day2, day1);
    assert_eq!(h.fast_path(), day1, "renewed entry is served again");
}

/// The host deleted the file (ImgBB `expiration`, Litterbox): the URL must
/// not be served and the cover has to be uploaded again.
#[tokio::test]
async fn cover_is_uploaded_again_once_the_host_deletes_it() {
    let h = Harness::with_host(|host| host.lifetime = Some(Duration::from_secs(86400)));
    let day1 = h.play(&CancellationToken::new()).await;

    h.age_entries(|entry| {
        entry.expires_at = hours_ago(1);
        entry.hosted_expires_at = Some(hours_ago(1));
        entry.last_validated = hours_ago(25);
    });
    assert_eq!(h.fast_path(), None);
    let day2 = h.play(&CancellationToken::new()).await;

    assert_eq!(h.uploads(), 2);
    assert!(day2.is_some());
    assert_ne!(day2, day1);
}

/// A track change drops the presence task while the upload is in flight; the
/// host keeps the file, so its URL must still be cached.
#[tokio::test]
async fn skipping_a_track_mid_upload_still_caches_the_url() {
    let h = Harness::new(Duration::from_millis(300));
    let track1 = CancellationToken::new();

    // presence.rs: select! { get_cover_art(..), cancel_token.cancelled() }
    tokio::select! {
        _ = h.play(&track1) => panic!("upload should still be in flight"),
        _ = async {
            while h.uploads() == 0 {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            track1.cancel();
        } => {}
    }

    // Track 2 of the same album: same cover bytes.
    let track2 = h.play(&CancellationToken::new()).await;

    assert!(track2.is_some());
    assert_eq!(
        h.uploads(),
        1,
        "same cover uploaded again after a track skip"
    );
}

/// Two lookups for one cover queue on the upload slot; the second one reuses
/// the URL the first one stored.
#[tokio::test]
async fn concurrent_lookups_share_one_upload() {
    let h = Harness::new(Duration::from_millis(100));
    let (player_a, player_b) = (CancellationToken::new(), CancellationToken::new());
    let (first, second) = tokio::join!(h.play(&player_a), h.play(&player_b));

    assert!(first.is_some());
    assert_eq!(first, second);
    assert_eq!(
        h.uploads(),
        1,
        "two uploads for one cover: {first:?} and {second:?}"
    );
}

/// A network error during the hourly recheck says nothing about the file, so
/// the entry is kept.
#[tokio::test]
async fn network_blip_during_revalidation_keeps_the_cover() {
    let h = Harness::new(Duration::ZERO);
    let first = h.play(&CancellationToken::new()).await;

    // 2 hours later (revalidation due), right after resume: no network yet.
    h.age_entries(|entry| entry.last_validated = hours_ago(2));
    h.cdn.network_down.store(true, SeqCst);
    assert_eq!(h.play(&CancellationToken::new()).await, first);

    h.cdn.network_down.store(false, SeqCst);
    assert_eq!(h.play(&CancellationToken::new()).await, first);

    assert_eq!(
        h.uploads(),
        1,
        "cover re-uploaded after a transient network error"
    );
}

/// Once a hosted URL is gone the cover is uploaded again, and that replacement
/// is cached under the cover's key so the next play reuses it.
#[tokio::test]
async fn replacement_upload_is_reused() {
    let h = Harness::new(Duration::ZERO);
    let first = h.play(&CancellationToken::new()).await.unwrap();

    // 2 hours later the user deletes the image from their ImgBB account.
    h.age_entries(|entry| entry.last_validated = hours_ago(2));
    h.cdn.set(&first, Served::Missing);
    let replacement = h.play(&CancellationToken::new()).await;
    assert_eq!(h.uploads(), 2, "dead URL should be replaced once");
    assert!(replacement.is_some());
    assert_ne!(replacement.as_deref(), Some(first.as_str()));

    let next = h.play(&CancellationToken::new()).await;
    assert_eq!(
        h.uploads(),
        2,
        "replacement {replacement:?} was not reused (now {next:?})"
    );
}

/// Cache entries keep the uploaded length, not the image itself, so the
/// cache limits hold thousands of covers.
#[tokio::test]
async fn upload_entry_keeps_length_not_image_bytes() {
    let h = Harness::new(Duration::ZERO);
    let url = h.play(&CancellationToken::new()).await.unwrap();

    assert_eq!(h.bin_files(), 0, "no image bytes stored");
    let entries = h.entries_json();
    assert_eq!(entries.len(), 1);
    let len = entries[0]["content_len"].as_u64().expect("content_len");
    assert_eq!(
        h.cdn.served(&url.replace(&h.cdn.base, "")),
        Served::File(len as usize)
    );
}

/// Catbox answers a broken upload with a URL whose file is empty. That URL
/// must not be cached; the next configured host is used instead.
#[tokio::test]
async fn empty_upload_is_skipped_for_the_next_host() {
    let cdn = Cdn::spawn();
    let uploads = Arc::new(AtomicUsize::new(0));
    let mut broken = FakeHost::new(&cdn, &uploads);
    broken.name = "broken";
    *broken.stored.lock().unwrap() = Stored::Empty;
    let working = FakeHost::new(&cdn, &uploads);
    let h = Harness::with_providers(vec![Arc::new(broken), Arc::new(working)], cdn, uploads);

    let url = h.play(&CancellationToken::new()).await.unwrap();
    assert!(url.contains("/fakehost-"), "served {url}");
    assert_eq!(h.uploads(), 2);

    assert_eq!(
        h.play(&CancellationToken::new()).await.as_deref(),
        Some(url.as_str())
    );
    assert_eq!(h.uploads(), 2, "working URL reused");
}

/// The size recorded is what the host served right after the upload, not
/// what was sent: providers may alter the bytes they send (Catbox's retry).
#[tokio::test]
async fn recorded_size_is_what_the_host_served() {
    let h = Harness::with_host(|host| *host.stored.lock().unwrap() = Stored::Truncated);
    let url = h.play(&CancellationToken::new()).await.unwrap();
    let served = h.entries_json()[0]["content_len"].as_u64().unwrap() as usize;
    assert_eq!(
        h.cdn.served(&url.replace(&h.cdn.base, "")),
        Served::File(served)
    );
}

/// An empty HEAD is confirmed with a one-byte GET before the upload is
/// rejected, since Catbox's HEAD always says 0.
#[tokio::test]
async fn empty_upload_is_confirmed_with_a_one_byte_get() {
    let h = Harness::with_host(|host| *host.stored.lock().unwrap() = Stored::Empty);
    let started = Instant::now();
    assert_eq!(h.play(&CancellationToken::new()).await, None);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(
        h.cdn.gets(),
        2,
        "one probe, and one more after a short wait"
    );
}

/// A fresh upload can answer 404 for a moment while the host's CDN catches
/// up; it is probed again before being thrown away.
#[tokio::test]
async fn upload_that_404s_briefly_is_kept() {
    let h = Harness::with_host(|host| *host.stored.lock().unwrap() = Stored::LateVisible);
    let url = h
        .play(&CancellationToken::new())
        .await
        .expect("upload kept");
    assert_eq!(h.uploads(), 1);
    assert_eq!(h.fast_path().as_deref(), Some(url.as_str()), "verified");
}

/// A 416 only proves an empty file when it says `bytes */0`.
#[tokio::test]
async fn refused_range_without_a_size_keeps_the_entry() {
    let h = Harness::new(Duration::ZERO);
    h.cdn.head_reports_zero.store(true, SeqCst);
    let first = h.play(&CancellationToken::new()).await.unwrap();

    h.age_entries(|entry| entry.last_validated = hours_ago(2));
    h.cdn.set(&first, Served::RangeRefusedWithoutSize);
    assert_eq!(
        h.play(&CancellationToken::new()).await.as_deref(),
        Some(first.as_str())
    );
    assert_eq!(h.uploads(), 1);
}

/// A server that ignores the range is judged from the first chunk of its
/// body; the rest is never waited for.
#[tokio::test]
async fn ignored_range_reads_only_the_first_chunk() {
    let h = Harness::new(Duration::ZERO);
    let first = h.play(&CancellationToken::new()).await.unwrap();

    h.age_entries(|entry| entry.last_validated = hours_ago(2));
    h.cdn.set(&first, Served::ChunkedFile);
    let started = Instant::now();
    assert_eq!(
        h.play(&CancellationToken::new()).await.as_deref(),
        Some(first.as_str())
    );
    assert!(
        started.elapsed() < Duration::from_millis(900),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(h.uploads(), 1);
}

/// An ignored range with an empty body is an empty file.
#[tokio::test]
async fn ignored_range_with_an_empty_body_is_replaced() {
    let h = Harness::new(Duration::ZERO);
    let first = h.play(&CancellationToken::new()).await.unwrap();

    h.age_entries(|entry| entry.last_validated = hours_ago(2));
    h.cdn.set(&first, Served::ChunkedEmpty);
    let replacement = h.play(&CancellationToken::new()).await;
    assert!(replacement.is_some());
    assert_ne!(replacement.as_deref(), Some(first.as_str()));
    assert_eq!(h.uploads(), 2);
}

/// Catbox answers every HEAD with `Content-Length: 0`, even for files that
/// exist. The ranged GET tells the real size, so Catbox uploads are cached
/// and rechecked like any other.
#[tokio::test]
async fn catbox_head_is_confirmed_with_a_ranged_get() {
    let h = Harness::new(Duration::ZERO);
    h.cdn.head_reports_zero.store(true, SeqCst);
    let url = h
        .play(&CancellationToken::new())
        .await
        .expect("upload used");
    let recorded = h.entries_json()[0]["content_len"].as_u64().unwrap() as usize;
    assert_eq!(
        h.cdn.served(&url.replace(&h.cdn.base, "")),
        Served::File(recorded)
    );
    assert_eq!(h.fast_path().as_deref(), Some(url.as_str()), "verified");

    h.age_entries(|entry| {
        entry.expires_at = hours_ago(1);
        entry.last_validated = hours_ago(25);
    });
    assert_eq!(
        h.play(&CancellationToken::new()).await.as_deref(),
        Some(url.as_str())
    );
    assert_eq!(h.uploads(), 1);
}

/// A Catbox URL whose file no longer exists looks like a stalled host: its
/// GET never answers. The entry is kept and checked again later.
#[tokio::test]
async fn vanished_catbox_file_is_kept_until_it_can_be_checked() {
    let h = Harness::new(Duration::ZERO);
    let first = h.play(&CancellationToken::new()).await.unwrap();

    h.age_entries(|entry| {
        entry.expires_at = hours_ago(1);
        entry.last_validated = hours_ago(25);
    });
    h.cdn.set(&first, Served::Vanished);
    let started = Instant::now();
    assert_eq!(
        h.play(&CancellationToken::new()).await.as_deref(),
        Some(first.as_str())
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(h.uploads(), 1);
}

/// An empty file may refuse the range request with 416.
#[tokio::test]
async fn empty_file_refusing_ranges_is_replaced() {
    let h = Harness::new(Duration::ZERO);
    let first = h.play(&CancellationToken::new()).await.unwrap();

    h.age_entries(|entry| entry.last_validated = hours_ago(2));
    h.cdn.set(&first, Served::EmptyRefusingRanges);
    let replacement = h.play(&CancellationToken::new()).await;
    assert!(replacement.is_some());
    assert_ne!(replacement.as_deref(), Some(first.as_str()));
    assert_eq!(h.uploads(), 2);
}

/// A cached file that turns empty (Catbox) is uploaded again.
#[tokio::test]
async fn hosted_file_that_turns_empty_is_replaced() {
    let h = Harness::new(Duration::ZERO);
    let first = h.play(&CancellationToken::new()).await.unwrap();

    h.age_entries(|entry| entry.last_validated = hours_ago(2));
    h.cdn.set(&first, Served::Empty);
    let replacement = h.play(&CancellationToken::new()).await;

    assert_eq!(h.uploads(), 2);
    assert!(replacement.is_some());
    assert_ne!(replacement.as_deref(), Some(first.as_str()));
}

/// A size change is only logged: treating it as gone would upload the cover
/// again whenever a host re-optimises its files.
#[tokio::test]
async fn hosted_file_with_another_size_is_kept() {
    let h = Harness::new(Duration::ZERO);
    let first = h.play(&CancellationToken::new()).await.unwrap();
    let recorded = h.entries_json()[0]["content_len"].clone();

    h.age_entries(|entry| entry.last_validated = hours_ago(2));
    h.cdn.set(&first, Served::File(7));
    assert_eq!(
        h.play(&CancellationToken::new()).await.as_deref(),
        Some(first.as_str())
    );
    assert_eq!(h.uploads(), 1);
    assert_eq!(
        h.entries_json()[0]["content_len"],
        recorded,
        "recorded size kept"
    );
}

/// An upload that never answers must not hold the upload slot: it times out
/// and the next cover is uploaded.
#[tokio::test]
async fn hanging_upload_releases_the_upload_slot() {
    let h = Harness::new(Duration::from_secs(60));
    let started = Instant::now();
    assert_eq!(h.play(&CancellationToken::new()).await, None);
    assert_eq!(
        h.play_cover(cover([9, 9, 9]), &CancellationToken::new())
            .await,
        None
    );
    assert_eq!(h.uploads(), 2);
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
}

/// The host has the file even if its URL cannot be written to the cache, so
/// the URL is still used (the next presence push reuses it).
#[tokio::test]
async fn cache_write_failure_still_returns_the_url() {
    use std::os::unix::fs::PermissionsExt;
    let h = Harness::new(Duration::ZERO);
    std::fs::set_permissions(&h.cache_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

    let url = h.play(&CancellationToken::new()).await;
    assert!(url.is_some());
    assert!(h.entry_files().is_empty(), "nothing could be written");
    assert!(h.manager.failures.lock().unwrap().is_empty());
}

/// A cached upload whose host cannot be reached is served and not checked
/// again for an hour, instead of making every play wait for the check.
#[tokio::test]
async fn unreachable_recheck_is_postponed_and_served() {
    let h = Harness::new(Duration::ZERO);
    let first = h.play(&CancellationToken::new()).await.unwrap();

    h.age_entries(|entry| {
        entry.expires_at = hours_ago(1);
        entry.last_validated = hours_ago(25);
    });
    h.cdn.set(&first, Served::Hangs);
    assert_eq!(
        h.play(&CancellationToken::new()).await.as_deref(),
        Some(first.as_str())
    );
    assert_eq!(h.fast_path().as_deref(), Some(first.as_str()));
    let expires = h.entries_json()[0]["expires_at"]["secs_since_epoch"]
        .as_u64()
        .unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(
        (now + 3500..=now + 3700).contains(&expires),
        "rechecked in about an hour"
    );
    assert_eq!(h.uploads(), 1);
}

/// Some servers send `Content-Length: 0` on HEAD; for URLs that were not
/// uploaded that says nothing, so the entry is kept.
#[tokio::test]
async fn empty_head_is_ignored_for_entries_that_were_not_uploaded() {
    let h = Harness::new(Duration::ZERO);
    let url = format!("{}/front.jpg", h.cdn.base);
    h.cdn.set(&url, Served::Empty);
    let entry = serde_json::json!({
        "url": url,
        "provider": "musicbrainz",
        "expires_at": secs(hours_ago(1)),
        "last_validated": secs(hours_ago(25)),
    });
    std::fs::write(
        h.cache_dir.join("mb-key"),
        serde_json::to_vec(&entry).unwrap(),
    )
    .unwrap();

    assert!(h
        .manager
        .lookup_cached_cover("mb-key")
        .await
        .unwrap()
        .is_hit());
}

/// Litterbox entries written by 1.9.0 have no `hosted_expires_at`; once their
/// old expiry has passed the file is deleted, so the cover is uploaded again
/// instead of the dead URL being renewed.
#[tokio::test]
async fn stale_legacy_litterbox_entry_is_not_renewed() {
    let h = Harness::new(Duration::ZERO);
    let url = format!("{}/legacy.jpg", h.cdn.base);
    h.cdn.set(&url, Served::File(4096));
    let key = album_cover().cache_key().unwrap();
    let legacy = serde_json::json!({
        "url": url,
        "provider": "litterbox",
        "expires_at": secs(hours_ago(1)),
        "last_validated": secs(hours_ago(2)),
    });
    std::fs::write(h.cache_dir.join(&key), serde_json::to_vec(&legacy).unwrap()).unwrap();

    let replacement = h.play(&CancellationToken::new()).await;
    assert!(replacement.is_some());
    assert_ne!(replacement.as_deref(), Some(url.as_str()));
    assert_eq!(h.uploads(), 1);
}

/// A compressed response says nothing about the file's size.
#[tokio::test]
async fn compressed_response_skips_the_size_check() {
    let h = Harness::new(Duration::ZERO);
    let first = h.play(&CancellationToken::new()).await.unwrap();

    h.age_entries(|entry| entry.last_validated = hours_ago(2));
    h.cdn.set(&first, Served::Compressed(7));
    assert_eq!(
        h.play(&CancellationToken::new()).await.as_deref(),
        Some(first.as_str())
    );
    assert_eq!(h.uploads(), 1);
}

/// Entries written before the size was recorded are only checked for an
/// empty file.
#[tokio::test]
async fn entry_without_recorded_size_only_rejects_empty_files() {
    let h = Harness::new(Duration::ZERO);
    let first = h.play(&CancellationToken::new()).await.unwrap();
    for path in h.entry_files() {
        let mut json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        json.as_object_mut().unwrap().remove("content_len");
        json["last_validated"] = secs(hours_ago(2));
        std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    }

    h.cdn.set(&first, Served::File(7));
    assert_eq!(
        h.play(&CancellationToken::new()).await.as_deref(),
        Some(first.as_str())
    );
    assert_eq!(h.uploads(), 1);
}

/// A failing host is not hammered: repeated presence pushes for the same
/// cover upload at most once per backoff window.
#[tokio::test]
async fn failing_host_is_tried_once_per_backoff_window() {
    let h = Harness::with_host(|host| *host.stored.lock().unwrap() = Stored::Empty);
    for _ in 0..10 {
        assert_eq!(h.play(&CancellationToken::new()).await, None);
    }
    assert_eq!(h.uploads(), 1);
}

/// After the backoff the cover is tried again, and a success clears it.
#[tokio::test]
async fn failed_cover_is_retried_after_the_backoff() {
    let cdn = Cdn::spawn();
    let uploads = Arc::new(AtomicUsize::new(0));
    let host = Arc::new(FakeHost::new(&cdn, &uploads));
    *host.stored.lock().unwrap() = Stored::Empty;
    let mut h = Harness::with_providers(vec![host.clone()], cdn, uploads);
    h.manager.failure_backoff = Duration::from_millis(300);

    assert_eq!(h.play(&CancellationToken::new()).await, None);
    assert_eq!(h.play(&CancellationToken::new()).await, None);
    assert_eq!(h.uploads(), 1, "backed off");

    *host.stored.lock().unwrap() = Stored::Intact;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let url = h.play(&CancellationToken::new()).await;
    assert!(url.is_some());
    assert_eq!(h.uploads(), 2);
    assert!(
        h.manager.failures.lock().unwrap().is_empty(),
        "success clears"
    );
    assert_eq!(h.play(&CancellationToken::new()).await, url);
    assert_eq!(h.uploads(), 2);
}

/// A lookup cancelled by a track change is not a failure.
#[tokio::test]
async fn cancelled_lookup_is_not_remembered_as_a_failure() {
    let h = Harness::new(Duration::from_millis(200));
    let skipped = CancellationToken::new();
    skipped.cancel();
    assert_eq!(h.play(&skipped).await, None);
    assert!(h.manager.failures.lock().unwrap().is_empty());

    assert!(h.play(&CancellationToken::new()).await.is_some());
    assert_eq!(h.uploads(), 1);
}

/// mpv reports `mpris:artUrl` as the same `cover.jpg` that the local search
/// finds; when uploading it fails, the lookup does not upload it again.
#[tokio::test]
async fn same_cover_file_is_not_uploaded_twice_in_one_lookup() {
    let h = Harness::with_host(|host| *host.stored.lock().unwrap() = Stored::Empty);
    let album = h.cache_dir.with_extension("album");
    std::fs::create_dir_all(&album).unwrap();
    let ArtSource::Bytes(jpeg) = album_cover() else {
        unreachable!()
    };
    std::fs::write(album.join("cover.jpg"), jpeg).unwrap();
    let mut data = HashMap::new();
    data.insert(
        "xesam:url".to_string(),
        MetadataValue::String(format!("file://{}", album.join("01.mp3").display())),
    );
    let metadata = Arc::new(MetadataSource::new(Some(Metadata::from(data)), None));

    let found = h
        .manager
        .get_cover_art(
            Some(ArtSource::File(album.join("cover.jpg"))),
            &metadata,
            true,
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(&album);

    assert_eq!(found, None);
    assert_eq!(h.uploads(), 1);
}

/// A CDN that accepts the connection but never answers must not hold the
/// upload slot: the check times out and the URL is used, unverified.
#[tokio::test]
async fn hanging_cdn_does_not_block_the_upload_queue() {
    let h = Harness::with_host(|host| *host.stored.lock().unwrap() = Stored::Hangs);
    let started = Instant::now();
    let url = h.play(&CancellationToken::new()).await;
    assert!(url.is_some());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(h.fast_path(), None, "unverified");

    let next = h
        .play_cover(cover([1, 2, 3]), &CancellationToken::new())
        .await;
    assert!(next.is_some());
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
}

/// Sizes are compared only for uploads; the CDN behind a direct URL or a
/// MusicBrainz result may legitimately serve another size later.
#[tokio::test]
async fn size_is_compared_only_for_uploads() {
    let h = Harness::new(Duration::ZERO);
    let url = format!("{}/front.jpg", h.cdn.base);
    h.cdn.set(&url, Served::File(500));
    let write = |last_validated: SystemTime| {
        let entry = serde_json::json!({
            "url": url,
            "provider": "musicbrainz",
            "expires_at": secs(SystemTime::now() + Duration::from_secs(3600)),
            "last_validated": secs(last_validated),
        });
        std::fs::write(
            h.cache_dir.join("mb-key"),
            serde_json::to_vec(&entry).unwrap(),
        )
        .unwrap();
    };

    write(hours_ago(2));
    assert!(h
        .manager
        .lookup_cached_cover("mb-key")
        .await
        .unwrap()
        .is_hit());
    assert_eq!(h.entries_json()[0]["content_len"], serde_json::Value::Null);

    h.cdn.set(&url, Served::File(900));
    write(hours_ago(2));
    assert!(h
        .manager
        .lookup_cached_cover("mb-key")
        .await
        .unwrap()
        .is_hit());
}

/// An entry written by a newer mprisence (after a downgrade) is served, but
/// never rewritten: this build would drop the fields it does not know.
#[tokio::test]
async fn entry_from_a_newer_build_is_served_but_not_rewritten() {
    let h = Harness::new(Duration::ZERO);
    let url = format!("{}/future.jpg", h.cdn.base);
    h.cdn.set(&url, Served::File(100));
    let key = album_cover().cache_key().unwrap();
    let future = serde_json::json!({
        "url": url,
        "provider": "imgbb",
        "expires_at": secs(hours_ago(1)),
        "last_validated": secs(hours_ago(25)),
        "format": 99,
        "field_from_the_future": true,
    });
    let written = serde_json::to_vec(&future).unwrap();
    std::fs::write(h.cache_dir.join(&key), &written).unwrap();

    assert_eq!(
        h.play(&CancellationToken::new()).await.as_deref(),
        Some(url.as_str())
    );
    assert_eq!(h.uploads(), 0);
    assert_eq!(std::fs::read(h.cache_dir.join(&key)).unwrap(), written);
}

/// A cover that has no art anywhere is not looked up again on every push.
#[tokio::test]
async fn cover_without_art_is_not_looked_up_on_every_push() {
    let h = Harness::new(Duration::ZERO);
    let no_source = || async {
        h.manager
            .get_cover_art(
                None,
                &Arc::new(MetadataSource::new(None, None)),
                true,
                &CancellationToken::new(),
            )
            .await
            .unwrap()
    };
    assert_eq!(no_source().await, None);
    assert_eq!(h.manager.failures.lock().unwrap().len(), 1);
    assert_eq!(no_source().await, None);
}

/// If the URL cannot be checked right after upload, it is used but marked
/// stale, so the next lookup checks it before the fast path serves it again.
#[tokio::test]
async fn unverifiable_upload_is_used_and_rechecked() {
    let h = Harness::with_host(|host| *host.stored.lock().unwrap() = Stored::Unreachable);
    let url = h
        .play(&CancellationToken::new())
        .await
        .expect("URL is used");
    assert_eq!(
        h.fast_path(),
        None,
        "unverified entry is not served blindly"
    );

    assert_eq!(h.entries_json()[0]["content_len"], serde_json::Value::Null);

    h.cdn.set(&url, Served::File(1234));
    assert_eq!(
        h.play(&CancellationToken::new()).await.as_deref(),
        Some(url.as_str())
    );
    assert_eq!(h.fast_path().as_deref(), Some(url.as_str()));
    assert_eq!(h.entries_json()[0]["content_len"].as_u64(), Some(1234));
    assert_eq!(h.uploads(), 1);
}

fn write_legacy_entry(h: &Harness, url: &str, stale: bool) -> PathBuf {
    let key = album_cover().cache_key().unwrap();
    let bin = h.cache_dir.join(format!("{key}.bin"));
    std::fs::write(&bin, vec![1_u8; 4096]).unwrap();
    let expires_at = if stale {
        hours_ago(1)
    } else {
        SystemTime::now() + Duration::from_secs(3600)
    };
    let legacy = serde_json::json!({
        "url": url,
        "provider": "fakehost",
        "expires_at": secs(expires_at),
        "last_validated": secs(hours_ago(25)),
        "data_file": format!("{key}.bin"),
    });
    std::fs::write(h.cache_dir.join(&key), serde_json::to_vec(&legacy).unwrap()).unwrap();
    bin
}

/// An entry written by 1.9.0 or earlier carries the image in a `.bin` file.
/// Renewing it reuses the URL and deletes the leftover bytes.
#[tokio::test]
async fn renewing_a_legacy_entry_deletes_its_image_bytes() {
    let h = Harness::new(Duration::ZERO);
    let url = format!("{}/legacy.jpg", h.cdn.base);
    h.cdn.set(&url, Served::File(4096));
    let bin = write_legacy_entry(&h, &url, true);

    assert_eq!(
        h.play(&CancellationToken::new()).await.as_deref(),
        Some(url.as_str())
    );
    assert_eq!(h.uploads(), 0);
    assert!(!bin.exists(), "leftover .bin removed on renew");
    assert_eq!(h.entries_json()[0]["data_file"], serde_json::Value::Null);
}

/// A legacy entry whose URL is gone is dropped together with its bytes.
#[tokio::test]
async fn dead_legacy_entry_is_dropped_with_its_image_bytes() {
    let h = Harness::new(Duration::ZERO);
    let url = format!("{}/legacy.jpg", h.cdn.base);
    let bin = write_legacy_entry(&h, &url, true);

    let replacement = h.play(&CancellationToken::new()).await;
    assert!(replacement.is_some());
    assert_ne!(replacement.as_deref(), Some(url.as_str()));
    assert_eq!(h.uploads(), 1);
    assert!(!bin.exists());
    assert_eq!(h.bin_files(), 0);
}

/// Counts upload requests sent to a real provider.
struct Counting<P> {
    inner: P,
    uploads: Arc<AtomicUsize>,
}

#[async_trait]
impl<P: CoverArtProvider> CoverArtProvider for Counting<P> {
    fn name(&self) -> &'static str {
        self.inner.name()
    }

    fn supports_source_type(&self, source: &ArtSource) -> bool {
        self.inner.supports_source_type(source)
    }

    async fn process(
        &self,
        source: ArtSource,
        metadata: &MetadataSource,
        cancel: &CancellationToken,
    ) -> Result<Option<CoverResult>, CoverArtError> {
        self.process_borrowed(&source, metadata, cancel).await
    }

    async fn process_borrowed(
        &self,
        source: &ArtSource,
        metadata: &MetadataSource,
        cancel: &CancellationToken,
    ) -> Result<Option<CoverResult>, CoverArtError> {
        self.uploads.fetch_add(1, SeqCst);
        self.inner.process_borrowed(source, metadata, cancel).await
    }
}

fn live_harness(provider: impl CoverArtProvider + 'static) -> Harness {
    let uploads = Arc::new(AtomicUsize::new(0));
    let mut h = Harness::with_providers(
        vec![Arc::new(Counting {
            inner: provider,
            uploads: uploads.clone(),
        })],
        Cdn::spawn(),
        uploads,
    );
    h.manager.timeouts = Timeouts::default();
    h
}

/// Colors that stay the same for one run but differ between runs, so a
/// live run never reuses another run's upload.
fn palette() -> impl Fn(u32) -> [u8; 3] {
    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    move |i| {
        let [r, g, b, _] = seed.wrapping_add(i.wrapping_mul(0x9e37_79b9)).to_le_bytes();
        [r, g, b]
    }
}

/// A 600x600 PNG of noise, large enough that `normalize_uploads = false`
/// uploads a file well past what a JPEG cover would be.
fn noisy_png(seed: u32) -> ArtSource {
    let mut state = seed | 1;
    let image = RgbImage::from_fn(600, 600, |_, _| {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        let [r, g, b, _] = state.to_le_bytes();
        Rgb([r, g, b])
    });
    let mut png = Vec::new();
    DynamicImage::ImageRgb8(image)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    ArtSource::Bytes(png)
}

/// Shared live scenarios: verified upload, next-day recheck, a skip while
/// the upload is in flight, and two concurrent plays.
async fn live_scenarios(h: &Harness) {
    let random_color = palette();
    let first = h
        .play_cover(cover(random_color(0)), &CancellationToken::new())
        .await
        .expect("hosted URL");
    println!("uploaded: {first}");
    let entries = h.entries_json();
    println!(
        "entry: content_len={} hosted_expires_at={}",
        entries[0]["content_len"], entries[0]["hosted_expires_at"]
    );
    assert!(entries[0]["content_len"].as_u64().is_some());
    assert_eq!(h.bin_files(), 0);
    assert_eq!(h.fast_path(), None, "different cover");

    // Next day: the recheck hits the real CDN, compares the size, reuses the URL.
    h.age_entries(|entry| {
        entry.expires_at = hours_ago(1);
        entry.last_validated = hours_ago(25);
    });
    let next_day = h
        .play_cover(cover(random_color(0)), &CancellationToken::new())
        .await;
    println!("next day: {next_day:?}");
    assert_eq!(next_day.as_deref(), Some(first.as_str()));
    assert_eq!(h.uploads.load(SeqCst), 1);

    // Skip while the upload is in flight, then the next track with that cover.
    let skipped = CancellationToken::new();
    tokio::select! {
        _ = h.play_cover(cover(random_color(1)), &skipped) => panic!("upload should still be in flight"),
        _ = async {
            while h.uploads.load(SeqCst) < 2 {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
            skipped.cancel();
        } => {}
    }
    let after_skip = h
        .play_cover(cover(random_color(1)), &CancellationToken::new())
        .await;
    println!("after skip: {after_skip:?}");
    assert!(after_skip.is_some());
    assert_eq!(h.uploads.load(SeqCst), 2);

    // Two players start the same cover at once.
    let (player_a, player_b) = (CancellationToken::new(), CancellationToken::new());
    let (a, b) = tokio::join!(
        h.play_cover(cover(random_color(2)), &player_a),
        h.play_cover(cover(random_color(2)), &player_b)
    );
    println!("concurrent: {a:?} / {b:?}");
    assert!(a.is_some());
    assert_eq!(a, b);
    assert_eq!(h.uploads.load(SeqCst), 3);
}

/// Live run against ImgBB, counting upload requests (ImgBB answers a repeat
/// upload within its duplicate window with the same URL, so the URL alone
/// proves nothing). Uploads small random covers that expire after 10 minutes:
/// `MPRISENCE_IMGBB_API_KEY=... cargo test --lib live_imgbb -- --ignored --nocapture`
#[tokio::test]
#[ignore = "uploads to ImgBB; set MPRISENCE_IMGBB_API_KEY"]
async fn live_imgbb_uploads_each_cover_once() {
    let api_key = std::env::var("MPRISENCE_IMGBB_API_KEY").expect("MPRISENCE_IMGBB_API_KEY");
    let h = live_harness(ImgbbProvider::with_config(ImgBBConfig {
        api_key: Some(api_key),
        expiration: 600,
    }));
    live_scenarios(&h).await;
}

/// `normalize_uploads = false`: a large PNG goes up unchanged and its size
/// must still verify.
#[tokio::test]
#[ignore = "uploads to ImgBB; set MPRISENCE_IMGBB_API_KEY"]
async fn live_imgbb_raw_png_verifies() {
    let api_key = std::env::var("MPRISENCE_IMGBB_API_KEY").expect("MPRISENCE_IMGBB_API_KEY");
    let mut h = live_harness(ImgbbProvider::with_config(ImgBBConfig {
        api_key: Some(api_key),
        expiration: 600,
    }));
    let mut config = Config::default();
    config.cover.normalize_uploads = false;
    h.manager.config = Arc::new(ConfigManager::new_with_config(config));

    let png = noisy_png(palette()(9)[0] as u32 + 1);
    let ArtSource::Bytes(bytes) = &png else {
        unreachable!()
    };
    println!("raw PNG: {} bytes", bytes.len());
    let url = h
        .play_cover(png.clone(), &CancellationToken::new())
        .await
        .expect("hosted URL");
    println!("uploaded: {url}");
    let entries = h.entries_json();
    println!(
        "ImgBB serves {} of {} uploaded bytes",
        entries[0]["content_len"],
        bytes.len()
    );
    assert!(entries[0]["content_len"].as_u64().is_some());
    assert_eq!(
        h.play_cover(png, &CancellationToken::new())
            .await
            .as_deref(),
        Some(url.as_str())
    );
    assert_eq!(h.uploads.load(SeqCst), 1);
}

/// Live run through the Catbox provider in Litterbox mode (public files that
/// expire after one hour):
/// `MPRISENCE_LIVE_LITTERBOX=1 cargo test --lib live_litterbox -- --ignored --nocapture`
#[tokio::test]
#[ignore = "uploads to Litterbox; set MPRISENCE_LIVE_LITTERBOX=1"]
async fn live_litterbox_uploads_each_cover_once() {
    assert!(std::env::var("MPRISENCE_LIVE_LITTERBOX").is_ok());
    let h = live_harness(CatboxProvider::with_config(CatboxConfig {
        use_litter: true,
        litter_hours: 1,
        ..CatboxConfig::default()
    }));
    live_scenarios(&h).await;
}

/// Live run through the Catbox provider with permanent uploads. They are tied
/// to your account so you can delete them afterwards (the run prints their
/// URLs):
/// `MPRISENCE_CATBOX_USER_HASH=... cargo test --lib live_catbox -- --ignored --nocapture`
#[tokio::test]
#[ignore = "uploads to Catbox; set MPRISENCE_CATBOX_USER_HASH"]
async fn live_catbox_uploads_each_cover_once() {
    let user_hash =
        std::env::var("MPRISENCE_CATBOX_USER_HASH").expect("MPRISENCE_CATBOX_USER_HASH");
    let h = live_harness(CatboxProvider::with_config(CatboxConfig {
        user_hash: Some(user_hash),
        use_litter: false,
        ..CatboxConfig::default()
    }));
    live_scenarios(&h).await;
}

/// What the fake Catbox endpoint does with one upload.
#[derive(Debug, Clone, Copy)]
enum CatboxReply {
    Intact,
    Empty,
    Hangs,
    LateVisible,
}

/// Drives `CatboxProvider::upload_checked` with a fake upload endpoint that
/// publishes each upload on the fake CDN, whose HEAD says 0 like Catbox's.
struct FakeCatbox {
    cdn: Arc<Cdn>,
    provider: CatboxProvider,
    replies: Vec<CatboxReply>,
    sent: Mutex<Vec<Vec<u8>>>,
}

impl FakeCatbox {
    fn new(replies: &[CatboxReply]) -> Self {
        let cdn = Cdn::spawn();
        cdn.head_reports_zero.store(true, SeqCst);
        let mut provider = CatboxProvider::with_config(CatboxConfig {
            use_litter: false,
            ..CatboxConfig::default()
        });
        provider.timeouts = Timeouts {
            url_check: Duration::from_secs(1),
            upload: Duration::from_secs(2),
            fresh_upload_recheck: Duration::from_millis(400),
        };
        Self {
            cdn,
            provider,
            replies: replies.to_vec(),
            sent: Mutex::default(),
        }
    }

    async fn upload(&self, data: &[u8]) -> Result<String, CoverArtError> {
        self.provider
            .upload_checked(data, |bytes: Vec<u8>| {
                let mut sent = self.sent.lock().unwrap();
                sent.push(bytes.clone());
                let n = sent.len();
                let url = format!("{}/catbox-{n}.jpg", self.cdn.base);
                let served = match self
                    .replies
                    .get(n - 1)
                    .copied()
                    .unwrap_or(CatboxReply::Intact)
                {
                    CatboxReply::Intact => Served::File(bytes.len()),
                    CatboxReply::Empty => Served::Empty,
                    CatboxReply::Hangs => Served::Hangs,
                    CatboxReply::LateVisible => {
                        let (cdn, url, len) = (Arc::clone(&self.cdn), url.clone(), bytes.len());
                        std::thread::spawn(move || {
                            std::thread::sleep(Duration::from_millis(150));
                            cdn.set(&url, Served::File(len));
                        });
                        Served::Missing
                    }
                };
                self.cdn.set(&url, served);
                async move { Ok(url) }
            })
            .await
    }

    fn uploads(&self) -> usize {
        self.sent.lock().unwrap().len()
    }
}

fn jpeg_bytes() -> Vec<u8> {
    let ArtSource::Bytes(bytes) = cover([12, 34, 56]) else {
        unreachable!()
    };
    bytes
}

/// A good Catbox upload is checked once and used.
#[tokio::test]
async fn catbox_good_upload_is_used() {
    let catbox = FakeCatbox::new(&[CatboxReply::Intact]);
    let url = catbox.upload(&jpeg_bytes()).await.unwrap();
    assert!(url.ends_with("/catbox-1.jpg"), "{url}");
    assert_eq!(catbox.uploads(), 1);
}

/// Catbox handing back an empty file is fixed by one upload of altered bytes;
/// the image itself is unchanged.
#[tokio::test]
async fn catbox_empty_upload_is_retried_with_altered_bytes() {
    let catbox = FakeCatbox::new(&[CatboxReply::Empty, CatboxReply::Intact]);
    let url = catbox.upload(&jpeg_bytes()).await.unwrap();
    assert!(url.ends_with("/catbox-2.jpg"), "{url}");
    assert_eq!(catbox.uploads(), 2);

    let sent = catbox.sent.lock().unwrap();
    assert_ne!(sent[0], sent[1]);
    assert_eq!(
        image::load_from_memory(&sent[1]).unwrap().into_rgb8(),
        image::load_from_memory(&sent[0]).unwrap().into_rgb8()
    );
}

/// A check that cannot reach Catbox says nothing about the file; the URL is
/// used unverified instead of uploading the cover again.
#[tokio::test]
async fn catbox_unreachable_check_does_not_upload_again() {
    let catbox = FakeCatbox::new(&[CatboxReply::Hangs]);
    let url = catbox.upload(&jpeg_bytes()).await.unwrap();
    assert!(url.ends_with("/catbox-1.jpg"), "{url}");
    assert_eq!(catbox.uploads(), 1);
}

/// A fresh upload that answers 404 for a moment is probed again, not
/// replaced.
#[tokio::test]
async fn catbox_briefly_missing_upload_is_not_retried() {
    let catbox = FakeCatbox::new(&[CatboxReply::LateVisible]);
    let url = catbox.upload(&jpeg_bytes()).await.unwrap();
    assert!(url.ends_with("/catbox-1.jpg"), "{url}");
    assert_eq!(catbox.uploads(), 1);
}

/// Two empty files in a row is an error; there is no third upload.
#[tokio::test]
async fn catbox_empty_twice_is_an_error() {
    let catbox = FakeCatbox::new(&[CatboxReply::Empty, CatboxReply::Empty]);
    assert!(catbox.upload(&jpeg_bytes()).await.is_err());
    assert_eq!(catbox.uploads(), 2);
}

/// When the retried upload cannot be checked, its URL is used unverified;
/// there is no third upload.
#[tokio::test]
async fn catbox_retry_with_unreachable_check_uses_that_url() {
    let catbox = FakeCatbox::new(&[CatboxReply::Empty, CatboxReply::Hangs]);
    let url = catbox.upload(&jpeg_bytes()).await.unwrap();
    assert!(url.ends_with("/catbox-2.jpg"), "{url}");
    assert_eq!(catbox.uploads(), 2);
}

/// Only JPEG bytes can be altered without changing the image, so other
/// formats (`normalize_uploads = false`) are not retried.
#[tokio::test]
async fn catbox_non_jpeg_upload_is_not_retried() {
    let catbox = FakeCatbox::new(&[CatboxReply::Empty]);
    let ArtSource::Bytes(png) = noisy_png(7) else {
        unreachable!()
    };
    assert!(catbox.upload(&png).await.is_err());
    assert_eq!(catbox.uploads(), 1);
}
