use std::{collections::BTreeMap, path::Path, time::Duration};

use lofty::{AudioFile, ItemKey, TaggedFileExt};
use mpris::{PlaybackStatus, Player};
use url::Url;

use crate::{config::PlayerConfig, cover::PROVIDERS, error::Error, player::cmus};

pub struct Context {
    player: Option<Player>,
    metadata: Option<mpris::Metadata>,
    properties: Option<lofty::FileProperties>,
    tag: Option<lofty::Tag>,
    path: Option<String>,
}

impl Context {
    pub fn from_player(player: Player) -> Self {
        let mut context = Context {
            player: Some(player),
            metadata: None,
            properties: None,
            tag: None,
            path: None,
        };

        let metadata = match context.player.as_ref().unwrap().get_metadata() {
            Ok(metadata) => metadata,
            Err(_) => return context,
        };

        context.merge(&Context::from_metadata(metadata));

        if context.config_identity() == "cmus" {
            if let Some(audio_path) = cmus::get_audio_path() {
                if let Ok(cmus_context) = Context::from_path(audio_path) {
                    context.merge(&cmus_context);
                }
            }
        }

        context
    }

    pub fn from_metadata(metadata: mpris::Metadata) -> Self {
        let mut context = Context {
            player: None,
            metadata: Some(metadata.clone()),
            properties: None,
            tag: None,
            path: None,
        };

        let path = match metadata.url() {
            Some(url) => match Url::parse(url) {
                Ok(url) => match url.to_file_path() {
                    Ok(path) => path,
                    Err(_) => return context,
                },
                Err(_) => return context,
            },
            None => return context,
        };

        let other = match Context::from_path(path) {
            Ok(other) => other,
            Err(_) => return context,
        };

        context.merge(&other);

        context
    }

    pub fn from_path<T>(path: T) -> Result<Self, Error>
    where
        T: AsRef<Path>,
    {
        let path = path.as_ref();
        let tagged_file = match lofty::read_from_path(path) {
            Ok(properties) => properties,
            Err(e) => return Err(Error::LoftyError(e)),
        };

        let tag = match tagged_file.primary_tag() {
            Some(tag) => Some(tag.clone()),
            None => tagged_file.first_tag().cloned(),
        };

        let properties = Some(tagged_file.properties().clone());

        let path = Some(path.to_string_lossy().to_string());

        let context = Context {
            player: None,
            metadata: None,
            properties,
            tag,
            path,
        };

        Ok(context)
    }

    fn merge(&mut self, other: &Self) {
        if self.metadata.is_none() {
            self.metadata = other.metadata.clone();
        }

        if self.properties.is_none() {
            self.properties = other.properties.clone();
        }

        if self.tag.is_none() {
            self.tag = other.tag.clone();
        }

        if self.path.is_none() {
            self.path = other.path.clone();
        }
    }

    pub fn player(&self) -> Option<&Player> {
        self.player.as_ref()
    }

    pub fn metadata(&self) -> Option<&mpris::Metadata> {
        self.metadata.as_ref()
    }

    pub fn properties(&self) -> Option<&lofty::FileProperties> {
        self.properties.as_ref()
    }

    pub fn tag(&self) -> Option<&lofty::Tag> {
        self.tag.as_ref()
    }

    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    pub fn title(&self) -> Option<&str> {
        if let Some(metadata) = &self.metadata {
            if let Some(title) = metadata.title() {
                return Some(title);
            }
        }

        if let Some(tag) = &self.tag {
            if let Some(title) = tag.get_string(&ItemKey::TrackTitle) {
                return Some(title);
            }
        }

        None
    }

    pub fn artists(&self) -> Option<Vec<&str>> {
        if let Some(metadata) = &self.metadata {
            if let Some(artists) = metadata.artists() {
                return Some(artists);
            }
        }

        if let Some(tag) = &self.tag {
            let mut artists = Vec::new();
            for artist in tag.get_strings(&ItemKey::TrackArtist) {
                artists.push(artist);
            }
            match artists.len() {
                0 => return None,
                _ => return Some(artists),
            }
        }

        None
    }

    pub fn album_name(&self) -> Option<&str> {
        if let Some(metadata) = &self.metadata {
            if let Some(album_name) = metadata.album_name() {
                return Some(album_name);
            }
        }

        if let Some(tag) = &self.tag {
            if let Some(album_name) = tag.get_string(&ItemKey::AlbumTitle) {
                return Some(album_name);
            }
        }

        None
    }

    pub fn album_artists(&self) -> Option<Vec<&str>> {
        if let Some(metadata) = &self.metadata {
            if let Some(album_artists) = metadata.album_artists() {
                return Some(album_artists);
            }
        }

        if let Some(tag) = &self.tag {
            let mut album_artists = Vec::new();
            for album_artist in tag.get_strings(&ItemKey::AlbumArtist) {
                album_artists.push(album_artist);
            }
            match album_artists.len() {
                0 => return None,
                _ => return Some(album_artists),
            }
        }

        None
    }

    pub fn identity(&self) -> String {
        if let Some(player) = &self.player {
            return player.identity().to_string();
        }

        "Unknown".to_string()
    }

    pub fn config_identity(&self) -> String {
        if let Some(player) = &self.player {
            return player.identity().to_lowercase().replace(" ", "_");
        }

        "default".to_string()
    }

    pub fn unique_name(&self) -> String {
        if let Some(player) = &self.player {
            return player.unique_name().to_string();
        }

        "Unknown".to_string()
    }

    pub fn playback_status(&self) -> PlaybackStatus {
        if let Some(player) = &self.player {
            return player
                .get_playback_status()
                .unwrap_or(PlaybackStatus::Stopped);
        }

        PlaybackStatus::Stopped
    }

    pub async fn cover_url(&self) -> Option<String> {
        for provider in PROVIDERS.iter() {
            let cover_url = provider.get_cover_url(self).await;
            if cover_url.is_some() {
                return cover_url;
            }
        }
        None
    }

    pub fn is_streaming(&self) -> bool {
        if let Some(metadata) = &self.metadata {
            if let Some(url) = metadata.url() {
                if let Ok(url) = Url::parse(url) {
                    return url.scheme() == "http" || url.scheme() == "https";
                }
            }
        }

        false
    }

    pub fn is_ignored(&self) -> bool {
        PlayerConfig::get_or_default(&self.config_identity()).ignore
    }

    pub fn data(&self) -> BTreeMap<String, String> {
        let mut btree_map: BTreeMap<String, String> = BTreeMap::new();

        if let Some(player) = &self.player {
            btree_map.insert("player".to_string(), player.identity().to_string());

            let position_dur = player.get_position().unwrap_or(Duration::from_secs(0));

            let position = format!(
                "{:02}:{:02}",
                position_dur.as_secs() / 60,
                position_dur.as_secs() % 60
            );

            btree_map.insert("position".to_string(), position);

            match player.get_playback_status() {
                Ok(playback_status) => {
                    let status;
                    let status_icon;

                    match playback_status {
                        mpris::PlaybackStatus::Playing => {
                            status = "playing";
                            status_icon = "▶";
                        }
                        mpris::PlaybackStatus::Paused => {
                            status = "paused";
                            status_icon = "⏸";
                        }
                        mpris::PlaybackStatus::Stopped => {
                            status = "stopped";
                            status_icon = "⏹";
                        }
                    };
                    btree_map.insert("status".to_string(), status.to_string());
                    btree_map.insert("status_icon".to_string(), status_icon.to_string());
                }
                Err(_) => {}
            };

            match player.get_volume() {
                Ok(volume) => {
                    btree_map.insert("volume".to_string(), ((volume * 100.0) as u8).to_string())
                }
                Err(_) => None,
            };
        }

        if let Some(metadata) = &self.metadata {
            if let Some(album_artists) = metadata.album_artists() {
                btree_map.insert("album_artists".to_string(), album_artists.join(", "));
            }

            if let Some(album_name) = metadata.album_name() {
                btree_map.insert("album_name".to_string(), album_name.to_string());
            }

            if let Some(artists) = metadata.artists() {
                btree_map.insert("artists".to_string(), artists.join(", "));
            }

            if let Some(auto_rating) = metadata.auto_rating() {
                btree_map.insert("auto_rating".to_string(), auto_rating.to_string());
            }

            if let Some(disc_number) = metadata.disc_number() {
                btree_map.insert("disc_number".to_string(), disc_number.to_string());
            }

            if let Some(length) = metadata.length() {
                let length = format!("{:02}:{:02}", length.as_secs() / 60, length.as_secs() % 60);
                btree_map.insert("length".to_string(), length);
            }

            if let Some(title) = metadata.title() {
                btree_map.insert("title".to_string(), title.to_string());
            }

            if let Some(track_number) = metadata.track_number() {
                btree_map.insert("track_number".to_string(), track_number.to_string());
            }
        }

        if let Some(tag) = &self.tag {
            if let Some(album_artist) = tag.get_string(&ItemKey::AlbumArtist) {
                btree_map.insert("album_artists".to_string(), album_artist.to_string());
            }

            if let Some(album_title) = tag.get_string(&ItemKey::AlbumTitle) {
                btree_map.insert("album_name".to_string(), album_title.to_string());
            }

            if let Some(arranger) = tag.get_string(&ItemKey::Arranger) {
                btree_map.insert("arranger".to_string(), arranger.to_string());
            }

            if let Some(bpm) = tag.get_string(&ItemKey::Bpm) {
                btree_map.insert("bpm".to_string(), bpm.to_string());
            }

            if let Some(catalog_number) = tag.get_string(&ItemKey::CatalogNumber) {
                btree_map.insert("catalog_number".to_string(), catalog_number.to_string());
            }

            if let Some(color) = tag.get_string(&ItemKey::Color) {
                btree_map.insert("color".to_string(), color.to_string());
            }

            if let Some(composer) = tag.get_string(&ItemKey::Composer) {
                btree_map.insert("composer".to_string(), composer.to_string());
            }

            if let Some(conductor) = tag.get_string(&ItemKey::Conductor) {
                btree_map.insert("conductor".to_string(), conductor.to_string());
            }

            if let Some(director) = tag.get_string(&ItemKey::Director) {
                btree_map.insert("director".to_string(), director.to_string());
            }

            if let Some(disc_number) = tag.get_string(&ItemKey::DiscNumber) {
                btree_map.insert("disc_number".to_string(), disc_number.to_string());
            }

            if let Some(disc_total) = tag.get_string(&ItemKey::DiscTotal) {
                btree_map.insert("disc_total".to_string(), disc_total.to_string());
            }

            if let Some(encoded_by) = tag.get_string(&ItemKey::EncodedBy) {
                btree_map.insert("encoded_by".to_string(), encoded_by.to_string());
            }

            if let Some(genre) = tag.get_string(&ItemKey::Genre) {
                btree_map.insert("genre".to_string(), genre.to_string());
            }

            if let Some(label) = tag.get_string(&ItemKey::Label) {
                btree_map.insert("label".to_string(), label.to_string());
            }

            if let Some(language) = tag.get_string(&ItemKey::Language) {
                btree_map.insert("language".to_string(), language.to_string());
            }

            if let Some(language) = tag.get_string(&ItemKey::Language) {
                btree_map.insert("language".to_string(), language.to_string());
            }

            if let Some(lyricist) = tag.get_string(&ItemKey::Lyricist) {
                btree_map.insert("lyricist".to_string(), lyricist.to_string());
            }

            if let Some(mix_dj) = tag.get_string(&ItemKey::MixDj) {
                btree_map.insert("mix_dj".to_string(), mix_dj.to_string());
            }

            if let Some(mix_engineer) = tag.get_string(&ItemKey::MixEngineer) {
                btree_map.insert("mix_engineer".to_string(), mix_engineer.to_string());
            }

            if let Some(mood) = tag.get_string(&ItemKey::Mood) {
                btree_map.insert("mood".to_string(), mood.to_string());
            }

            if let Some(perfomer) = tag.get_string(&ItemKey::Performer) {
                btree_map.insert("perfomer".to_string(), perfomer.to_string());
            }

            if let Some(producer) = tag.get_string(&ItemKey::Producer) {
                btree_map.insert("producer".to_string(), producer.to_string());
            }

            if let Some(publisher) = tag.get_string(&ItemKey::Publisher) {
                btree_map.insert("publisher".to_string(), publisher.to_string());
            }

            if let Some(recording_date) = tag.get_string(&ItemKey::RecordingDate) {
                btree_map.insert("recording_date".to_string(), recording_date.to_string());
            }

            if let Some(track_artist) = tag.get_string(&ItemKey::TrackArtist) {
                btree_map.insert("artists".to_string(), track_artist.to_string());
            }

            if let Some(track_number) = tag.get_string(&ItemKey::TrackNumber) {
                btree_map.insert("track_number".to_string(), track_number.to_string());
            }

            if let Some(track_title) = tag.get_string(&ItemKey::TrackTitle) {
                btree_map.insert("title".to_string(), track_title.to_string());
            }

            if let Some(track_total) = tag.get_string(&ItemKey::TrackTotal) {
                btree_map.insert("track_total".to_string(), track_total.to_string());
            }

            if let Some(year) = tag.get_string(&ItemKey::Year) {
                btree_map.insert("year".to_string(), year.to_string());
            }
        }

        if let Some(properties) = &self.properties {
            if let Some(audio_bitrate) = properties.audio_bitrate() {
                btree_map.insert("audio_bitrate".to_string(), audio_bitrate.to_string());
            }

            if let Some(bit_depth) = properties.bit_depth() {
                btree_map.insert("bit_depth".to_string(), bit_depth.to_string());
            }

            if let Some(channels) = properties.channels() {
                btree_map.insert("channels".to_string(), channels.to_string());
            }

            if let Some(overall_bitrate) = properties.overall_bitrate() {
                btree_map.insert("overall_bitrate".to_string(), overall_bitrate.to_string());
            }

            if let Some(sample_rate) = properties.sample_rate() {
                btree_map.insert("sample_rate".to_string(), sample_rate.to_string());
            }
        }

        btree_map
    }
}

impl PartialEq for Context {
    fn eq(&self, other: &Self) -> bool {
        self.data() == other.data()
    }
}
