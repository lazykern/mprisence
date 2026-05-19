use std::time::Duration;

use blake3::Hasher;
use crate::cover::sources::ArtSource;
use crate::utils::{
    format_audio_channels, format_bit_depth, format_bitrate, format_duration, format_sample_rate,
    format_track_number,
};
use lofty::{
    file::{AudioFile, TaggedFile, TaggedFileExt},
    prelude::*,
    properties::FileProperties,
};
use log::{trace, warn};
use mpris::Metadata;
use serde::Serialize;
use url::Url;

macro_rules! impl_metadata_getter {
    // String getter with both MPRIS and Lofty
    ($name:ident, $mpris_key:expr, $lofty_key:expr) => {
        pub fn $name(&self) -> Option<String> {
            trace!(concat!(
                "Getting ",
                stringify!($name),
                " from metadata sources"
            ));
            self.mpris_metadata
                .as_ref()
                .and_then(|m| m.get($mpris_key).and_then(|v| v.as_str()).map(String::from))
                .or_else(|| {
                    self.tagged_file
                        .as_ref()
                        .and_then(|t| t.primary_tag())
                        .and_then(|tag| tag.get_string($lofty_key))
                        .map(String::from)
                })
        }
    };
    // u32 getter with parsing for both MPRIS and Lofty
    ($name:ident, $mpris_key:expr, $lofty_key:expr, parse_u32) => {
        pub fn $name(&self) -> Option<u32> {
            trace!(concat!(
                "Getting ",
                stringify!($name),
                " from metadata sources"
            ));
            self.mpris_metadata
                .as_ref()
                .and_then(|m| {
                    m.get($mpris_key)
                        .and_then(|v| v.as_str())
                        .and_then(|s| s.parse().ok())
                })
                .or_else(|| {
                    self.tagged_file
                        .as_ref()
                        .and_then(|t| t.primary_tag())
                        .and_then(|tag| tag.get_string($lofty_key))
                        .and_then(|s| s.parse().ok())
                })
        }
    };
    // Array getter for both MPRIS and Lofty
    ($name:ident, $mpris_key:expr, $lofty_key:expr, array) => {
        pub fn $name(&self) -> Option<Vec<String>> {
            trace!(concat!(
                "Getting ",
                stringify!($name),
                " from metadata sources"
            ));
            self.mpris_metadata
                .as_ref()
                .and_then(|m| m.get($mpris_key))
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|g| g.as_str())
                        .map(String::from)
                        .collect()
                })
                .or_else(|| {
                    self.tagged_file
                        .as_ref()
                        .and_then(|t| t.primary_tag())
                        .and_then(|tag| tag.get_string($lofty_key))
                        .map(|s| vec![s.to_string()])
                })
        }
    };
    // MPRIS-only string getter
    ($name:ident, $mpris_key:expr) => {
        pub fn $name(&self) -> Option<String> {
            trace!(concat!(
                "Getting ",
                stringify!($name),
                " from MPRIS metadata"
            ));
            self.mpris_metadata
                .as_ref()
                .and_then(|m| m.get($mpris_key))
                .and_then(|v| v.as_str())
                .map(String::from)
        }
    };
    // MPRIS-only u32 getter
    ($name:ident, $mpris_key:expr, _) => {
        pub fn $name(&self) -> Option<u32> {
            trace!(concat!(
                "Getting ",
                stringify!($name),
                " from MPRIS metadata"
            ));
            self.mpris_metadata
                .as_ref()
                .and_then(|m| m.get($mpris_key))
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse().ok())
        }
    };
}

/// A template-friendly representation of metadata with non-optional fields and sensible defaults.
/// This struct is designed to be easily used with handlebars templates.
#[derive(Debug, Clone, Serialize, Default)]
pub struct MediaMetadata {
    pub title: Option<String>,
    pub artists: Vec<String>, // Keep as Vec since empty vec is semantically correct
    pub artist_display: Option<String>, // Comma-separated artists for easy template use
    pub album: Option<String>,
    pub album_artists: Vec<String>, // Keep as Vec since empty vec is semantically correct
    pub album_artist_display: Option<String>, // Comma-separated album artists
    pub track_number: Option<u32>,  // Raw track number (e.g., 1)
    pub track_total: Option<u32>,   // Total tracks (e.g., 12)
    pub track_display: Option<String>, // "1/12" format
    pub disc_number: Option<u32>,   // Raw disc number (e.g., 1)
    pub disc_total: Option<u32>,    // Total discs (e.g., 3)
    pub disc_display: Option<String>, // "1/3" format
    pub genres: Vec<String>,        // Keep as Vec since empty vec is semantically correct
    pub genre_display: Option<String>, // Comma-separated genres for easy template use
    pub year: Option<String>,

    pub duration_secs: Option<u64>,       // Raw duration in seconds
    pub duration_display: Option<String>, // Formatted as "mm:ss"
    pub initial_key: Option<String>,
    pub bpm: Option<String>,
    pub mood: Option<String>,

    pub bitrate_display: Option<String>,     // "320 kbps"
    pub sample_rate_display: Option<String>, // "44.1 kHz"
    pub bit_depth_display: Option<String>,   // "16-bit"
    pub channels_display: Option<String>,    // "Stereo" or "5.1" etc.

    pub isrc: Option<String>,
    pub barcode: Option<String>,
    pub catalog_number: Option<String>,
    pub label: Option<String>,

    pub musicbrainz_track_id: Option<String>,
    pub musicbrainz_album_id: Option<String>,
    pub musicbrainz_artist_id: Option<String>,
    pub musicbrainz_album_artist_id: Option<String>,
    pub musicbrainz_release_group_id: Option<String>,

    pub composer: Option<String>,
    pub lyricist: Option<String>,
    pub conductor: Option<String>,
    pub remixer: Option<String>,
    pub language: Option<String>,
    pub encoded_by: Option<String>,
    pub encoder_settings: Option<String>,
    pub copyright: Option<String>,
    pub publisher: Option<String>,
    pub url: Option<String>,
    pub comment: Option<String>,
    pub content_created: Option<String>,
    pub last_used: Option<String>,
    pub use_count: Option<u32>,
    // Classical music specific
    pub movement: Option<String>,
    pub movement_number: Option<u32>,
    pub movement_total: Option<u32>,
    pub movement_display: Option<String>, // "1/3" format like track_display
}

pub struct MetadataSource {
    mpris_metadata: Option<Metadata>,
    tagged_file: Option<TaggedFile>,
    override_url: Option<String>,
    /// Memoized cover-cache key. Computed once via `generate_cache_key()`
    /// and reused across fast-path and slow-path lookups on the same track.
    cache_key: std::sync::OnceLock<String>,
}

impl MetadataSource {
    pub fn new(mpris_metadata: Option<Metadata>, lofty_tagged_file: Option<TaggedFile>) -> Self {
        Self {
            mpris_metadata,
            tagged_file: lofty_tagged_file,
            override_url: None,
            cache_key: std::sync::OnceLock::new(),
        }
    }

    pub fn from_mpris_with_override(metadata: Metadata, override_url: Option<String>) -> Self {
        let override_tagged_file = override_url
            .as_ref()
            .and_then(|url| Self::lofty_tag_from_url(url).ok());
        let tagged_file = metadata
            .url()
            .and_then(|url| Self::lofty_tag_from_url(url).ok());
        let tagged_file = override_tagged_file.or(tagged_file);
        let mut source = Self::new(Some(metadata), tagged_file);
        source.override_url = override_url;
        source
    }

    fn lofty_tag_from_url<S: AsRef<str>>(url: S) -> Result<TaggedFile, String> {
        let url = Url::parse(url.as_ref()).map_err(|e| e.to_string())?;
        if url.scheme() == "file" {
            let encoded_path = url.path();
            match urlencoding::decode(encoded_path) {
                Ok(decoded_cow) => {
                    let decoded_path = decoded_cow.into_owned();
                    lofty::read_from_path(&decoded_path).map_err(|e| e.to_string())
                }
                Err(e) => {
                    warn!(
                        "Failed to URL-decode path '{}': {}. Lofty might fail.",
                        encoded_path, e
                    );
                    lofty::read_from_path(encoded_path).map_err(|e| e.to_string())
                }
            }
        } else {
            Err(format!("Unsupported URL scheme: {}", url.scheme()))
        }
    }

    impl_metadata_getter!(title, "xesam:title", ItemKey::TrackTitle);
    impl_metadata_getter!(album, "xesam:album", ItemKey::AlbumTitle);
    impl_metadata_getter!(initial_key, "xesam:initialKey", ItemKey::InitialKey);
    impl_metadata_getter!(bpm, "xesam:bpm", ItemKey::Bpm);
    impl_metadata_getter!(mood, "xesam:mood", ItemKey::Mood);

    impl_metadata_getter!(isrc, "xesam:isrc", ItemKey::Isrc);
    impl_metadata_getter!(barcode, "xesam:barcode", ItemKey::Barcode);
    impl_metadata_getter!(
        catalog_number,
        "xesam:catalogNumber",
        ItemKey::CatalogNumber
    );
    impl_metadata_getter!(label, "xesam:label", ItemKey::Label);

    impl_metadata_getter!(
        musicbrainz_track_id,
        "xesam:musicbrainzTrackID",
        ItemKey::MusicBrainzTrackId
    );
    impl_metadata_getter!(
        musicbrainz_album_id,
        "xesam:musicbrainzAlbumID",
        ItemKey::MusicBrainzReleaseId
    );
    impl_metadata_getter!(
        musicbrainz_artist_id,
        "xesam:musicbrainzArtistID",
        ItemKey::MusicBrainzArtistId
    );
    impl_metadata_getter!(
        musicbrainz_album_artist_id,
        "xesam:musicbrainzAlbumArtistID",
        ItemKey::MusicBrainzReleaseArtistId
    );
    impl_metadata_getter!(
        musicbrainz_release_group_id,
        "xesam:musicbrainzReleaseGroupID",
        ItemKey::MusicBrainzReleaseGroupId
    );

    impl_metadata_getter!(
        track_number,
        "xesam:trackNumber",
        ItemKey::TrackNumber,
        parse_u32
    );
    impl_metadata_getter!(
        track_total,
        "xesam:trackTotal",
        ItemKey::TrackTotal,
        parse_u32
    );
    impl_metadata_getter!(
        disc_number,
        "xesam:discNumber",
        ItemKey::DiscNumber,
        parse_u32
    );
    impl_metadata_getter!(disc_total, "xesam:discTotal", ItemKey::DiscTotal, parse_u32);
    impl_metadata_getter!(year, "xesam:year", ItemKey::Year, parse_u32);

    impl_metadata_getter!(composer, "xesam:composer", ItemKey::Composer);
    impl_metadata_getter!(lyricist, "xesam:lyricist", ItemKey::Lyricist);
    impl_metadata_getter!(conductor, "xesam:conductor", ItemKey::Conductor);
    impl_metadata_getter!(remixer, "xesam:remixer", ItemKey::Remixer);
    impl_metadata_getter!(language, "xesam:language", ItemKey::Language);
    impl_metadata_getter!(encoded_by, "xesam:encodedBy", ItemKey::EncodedBy);
    impl_metadata_getter!(
        encoder_settings,
        "xesam:encoderSettings",
        ItemKey::EncoderSettings
    );
    impl_metadata_getter!(comment, "xesam:comment", ItemKey::Comment);

    impl_metadata_getter!(genres, "xesam:genre", ItemKey::Genre, array);
    impl_metadata_getter!(copyright, "xesam:copyright");
    impl_metadata_getter!(publisher, "xesam:publisher");
    impl_metadata_getter!(movement, "xesam:movement");
    impl_metadata_getter!(movement_number, "xesam:movementNumber", _);
    impl_metadata_getter!(movement_total, "xesam:movementTotal", _);
    impl_metadata_getter!(use_count, "xesam:useCount", _);

    pub fn artists(&self) -> Option<Vec<String>> {
        trace!("Getting artists from metadata sources");
        self.mpris_metadata
            .as_ref()
            .and_then(|m| m.artists())
            .map(|artists| artists.iter().map(|s| s.to_string()).collect())
            .or_else(|| {
                self.tagged_file
                    .as_ref()
                    .and_then(|t| t.primary_tag())
                    .and_then(|tag| tag.get_string(ItemKey::TrackArtist))
                    .map(|artist| vec![artist.to_string()])
            })
    }

    pub fn album_artists(&self) -> Option<Vec<String>> {
        trace!("Getting album artists from metadata sources");
        self.mpris_metadata
            .as_ref()
            .and_then(|m| m.album_artists())
            .map(|artists| artists.iter().map(|s| s.to_string()).collect())
            .or_else(|| {
                self.tagged_file
                    .as_ref()
                    .and_then(|t| t.primary_tag())
                    .and_then(|tag| tag.get_string(ItemKey::AlbumArtist))
                    .map(|artist| vec![artist.to_string()])
            })
    }

    pub fn length(&self) -> Option<Duration> {
        trace!("Getting track length from metadata sources");
        self.mpris_metadata
            .as_ref()
            .and_then(|m| m.length())
            .or_else(|| self.tagged_file.as_ref().map(|t| t.properties().duration()))
    }

    pub fn audio_properties(&self) -> Option<&FileProperties> {
        self.tagged_file.as_ref().map(|t| t.properties())
    }

    pub fn art_source_with_options(&self, options: ArtSourceOptions) -> Option<ArtSource> {
        trace!("Getting art source from metadata");

        let art_url = options
            .allow_mpris_art_url
            .then(|| self.mpris_metadata.as_ref().and_then(|m| m.art_url()))
            .flatten();
        let inferred = options
            .allow_inferred_url
            .then(|| self.url().as_deref().and_then(infer_art_url_from_url))
            .flatten();
        let embedded = self
            .tagged_file
            .as_ref()
            .and_then(|t| t.primary_tag())
            .and_then(|tag| tag.pictures().first())
            .map(|picture| picture.data().to_vec());

        select_art_source(art_url, inferred, embedded)
    }

    pub fn mpris_metadata(&self) -> Option<&Metadata> {
        self.mpris_metadata.as_ref()
    }

    pub fn lofty_tag(&self) -> Option<&TaggedFile> {
        self.tagged_file.as_ref()
    }

    pub fn url(&self) -> Option<String> {
        self.override_url
            .as_ref()
            .filter(|url| !url.is_empty())
            .cloned()
            .or_else(|| {
                self.mpris_metadata
                    .as_ref()
                    .and_then(|m| m.url())
                    .map(String::from)
            })
    }

    pub fn track_id(&self) -> Option<String> {
        self.mpris_metadata
            .as_ref()
            .and_then(|m| m.get("mpris:trackid"))
            .and_then(|v| v.as_str())
            .map(String::from)
    }

    /// Returns the memoized cover-cache key for this track.
    /// On first call, generates the key via BLAKE3 hashing of sorted
    /// metadata fields; subsequent calls return the cached string.
    /// This avoids redundant hashing when both the fast-path and
    /// background cover-fetch paths need the same key.
    pub fn cache_key(&self) -> &str {
        self.cache_key
            .get_or_init(|| Self::generate_cache_key(self))
    }

    fn generate_cache_key(&self) -> String {
        let mut hasher = Hasher::new();
        let mut key_components = Vec::new();

        if let Some(title) = self.title() {
            if !title.is_empty() {
                key_components.push(format!("title:{}", title));
            }
        }
        if let Some(mut artists) = self.artists() {
            if !artists.is_empty() {
                artists.sort_unstable();
                key_components.push(format!("artists:{}", artists.join("|")));
            }
        }
        if let Some(album) = self.album() {
            if !album.is_empty() {
                key_components.push(format!("album:{}", album));
                if let Some(mut album_artists) = self.album_artists() {
                    if !album_artists.is_empty()
                        && Some(&album_artists) != self.artists().as_ref()
                    {
                        album_artists.sort_unstable();
                        key_components.push(format!(
                            "album_artists:{}",
                            album_artists.join("|")
                        ));
                    }
                }
            }
        }
        if let Some(url) = self.url() {
            if !url.is_empty() {
                key_components.push(format!("url:{}", url));
            }
        }
        if let Some(track_id) = self.track_id() {
            if !track_id.is_empty() {
                key_components.push(format!("track_id:{}", track_id));
            }
        }
        if let Some(art_url) = self
            .mpris_metadata()
            .and_then(|m| m.art_url())
            .filter(|s| !s.is_empty())
        {
            key_components.push(format!("art_url:{}", art_url));
        }
        if key_components.is_empty() {
            key_components.push("default_mprisence_key".to_string());
        }
        let combined = key_components.join("||");
        hasher.update(combined.as_bytes());
        hasher.finalize().to_hex().to_string()
    }

    pub fn content_created(&self) -> Option<String> {
        self.mpris_metadata
            .as_ref()
            .and_then(|m| m.get("xesam:contentCreated"))
            .and_then(|v| v.as_str())
            .map(String::from)
    }

    pub fn last_used(&self) -> Option<String> {
        self.mpris_metadata
            .as_ref()
            .and_then(|m| m.get("xesam:lastUsed"))
            .and_then(|v| v.as_str())
            .map(String::from)
    }

    pub fn to_media_metadata(&self) -> MediaMetadata {
        let mut metadata = MediaMetadata {
            title: self.title(),
            ..Default::default()
        };

        if let Some(artists) = self.artists() {
            let artists: Vec<String> = artists.into_iter().filter(|s| !s.is_empty()).collect();
            if !artists.is_empty() {
                metadata.artist_display = Some(artists.join(", "));
                metadata.artists = artists;
            }
        }

        metadata.album = self.album();

        if let Some(album_artists) = self.album_artists() {
            let album_artists: Vec<String> =
                album_artists.into_iter().filter(|s| !s.is_empty()).collect();
            if !album_artists.is_empty() {
                metadata.album_artist_display = Some(album_artists.join(", "));
                metadata.album_artists = album_artists;
            }
        }

        metadata.track_number = self.track_number();
        metadata.track_total = self.track_total();
        if let Some(track_num) = metadata.track_number {
            metadata.track_display = Some(format_track_number(track_num, metadata.track_total));
        }

        metadata.disc_number = self.disc_number();
        metadata.disc_total = self.disc_total();
        if let Some(disc_num) = metadata.disc_number {
            metadata.disc_display = Some(format_track_number(disc_num, metadata.disc_total));
        }

        metadata.genres = self.genres().unwrap_or_default();
        metadata.genre_display = Some(metadata.genres.join(", "));

        metadata.year = self.year().map(|y| y.to_string());

        if let Some(duration) = self.length() {
            metadata.duration_secs = Some(duration.as_secs());
            metadata.duration_display = Some(format_duration(duration.as_secs()));
        }

        metadata.initial_key = self.initial_key();
        metadata.bpm = self.bpm();
        metadata.mood = self.mood();

        if let Some(props) = self.audio_properties() {
            if let Some(bitrate) = props.overall_bitrate() {
                metadata.bitrate_display = Some(format_bitrate(bitrate));
            }
            if let Some(rate) = props.sample_rate() {
                metadata.sample_rate_display = Some(format_sample_rate(rate));
            }
            if let Some(depth) = props.bit_depth() {
                metadata.bit_depth_display = Some(format_bit_depth(depth));
            }
            if let Some(channels) = props.channels() {
                metadata.channels_display = Some(format_audio_channels(channels));
            }
        }

        metadata.isrc = self.isrc();
        metadata.barcode = self.barcode();
        metadata.catalog_number = self.catalog_number();
        metadata.label = self.label();

        metadata.musicbrainz_track_id = self.musicbrainz_track_id();
        metadata.musicbrainz_album_id = self.musicbrainz_album_id();
        metadata.musicbrainz_artist_id = self.musicbrainz_artist_id();
        metadata.musicbrainz_album_artist_id = self.musicbrainz_album_artist_id();
        metadata.musicbrainz_release_group_id = self.musicbrainz_release_group_id();

        metadata.composer = self.composer();
        metadata.lyricist = self.lyricist();
        metadata.conductor = self.conductor();
        metadata.remixer = self.remixer();
        metadata.language = self.language();
        metadata.encoded_by = self.encoded_by();
        metadata.encoder_settings = self.encoder_settings();
        metadata.copyright = self.copyright();
        metadata.publisher = self.publisher();
        metadata.url = self.url();
        metadata.comment = self.comment();
        metadata.content_created = self.content_created();
        metadata.last_used = self.last_used();
        metadata.use_count = self.use_count();

        metadata.movement = self.movement();
        metadata.movement_number = self.movement_number();
        metadata.movement_total = self.movement_total();
        if let Some(mov_num) = metadata.movement_number {
            metadata.movement_display = Some(format_track_number(mov_num, metadata.movement_total));
        }

        metadata
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtSourceOptions {
    pub allow_inferred_url: bool,
    pub allow_mpris_art_url: bool,
}

impl Default for ArtSourceOptions {
    fn default() -> Self {
        Self {
            allow_inferred_url: true,
            allow_mpris_art_url: true,
        }
    }
}

pub fn is_http_art_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

/// Pick the best art source from the available inputs.
///
/// Priority:
/// 1. `mpris:artUrl` if it is an `http(s)://` URL — Discord can consume it
///    directly without a provider upload.
/// 2. A URL inferred from `xesam:url` (e.g. YouTube thumbnail) — also
///    directly usable and avoids touching plasma's temp artwork dump.
/// 3. `mpris:artUrl` for any other scheme (`data:image/...;base64,...`,
///    `file://`, bare path) — needs upload via the provider chain.
/// 4. Embedded picture from the local file's tag.
fn select_art_source(
    art_url: Option<&str>,
    inferred_url: Option<String>,
    embedded_bytes: Option<Vec<u8>>,
) -> Option<ArtSource> {
    if let Some(url) = art_url {
        if is_http_art_url(url) {
            if let Some(src) = ArtSource::from_art_url(url) {
                return Some(src);
            }
        }
    }

    if let Some(url) = inferred_url {
        return Some(ArtSource::Url(url));
    }

    if let Some(url) = art_url {
        if let Some(src) = ArtSource::from_art_url(url) {
            return Some(src);
        }
    }

    embedded_bytes.map(ArtSource::Bytes)
}

/// Derive a public cover-art URL from a known web service URL.
///
/// Web players (YouTube in a browser, Plasma browser integration, etc.)
/// often expose `xesam:url` but no `mpris:artUrl`. For services whose
/// thumbnails are addressable from the page URL alone, return a direct
/// image URL so Discord gets a real cover instead of falling back to the
/// site's static icon.
pub fn infer_art_url_from_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    let scheme = parsed.scheme();
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let host = parsed.host_str()?.to_ascii_lowercase();

    let id = if host == "youtu.be" {
        parsed
            .path_segments()
            .and_then(|mut s| s.next())
            .map(str::to_string)
    } else if host == "youtube.com"
        || host.ends_with(".youtube.com")
        || host == "youtube-nocookie.com"
        || host.ends_with(".youtube-nocookie.com")
    {
        if let Some((_, v)) = parsed.query_pairs().find(|(k, _)| k == "v") {
            Some(v.into_owned())
        } else {
            let mut segments = parsed.path_segments()?;
            let first = segments.next()?;
            match first {
                "shorts" | "embed" | "live" | "v" => segments.next().map(str::to_string),
                _ => None,
            }
        }
    } else {
        None
    }?;

    let id: String = id
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .collect();
    if id.is_empty() || id.len() > 32 {
        return None;
    }
    Some(format!("https://i.ytimg.com/vi/{}/hqdefault.jpg", id))
}

#[cfg(test)]
mod tests {
    use super::{infer_art_url_from_url, select_art_source};
    use crate::cover::sources::ArtSource;
    use std::path::PathBuf;

    const YT_URL: &str = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
    const YT_THUMB: &str = "https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg";
    const PLASMA_FILE: &str = "file:///tmp/plasma-browser-integration_artwork_zmXyTR.jpg";

    fn youtube_inferred() -> Option<String> {
        infer_art_url_from_url(YT_URL)
    }

    #[test]
    fn file_art_url_with_youtube_xesam_prefers_inferred_thumbnail() {
        let got = select_art_source(Some(PLASMA_FILE), youtube_inferred(), None);
        match got {
            Some(ArtSource::Url(url)) => assert_eq!(url, YT_THUMB),
            other => panic!("expected inferred YouTube URL, got {other:?}"),
        }
    }

    #[test]
    fn remote_http_art_url_beats_inferred_thumbnail() {
        let curated = "https://cdn.example.com/cover.png";
        let got = select_art_source(Some(curated), youtube_inferred(), None);
        match got {
            Some(ArtSource::Url(url)) => assert_eq!(url, curated),
            other => panic!("expected curated http URL, got {other:?}"),
        }
    }

    #[test]
    fn data_art_url_with_youtube_xesam_prefers_inferred_thumbnail() {
        let data_uri = "data:image/png;base64,iVBORw0KGgo=";
        let got = select_art_source(Some(data_uri), youtube_inferred(), None);
        match got {
            Some(ArtSource::Url(url)) => assert_eq!(url, YT_THUMB),
            other => panic!("expected inferred YouTube URL, got {other:?}"),
        }
    }

    #[test]
    fn file_art_url_with_non_youtube_xesam_keeps_file() {
        let got = select_art_source(Some(PLASMA_FILE), None, None);
        match got {
            Some(ArtSource::File(path)) => assert_eq!(
                path,
                PathBuf::from("/tmp/plasma-browser-integration_artwork_zmXyTR.jpg")
            ),
            other => panic!("expected file source, got {other:?}"),
        }
    }

    #[test]
    fn no_art_url_with_youtube_xesam_returns_inferred() {
        let got = select_art_source(None, youtube_inferred(), None);
        match got {
            Some(ArtSource::Url(url)) => assert_eq!(url, YT_THUMB),
            other => panic!("expected inferred YouTube URL, got {other:?}"),
        }
    }

    #[test]
    fn no_art_url_no_xesam_with_embedded_returns_bytes() {
        let bytes = vec![1u8, 2, 3, 4];
        let got = select_art_source(None, None, Some(bytes.clone()));
        match got {
            Some(ArtSource::Bytes(b)) => assert_eq!(b, bytes),
            other => panic!("expected embedded bytes, got {other:?}"),
        }
    }

    #[test]
    fn data_art_url_without_inference_falls_back_to_base64() {
        let data_uri = "data:image/png;base64,iVBORw0KGgo=";
        let got = select_art_source(Some(data_uri), None, None);
        match got {
            Some(ArtSource::Base64(payload)) => assert_eq!(payload, "iVBORw0KGgo="),
            other => panic!("expected base64 source, got {other:?}"),
        }
    }

    #[test]
    fn all_inputs_empty_returns_none() {
        assert!(select_art_source(None, None, None).is_none());
    }

    #[test]
    fn youtube_watch_url_yields_thumbnail() {
        let got = infer_art_url_from_url("https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        assert_eq!(
            got.as_deref(),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
        );
    }

    #[test]
    fn youtube_music_watch_url_yields_thumbnail() {
        let got = infer_art_url_from_url("https://music.youtube.com/watch?v=abcDEF12345&list=foo");
        assert_eq!(
            got.as_deref(),
            Some("https://i.ytimg.com/vi/abcDEF12345/hqdefault.jpg")
        );
    }

    #[test]
    fn youtu_be_short_url_yields_thumbnail() {
        let got = infer_art_url_from_url("https://youtu.be/dQw4w9WgXcQ?t=42");
        assert_eq!(
            got.as_deref(),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
        );
    }

    #[test]
    fn youtube_shorts_url_yields_thumbnail() {
        let got = infer_art_url_from_url("https://www.youtube.com/shorts/abc_def-123");
        assert_eq!(
            got.as_deref(),
            Some("https://i.ytimg.com/vi/abc_def-123/hqdefault.jpg")
        );
    }

    #[test]
    fn youtube_embed_and_live_urls_yield_thumbnail() {
        assert_eq!(
            infer_art_url_from_url("https://www.youtube.com/embed/dQw4w9WgXcQ").as_deref(),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
        );
        assert_eq!(
            infer_art_url_from_url("https://www.youtube.com/live/dQw4w9WgXcQ").as_deref(),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
        );
    }

    #[test]
    fn youtube_nocookie_embed_url_yields_thumbnail() {
        let got =
            infer_art_url_from_url("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ?rel=0");
        assert_eq!(
            got.as_deref(),
            Some("https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg")
        );
    }

    #[test]
    fn unrecognized_host_returns_none() {
        assert!(infer_art_url_from_url("https://soundcloud.com/foo/bar").is_none());
        assert!(infer_art_url_from_url("https://example.com/watch?v=dQw4w9WgXcQ").is_none());
    }

    #[test]
    fn youtube_homepage_returns_none() {
        assert!(infer_art_url_from_url("https://www.youtube.com/").is_none());
        assert!(infer_art_url_from_url("https://www.youtube.com/feed/subscriptions").is_none());
    }

    #[test]
    fn non_http_scheme_returns_none() {
        assert!(infer_art_url_from_url("file:///tmp/song.mp3").is_none());
        assert!(infer_art_url_from_url("spotify:track:xyz").is_none());
    }

    #[test]
    fn malformed_video_id_returns_none() {
        assert!(infer_art_url_from_url("https://www.youtube.com/watch?v=").is_none());
        assert!(infer_art_url_from_url("https://youtu.be/").is_none());
    }
}
