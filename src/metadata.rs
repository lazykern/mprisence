use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;

use crate::cover::sources::ArtSource;
use crate::utils::{
    format_audio_channels, format_bit_depth, format_bitrate, format_duration, format_sample_rate,
    format_track_number,
};
use blake3::Hasher;
use lofty::{
    config::ParseOptions,
    file::{AudioFile, TaggedFile, TaggedFileExt},
    prelude::*,
    probe::Probe,
    properties::FileProperties,
};
use log::trace;
use mpris::Metadata;
use serde::Serialize;
use url::Url;

fn mpris_value_as_u32(value: &mpris::MetadataValue) -> Option<u32> {
    value
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .or_else(|| value.as_i64().and_then(|v| u32::try_from(v).ok()))
        .or_else(|| value.as_str().and_then(|s| s.trim().parse().ok()))
}

fn parse_year(s: &str) -> Option<u32> {
    let digits = s.trim().get(..4)?;
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|&y| y > 0)
}

fn mpris_value_as_string(value: &mpris::MetadataValue) -> Option<String> {
    let parts: Vec<&str> = value
        .as_str_array()?
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

macro_rules! impl_metadata_getter {
    // String getter with both MPRIS and Lofty (first matching tag key wins)
    ($name:ident, $mpris_key:expr, [$($lofty_key:expr),+]) => {
        pub fn $name(&self) -> Option<String> {
            trace!(concat!(
                "Getting ",
                stringify!($name),
                " from metadata sources"
            ));
            self.mpris_metadata
                .as_ref()
                .and_then(|m| m.get($mpris_key))
                .and_then(mpris_value_as_string)
                .or_else(|| {
                    let tag = self.tagged_file.as_ref()?.primary_tag()?;
                    [$($lofty_key),+]
                        .into_iter()
                        .find_map(|key| tag.get_string(key))
                        .map(String::from)
                })
        }
    };
    ($name:ident, $mpris_key:expr, $lofty_key:expr) => {
        impl_metadata_getter!($name, $mpris_key, [$lofty_key]);
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
                .and_then(|m| m.get($mpris_key))
                .and_then(mpris_value_as_u32)
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
                .and_then(|v| v.as_str_array())
                .map(|arr| arr.into_iter().map(String::from).collect())
                .or_else(|| self.tag_strings($lofty_key))
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
                .and_then(mpris_value_as_string)
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
                .and_then(mpris_value_as_u32)
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
    cache_key: OnceLock<String>,
    cover_cache_key: OnceLock<String>,
}

impl MetadataSource {
    pub fn new(mpris_metadata: Option<Metadata>, lofty_tagged_file: Option<TaggedFile>) -> Self {
        Self {
            mpris_metadata,
            tagged_file: lofty_tagged_file,
            override_url: None,
            cache_key: OnceLock::new(),
            cover_cache_key: OnceLock::new(),
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
        let path = Self::local_path_from_url(url.as_ref())?;
        Probe::open(path)
            .map_err(|e| e.to_string())?
            .options(ParseOptions::new().read_cover_art(false))
            .read()
            .map_err(|e| e.to_string())
    }

    fn local_path_from_url(url: &str) -> Result<PathBuf, String> {
        let url = Url::parse(url).map_err(|e| e.to_string())?;
        if url.scheme() != "file" {
            return Err(format!("Unsupported URL scheme: {}", url.scheme()));
        }
        url.to_file_path()
            .map_err(|_| format!("Invalid file URL: {url}"))
    }

    pub(crate) fn local_file_path(&self) -> Option<PathBuf> {
        self.url()
            .and_then(|url| Self::local_path_from_url(&url).ok())
    }

    pub(crate) fn embedded_art_from_loaded_tag(&self) -> Option<ArtSource> {
        Self::first_picture_bytes(self.tagged_file.as_ref()?).map(ArtSource::Bytes)
    }

    pub(crate) fn embedded_art_from_path(path: &Path) -> Result<Option<ArtSource>, String> {
        let mut tagged_file = Probe::open(path)
            .map_err(|e| e.to_string())?
            .options(ParseOptions::new().read_properties(false))
            .read()
            .map_err(|e| e.to_string())?;
        let picture = tagged_file.primary_tag_mut().and_then(|tag| {
            (!tag.pictures().is_empty()).then(|| tag.remove_picture(0).into_data())
        });
        Ok(picture.map(ArtSource::Bytes))
    }

    fn first_picture_bytes(tagged_file: &TaggedFile) -> Option<Vec<u8>> {
        tagged_file
            .primary_tag()
            .and_then(|tag| tag.pictures().first())
            .map(|picture| picture.data().to_vec())
    }

    impl_metadata_getter!(title, "xesam:title", ItemKey::TrackTitle);
    impl_metadata_getter!(album, "xesam:album", ItemKey::AlbumTitle);
    impl_metadata_getter!(initial_key, "xesam:initialKey", ItemKey::InitialKey);
    pub fn bpm(&self) -> Option<String> {
        trace!("Getting bpm from metadata sources");
        self.mpris_metadata
            .as_ref()
            .and_then(|m| {
                m.get("xesam:audioBPM")
                    .and_then(mpris_value_as_u32)
                    .filter(|&bpm| bpm > 0)
                    .map(|bpm| bpm.to_string())
                    .or_else(|| m.get("xesam:bpm").and_then(mpris_value_as_string))
            })
            .or_else(|| {
                self.tagged_file
                    .as_ref()?
                    .primary_tag()?
                    .get_string(ItemKey::Bpm)
                    .map(String::from)
            })
    }
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
        [ItemKey::MusicBrainzRecordingId, ItemKey::MusicBrainzTrackId]
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
    pub fn year(&self) -> Option<u32> {
        trace!("Getting year from metadata sources");
        self.mpris_metadata
            .as_ref()
            .and_then(|m| {
                m.get("xesam:year")
                    .and_then(|v| {
                        mpris_value_as_u32(v)
                            .filter(|&y| y > 0)
                            .or_else(|| v.as_str().and_then(parse_year))
                    })
                    .or_else(|| {
                        m.get("xesam:contentCreated")
                            .and_then(|v| v.as_str())
                            .and_then(parse_year)
                    })
            })
            .or_else(|| {
                let tag = self.tagged_file.as_ref()?.primary_tag()?;
                [
                    ItemKey::RecordingDate,
                    ItemKey::Year,
                    ItemKey::ReleaseDate,
                    ItemKey::OriginalReleaseDate,
                ]
                .into_iter()
                .find_map(|key| tag.get_string(key).and_then(parse_year))
            })
    }

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
    impl_metadata_getter!(
        comment,
        "xesam:comment",
        [ItemKey::Comment, ItemKey::Description]
    );

    impl_metadata_getter!(genres, "xesam:genre", ItemKey::Genre, array);
    impl_metadata_getter!(copyright, "xesam:copyright", ItemKey::CopyrightMessage);
    impl_metadata_getter!(publisher, "xesam:publisher", ItemKey::Publisher);
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
            .or_else(|| self.tag_strings(ItemKey::TrackArtist))
    }

    pub fn album_artists(&self) -> Option<Vec<String>> {
        trace!("Getting album artists from metadata sources");
        self.mpris_metadata
            .as_ref()
            .and_then(|m| m.album_artists())
            .map(|artists| artists.iter().map(|s| s.to_string()).collect())
            .or_else(|| self.tag_strings(ItemKey::AlbumArtist))
    }

    fn tag_strings(&self, key: ItemKey) -> Option<Vec<String>> {
        let values: Vec<String> = self
            .tagged_file
            .as_ref()?
            .primary_tag()?
            .get_strings(key)
            .map(String::from)
            .collect();
        (!values.is_empty()).then_some(values)
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

        options
            .allow_mpris_art_url
            .then(|| self.mpris_metadata.as_ref().and_then(|m| m.art_url()))
            .flatten()
            .and_then(ArtSource::from_art_url)
    }

    pub fn mpris_metadata(&self) -> Option<&Metadata> {
        self.mpris_metadata.as_ref()
    }

    /// Exposed for the integration test crate and potential external consumers.
    #[allow(dead_code)]
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
                    if !album_artists.is_empty() && Some(&album_artists) != self.artists().as_ref()
                    {
                        album_artists.sort_unstable();
                        key_components.push(format!("album_artists:{}", album_artists.join("|")));
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

    pub fn cover_cache_key(&self) -> &str {
        self.cover_cache_key
            .get_or_init(|| self.generate_cover_cache_key())
    }

    fn generate_cover_cache_key(&self) -> String {
        let mut components = Vec::new();

        if let Some(id) = self
            .musicbrainz_release_group_id()
            .filter(|id| !id.is_empty())
        {
            components.push(format!("release_group:{id}"));
        }
        if let Some(id) = self.musicbrainz_album_id().filter(|id| !id.is_empty()) {
            components.push(format!("release:{id}"));
        }
        if let Some(album) = self.album().filter(|album| !album.is_empty()) {
            components.push(format!("album:{album}"));

            let mut album_artists = self
                .album_artists()
                .filter(|artists| !artists.is_empty())
                .or_else(|| self.artists().filter(|artists| !artists.is_empty()))
                .unwrap_or_default();
            album_artists.sort_unstable();
            if !album_artists.is_empty() {
                components.push(format!("album_artists:{}", album_artists.join("|")));
            }
        }
        if let Some(barcode) = self.barcode().filter(|barcode| !barcode.is_empty()) {
            components.push(format!("barcode:{barcode}"));
        }

        if components.is_empty() {
            return self.cache_key().to_string();
        }

        let mut hasher = Hasher::new();
        hasher.update(b"mprisence-cover-metadata-v1\0");
        hasher.update(components.join("||").as_bytes());
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
            let album_artists: Vec<String> = album_artists
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect();
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
    pub allow_mpris_art_url: bool,
}

impl Default for ArtSourceOptions {
    fn default() -> Self {
        Self {
            allow_mpris_art_url: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::cover::sources::ArtSource;
    use lofty::file::TaggedFileExt;
    use mpris::Metadata;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::{fs, time::SystemTime};
    use url::Url;

    const PLASMA_FILE: &str = "file:///tmp/plasma-browser-integration_artwork_zmXyTR.jpg";

    #[test]
    fn remote_http_art_url_wins() {
        let curated = "https://cdn.example.com/cover.png";
        let got = metadata_with_art_url(curated).art_source_with_options(Default::default());
        match got {
            Some(ArtSource::Url(url)) => assert_eq!(url, curated),
            other => panic!("expected curated http URL, got {other:?}"),
        }
    }

    #[test]
    fn file_art_url_keeps_file() {
        let got = metadata_with_art_url(PLASMA_FILE).art_source_with_options(Default::default());
        match got {
            Some(ArtSource::File(path)) => assert_eq!(
                path,
                PathBuf::from("/tmp/plasma-browser-integration_artwork_zmXyTR.jpg")
            ),
            other => panic!("expected file source, got {other:?}"),
        }
    }

    #[test]
    fn no_art_url_defers_embedded_art() {
        let got = super::MetadataSource::new(Some(Metadata::new("/test/1")), None)
            .art_source_with_options(Default::default());
        assert!(got.is_none());
    }

    #[test]
    fn data_art_url_falls_back_to_base64() {
        let data_uri = "data:image/png;base64,iVBORw0KGgo=";
        let got = metadata_with_art_url(data_uri).art_source_with_options(Default::default());
        match got {
            Some(ArtSource::Base64(payload)) => assert_eq!(payload, "iVBORw0KGgo="),
            other => panic!("expected base64 source, got {other:?}"),
        }
    }

    #[test]
    fn all_inputs_empty_returns_none() {
        let source = super::MetadataSource::new(None, None);
        assert!(source.art_source_with_options(Default::default()).is_none());
    }

    #[test]
    fn embedded_art_is_loaded_only_by_the_fallback_reader() {
        let picture = b"embedded-picture";
        let path = temp_flac_path();
        fs::write(&path, minimal_flac_with_picture(picture)).unwrap();

        let mut values = HashMap::new();
        values.insert(
            "xesam:url".to_string(),
            Url::from_file_path(&path).unwrap().to_string().into(),
        );
        let normal = super::MetadataSource::from_mpris_with_override(Metadata::from(values), None);
        let normal = normal.lofty_tag().unwrap();
        assert!(normal.tags().iter().all(|tag| tag.pictures().is_empty()));

        let embedded = super::MetadataSource::embedded_art_from_path(&path)
            .unwrap()
            .unwrap();
        fs::remove_file(path).unwrap();

        match embedded {
            ArtSource::Bytes(bytes) => assert_eq!(bytes, picture),
            other => panic!("expected embedded bytes, got {other:?}"),
        }
    }

    fn mpris_source(entries: Vec<(&str, mpris::MetadataValue)>) -> super::MetadataSource {
        let data: HashMap<String, mpris::MetadataValue> = entries
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
        super::MetadataSource::new(Some(Metadata::from(data)), None)
    }

    fn str_array(values: &[&str]) -> mpris::MetadataValue {
        mpris::MetadataValue::Array(values.iter().map(|&v| v.into()).collect())
    }

    fn tagged_source(items: &[(lofty::tag::ItemKey, &str)]) -> super::MetadataSource {
        let mut tag = lofty::tag::Tag::new(lofty::tag::TagType::VorbisComments);
        for (key, value) in items {
            assert!(tag.insert_text(*key, value.to_string()));
        }
        let tagged = lofty::file::TaggedFile::new(
            lofty::file::FileType::Flac,
            Default::default(),
            vec![tag],
        );
        super::MetadataSource::new(None, Some(tagged))
    }

    #[test]
    fn year_from_integer_mpris_value() {
        let source = mpris_source(vec![("xesam:year", mpris::MetadataValue::I32(2019))]);
        assert_eq!(source.year(), Some(2019));
    }

    #[test]
    fn year_from_string_mpris_value() {
        let source = mpris_source(vec![("xesam:year", "2022-08-05".into())]);
        assert_eq!(source.year(), Some(2022));
    }

    #[test]
    fn year_falls_back_to_content_created() {
        let source = mpris_source(vec![(
            "xesam:contentCreated",
            "2022-08-05T00:00:00+07:00".into(),
        )]);
        assert_eq!(source.year(), Some(2022));
    }

    #[test]
    fn year_from_full_date_tags() {
        use lofty::tag::ItemKey;
        assert_eq!(
            tagged_source(&[(ItemKey::RecordingDate, "2019-05-12")]).year(),
            Some(2019)
        );
        assert_eq!(
            tagged_source(&[(ItemKey::Year, "2022-08-05")]).year(),
            Some(2022)
        );
        assert_eq!(tagged_source(&[(ItemKey::Year, "unknown")]).year(), None);
    }

    #[test]
    fn integer_track_number_from_mpris() {
        let source = mpris_source(vec![("xesam:trackNumber", mpris::MetadataValue::I32(3))]);
        assert_eq!(source.track_number(), Some(3));
    }

    #[test]
    fn string_array_mpris_values_are_joined() {
        let source = mpris_source(vec![
            ("xesam:composer", str_array(&["Comp1", "Comp2"])),
            ("xesam:comment", str_array(&["hello"])),
        ]);
        assert_eq!(source.composer().as_deref(), Some("Comp1, Comp2"));
        assert_eq!(source.comment().as_deref(), Some("hello"));
    }

    #[test]
    fn bpm_from_spec_audio_bpm_key() {
        let source = mpris_source(vec![("xesam:audioBPM", mpris::MetadataValue::I32(128))]);
        assert_eq!(source.bpm().as_deref(), Some("128"));
    }

    #[test]
    fn tag_fallbacks_cover_alternate_keys() {
        use lofty::tag::ItemKey;
        let source = tagged_source(&[
            (ItemKey::Description, "hello comment"),
            (ItemKey::CopyrightMessage, "Cpy"),
            (ItemKey::Publisher, "Pub"),
            (ItemKey::MusicBrainzRecordingId, "mbt"),
        ]);
        assert_eq!(source.comment().as_deref(), Some("hello comment"));
        assert_eq!(source.copyright().as_deref(), Some("Cpy"));
        assert_eq!(source.publisher().as_deref(), Some("Pub"));
        assert_eq!(source.musicbrainz_track_id().as_deref(), Some("mbt"));
    }

    #[test]
    fn multi_value_tags_keep_every_value() {
        use lofty::tag::{ItemKey, ItemValue, Tag, TagItem, TagType};
        let mut tag = Tag::new(TagType::VorbisComments);
        for (key, value) in [
            (ItemKey::TrackArtist, "A1"),
            (ItemKey::TrackArtist, "A2"),
            (ItemKey::Genre, "Dance"),
            (ItemKey::Genre, "Electronica"),
        ] {
            assert!(tag.push(TagItem::new(key, ItemValue::Text(value.to_string()))));
        }
        let tagged = lofty::file::TaggedFile::new(
            lofty::file::FileType::Flac,
            Default::default(),
            vec![tag],
        );
        let source = super::MetadataSource::new(None, Some(tagged));
        assert_eq!(source.artists(), Some(vec!["A1".into(), "A2".into()]));
        assert_eq!(
            source.genres(),
            Some(vec!["Dance".into(), "Electronica".into()])
        );
    }

    fn metadata_with_art_url(art_url: &str) -> super::MetadataSource {
        let mut data = HashMap::new();
        data.insert("mpris:artUrl".to_string(), art_url.into());
        super::MetadataSource::new(Some(Metadata::from(data)), None)
    }

    fn temp_flac_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "mprisence-lazy-cover-{}-{}.flac",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    fn minimal_flac_with_picture(picture: &[u8]) -> Vec<u8> {
        let mut picture_block = Vec::new();
        push_u32(&mut picture_block, 3);
        push_u32(&mut picture_block, 9);
        picture_block.extend_from_slice(b"image/png");
        push_u32(&mut picture_block, 0);
        push_u32(&mut picture_block, 1);
        push_u32(&mut picture_block, 1);
        push_u32(&mut picture_block, 24);
        push_u32(&mut picture_block, 0);
        push_u32(&mut picture_block, picture.len() as u32);
        picture_block.extend_from_slice(picture);

        let mut flac = b"fLaC".to_vec();
        flac.extend_from_slice(&[0, 0, 0, 34]);
        flac.extend_from_slice(&[0; 34]);
        flac.push(0x86);
        let len = picture_block.len() as u32;
        flac.extend_from_slice(&[
            ((len >> 16) & 0xff) as u8,
            ((len >> 8) & 0xff) as u8,
            (len & 0xff) as u8,
        ]);
        flac.extend_from_slice(&picture_block);
        flac
    }

    fn push_u32(buffer: &mut Vec<u8>, value: u32) {
        buffer.extend_from_slice(&value.to_be_bytes());
    }
}
