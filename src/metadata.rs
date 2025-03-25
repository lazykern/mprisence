use std::path::Path;
use std::time::Duration;

use lofty::{
    file::{AudioFile, TaggedFile, TaggedFileExt},
    prelude::*,
    properties::FileProperties,
};
use log::trace;
use mpris::Metadata;
use serde::Serialize;
use url::Url;
use crate::utils::{format_duration, format_track_number, format_audio_channels, format_bitrate, format_sample_rate, format_bit_depth};
use crate::cover::sources::ArtSource;

macro_rules! impl_metadata_getter {
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
}

/// A template-friendly representation of metadata with non-optional fields and sensible defaults.
/// This struct is designed to be easily used with handlebars templates.
#[derive(Debug, Clone, Serialize)]
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
    pub genre: Option<String>,
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
}

impl Default for MediaMetadata {
    fn default() -> Self {
        Self {
            title: None,
            artists: vec![],
            artist_display: None,
            album: None,
            album_artists: vec![],
            album_artist_display: None,
            track_number: None,
            track_total: None,
            track_display: None,
            disc_number: None,
            disc_total: None,
            disc_display: None,
            genre: None,
            year: None,
            duration_secs: None,
            duration_display: None,
            initial_key: None,
            bpm: None,
            mood: None,
            bitrate_display: None,
            sample_rate_display: None,
            bit_depth_display: None,
            channels_display: None,
            isrc: None,
            barcode: None,
            catalog_number: None,
            label: None,
            musicbrainz_track_id: None,
            musicbrainz_album_id: None,
            musicbrainz_artist_id: None,
            musicbrainz_album_artist_id: None,
            musicbrainz_release_group_id: None,
        }
    }
}

pub struct MetadataSource {
    mpris_metadata: Option<Metadata>,
    tagged_file: Option<TaggedFile>,
}

impl MetadataSource {
    pub fn new(mpris_metadata: Option<Metadata>, lofty_tagged_file: Option<TaggedFile>) -> Self {
        Self {
            mpris_metadata,
            tagged_file: lofty_tagged_file,
        }
    }

    pub fn from_mpris(metadata: Metadata) -> Self {
        let tagged_file = metadata
            .url()
            .and_then(|url| Self::lofty_tag_from_url(url).ok());
        Self::new(Some(metadata), tagged_file)
    }

    fn lofty_tag_from_url<S: AsRef<str>>(url: S) -> Result<TaggedFile, String> {
        let url = Url::parse(url.as_ref()).map_err(|e| e.to_string())?;
        if url.scheme() == "file" {
            let path = url.path();
            Self::lofty_tag_from_path(path)
        } else {
            Err(format!("Unsupported URL scheme: {}", url.scheme()))
        }
    }

    fn lofty_tag_from_path<P: AsRef<Path>>(path: P) -> Result<TaggedFile, String> {
        let tagged_file = lofty::read_from_path(path).map_err(|e| e.to_string())?;
        Ok(tagged_file)
    }

    impl_metadata_getter!(title, "xesam:title", &ItemKey::TrackTitle);
    impl_metadata_getter!(album, "xesam:album", &ItemKey::AlbumTitle);
    impl_metadata_getter!(genre, "xesam:genre", &ItemKey::Genre);
    impl_metadata_getter!(initial_key, "xesam:initialKey", &ItemKey::InitialKey);
    impl_metadata_getter!(bpm, "xesam:bpm", &ItemKey::Bpm);
    impl_metadata_getter!(mood, "xesam:mood", &ItemKey::Mood);

    impl_metadata_getter!(isrc, "xesam:isrc", &ItemKey::Isrc);
    impl_metadata_getter!(barcode, "xesam:barcode", &ItemKey::Barcode);
    impl_metadata_getter!(
        catalog_number,
        "xesam:catalogNumber",
        &ItemKey::CatalogNumber
    );
    impl_metadata_getter!(label, "xesam:label", &ItemKey::Label);

    impl_metadata_getter!(
        musicbrainz_track_id,
        "xesam:musicbrainzTrackID",
        &ItemKey::MusicBrainzTrackId
    );
    impl_metadata_getter!(
        musicbrainz_album_id,
        "xesam:musicbrainzAlbumID",
        &ItemKey::MusicBrainzReleaseId
    );
    impl_metadata_getter!(
        musicbrainz_artist_id,
        "xesam:musicbrainzArtistID",
        &ItemKey::MusicBrainzArtistId
    );
    impl_metadata_getter!(
        musicbrainz_album_artist_id,
        "xesam:musicbrainzAlbumArtistID",
        &ItemKey::MusicBrainzReleaseArtistId
    );
    impl_metadata_getter!(
        musicbrainz_release_group_id,
        "xesam:musicbrainzReleaseGroupID",
        &ItemKey::MusicBrainzReleaseGroupId
    );

    impl_metadata_getter!(
        track_number,
        "xesam:trackNumber",
        &ItemKey::TrackNumber,
        parse_u32
    );
    impl_metadata_getter!(
        track_total,
        "xesam:trackTotal",
        &ItemKey::TrackTotal,
        parse_u32
    );
    impl_metadata_getter!(
        disc_number,
        "xesam:discNumber",
        &ItemKey::DiscNumber,
        parse_u32
    );
    impl_metadata_getter!(
        disc_total,
        "xesam:discTotal",
        &ItemKey::DiscTotal,
        parse_u32
    );
    impl_metadata_getter!(year, "xesam:year", &ItemKey::Year, parse_u32);

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
                    .and_then(|tag| tag.get_string(&ItemKey::TrackArtist))
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
                    .and_then(|tag| tag.get_string(&ItemKey::AlbumArtist))
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

    pub fn art_source(&self) -> Option<ArtSource> {
        trace!("Getting art source from metadata");
        
        self.mpris_metadata
            .as_ref()
            .and_then(|m| m.art_url())
            .and_then(ArtSource::from_art_url)
            .or_else(|| {
                self.tagged_file
                    .as_ref()
                    .and_then(|t| t.primary_tag())
                    .and_then(|tag| tag.pictures().first())
                    .map(|picture| ArtSource::Bytes(picture.data().to_vec()))
            })
    }

    #[allow(dead_code)]
    pub fn mpris_metadata(&self) -> Option<&Metadata> {
        self.mpris_metadata.as_ref()
    }

    #[allow(dead_code)]
    pub fn lofty_tag(&self) -> Option<&TaggedFile> {
        self.tagged_file.as_ref()
    }

    pub fn to_media_metadata(&self) -> MediaMetadata {
        let mut metadata = MediaMetadata::default();

        metadata.title = self.title();

        if let Some(artists) = self.artists() {
            metadata.artists = artists.clone();
            metadata.artist_display = Some(artists.join(", "));
        }

        metadata.album = self.album();

        if let Some(album_artists) = self.album_artists() {
            metadata.album_artists = album_artists.clone();
            metadata.album_artist_display = Some(album_artists.join(", "));
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

        metadata.genre = self.genre();
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

        metadata
    }
}
