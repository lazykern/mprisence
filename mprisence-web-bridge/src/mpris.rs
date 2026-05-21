use crate::protocol::{SourceState, Status};
use log::{debug, info};
use mpris_server::{
    builder::PlayerBuilder, zbus::zvariant::ObjectPath, zbus::Result as ZbusResult, Metadata,
    Player, Time, TrackId,
};
use std::sync::Arc;
use tokio::sync::mpsc;

/// Commands received from MPRIS clients (user pressing play/pause in their desktop).
#[derive(Debug, Clone)]
pub enum MprisCommand {
    PlayPause,
    Next,
    Previous,
    Seek(i64),          // offset in microseconds
    SetPosition(i64),   // absolute position in microseconds
    Stop,
    Play,
    Pause,
}

/// Wraps an MPRIS server player, handling property updates and command forwarding.
///
/// Design note: `player.run()` returns a `!Send` future (`LocalServerRunTask`).
/// The caller must run it inside a `tokio::select!` loop (requires
/// `#[tokio::main(flavor = "current_thread")]` or `LocalSet`).
pub struct MprisPublisher {
    player: Arc<Player>,
    bus_name: String,
    /// Track the current track ID to detect changes.
    current_track_id: std::sync::Mutex<String>,
}

impl MprisPublisher {
    /// Create a new MPRIS publisher, register on D-Bus, and connect command handlers.
    pub async fn new(
        bus_name_suffix: &str,
        cmd_tx: mpsc::Sender<MprisCommand>,
    ) -> ZbusResult<Self> {
        // Player::builder() takes a suffix — it prepends "org.mpris.MediaPlayer2."
        let suffix = format!("mprisence_{bus_name_suffix}");
        let bus_name = format!("org.mpris.MediaPlayer2.{suffix}");

        let builder: PlayerBuilder = Player::builder(&suffix);
        let player = builder
            .can_play(true)
            .can_pause(true)
            .can_go_next(false)
            .can_go_previous(false)
            .can_seek(false)
            .can_raise(false)
            .build()
            .await?;

        let arc_player = Arc::new(player);

        // Connect synchronous callbacks. Use try_send to avoid deadlock
        // (callbacks run inside the MPRIS event loop on the same thread).
        let tx = cmd_tx.clone();
        arc_player.connect_play_pause(move |_| { let _ = tx.try_send(MprisCommand::PlayPause); });

        let tx = cmd_tx.clone();
        arc_player.connect_next(move |_| { let _ = tx.try_send(MprisCommand::Next); });

        let tx = cmd_tx.clone();
        arc_player.connect_previous(move |_| { let _ = tx.try_send(MprisCommand::Previous); });

        let tx = cmd_tx.clone();
        arc_player.connect_seek(move |_player, offset: Time| {
            let _ = tx.try_send(MprisCommand::Seek(offset.as_micros()));
        });

        let tx = cmd_tx.clone();
        arc_player.connect_set_position(move |_player, _track_id: &TrackId, position: Time| {
            let _ = tx.try_send(MprisCommand::SetPosition(position.as_micros()));
        });

        let tx = cmd_tx.clone();
        arc_player.connect_stop(move |_| { let _ = tx.try_send(MprisCommand::Stop); });

        let tx = cmd_tx.clone();
        arc_player.connect_play(move |_| { let _ = tx.try_send(MprisCommand::Play); });

        let tx = cmd_tx;
        arc_player.connect_pause(move |_| { let _ = tx.try_send(MprisCommand::Pause); });

        info!("MPRIS player published on bus: {bus_name}");

        Ok(Self {
            player: arc_player,
            bus_name,
            current_track_id: std::sync::Mutex::new(String::new()),
        })
    }

    /// Return the `!Send` MPRIS server run task.
    pub fn run_task(&self) -> mpris_server::LocalServerRunTask {
        self.player.run()
    }

    /// Update the MPRIS player state from a source.
    pub async fn publish(&self, source: Option<&SourceState>) {
        let player = &self.player;

        let identity = source
            .map(|s| format_site_name(&s.site))
            .unwrap_or_default();
        let _ = player.set_identity(&identity).await;

        let status = source
            .map(|s| match s.playback.status {
                Status::Playing => mpris_server::PlaybackStatus::Playing,
                Status::Paused => mpris_server::PlaybackStatus::Paused,
                Status::Stopped => mpris_server::PlaybackStatus::Stopped,
            })
            .unwrap_or(mpris_server::PlaybackStatus::Stopped);
        let _ = player.set_playback_status(status).await;

        let metadata = build_metadata(source);
        let _ = player.set_metadata(metadata).await;

        if let Some(s) = source {
            let _ = player.set_can_play(s.capabilities.play_pause).await;
            let _ = player.set_can_pause(s.capabilities.play_pause).await;
            let _ = player.set_can_go_next(s.capabilities.next).await;
            let _ = player.set_can_go_previous(s.capabilities.previous).await;
            let can_seek = s.capabilities.seek || s.capabilities.set_position;
            let _ = player.set_can_seek(can_seek).await;
        } else {
            let _ = player.set_can_play(false).await;
            let _ = player.set_can_pause(false).await;
            let _ = player.set_can_go_next(false).await;
            let _ = player.set_can_go_previous(false).await;
            let _ = player.set_can_seek(false).await;
        }

        // Track change detection
        let new_track_id = source
            .and_then(|s| s.metadata.track_id.as_deref())
            .unwrap_or("");
        let mut cached_track = self.current_track_id.lock().unwrap();
        if *cached_track != new_track_id && !new_track_id.is_empty() {
            *cached_track = new_track_id.to_string();
            info!(
                "Track changed: {identity} — {title}",
                identity = identity,
                title = source
                    .and_then(|s| s.metadata.title.as_deref())
                    .unwrap_or("(unknown)")
            );
        }

        debug!(
            "MPRIS player {bus} updated (status={status}, identity={identity})",
            bus = self.bus_name,
            status = source
                .map(|s| format!("{:?}", s.playback.status))
                .unwrap_or("stopped".into()),
        );
    }

    /// Emit a Seeked signal.
    #[allow(dead_code)]
    pub async fn seeked(&self, position_us: i64) {
        let _ = self.player.seeked(Time::from_micros(position_us)).await;
    }

    pub fn bus_name(&self) -> &str {
        &self.bus_name
    }
}

fn format_site_name(site: &str) -> String {
    match site {
        "youtube_music" => "YouTube Music".into(),
        "youtube" => "YouTube".into(),
        "spotify" => "Spotify".into(),
        "soundcloud" => "SoundCloud".into(),
        "bandcamp" => "Bandcamp".into(),
        "tidal" => "Tidal".into(),
        "apple_music" => "Apple Music".into(),
        "generic" => "Browser".into(),
        other => other
            .split('_')
            .map(|w| {
                let mut c = w.chars();
                match c.next() {
                    None => String::new(),
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    }
}

fn build_metadata(source: Option<&SourceState>) -> Metadata {
    let mut builder = Metadata::builder();

    if let Some(s) = source {
        let meta = &s.metadata;

        let track_id_str = make_track_id(&s.source_id, &meta.track_id, meta.title.as_deref());
        // ObjectPath::try_from(&str) converts a string to a valid D-Bus object path
        if let Ok(path) = ObjectPath::try_from(track_id_str.as_str()) {
            builder = builder.trackid(path);
        }

        if let Some(title) = &meta.title {
            builder = builder.title(title);
        }
        if let Some(album) = &meta.album {
            builder = builder.album(album);
        }
        if !meta.artist.is_empty() {
            let artists: Vec<&str> = meta.artist.iter().map(|s| s.as_str()).collect();
            builder = builder.artist(artists);
        }
        if !meta.album_artist.is_empty() {
            let album_artists: Vec<&str> =
                meta.album_artist.iter().map(|s| s.as_str()).collect();
            builder = builder.album_artist(album_artists);
        }
        if let Some(art_url) = &meta.art_url {
            builder = builder.art_url(art_url);
        }

        let length_us = s.playback.duration_ms * 1000;
        builder = builder.length(Time::from_micros(length_us as i64));

        builder = builder.url(&s.url);
    }

    builder.build()
}

fn make_track_id(source_id: &str, track_id: &Option<String>, title: Option<&str>) -> String {
    if let Some(tid) = track_id {
        if !tid.is_empty() {
            // Ensure it starts with /
            if tid.starts_with('/') {
                return tid.clone();
            }
            return format!("/mprisence/track/{tid}");
        }
    }
    let hash_input = format!("{source_id}+{}", title.unwrap_or(""));
    format!("/mprisence/track/{}", simple_hash(&hash_input))
}

fn simple_hash(input: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
