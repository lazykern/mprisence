use crate::protocol::{SourceState, Status};
use log::{debug, info, warn};
use mpris_server::{
    zbus::zvariant::ObjectPath, Metadata, Player, Time, TrackId,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Commands received from MPRIS clients (user pressing play/pause in their desktop).
#[derive(Debug, Clone)]
pub enum MprisCommand {
    PlayPause,
    Next,
    Previous,
    Seek(i64),
    SetPosition(i64),
    Stop,
    Play,
    Pause,
}

/// A command from a specific MPRIS player, tagged with its source_id.
pub type TaggedCommand = (String, MprisCommand);

/// Manages multiple MPRIS players — one per browser source (tab + browser combo).
///
/// Each source gets its own `org.mpris.MediaPlayer2.mprisence_<hash>` player
/// so `playerctl -l` shows all active tabs/browsers simultaneously.
pub struct PlayerManager {
    players: HashMap<String, PlayerEntry>,
}

struct PlayerEntry {
    publisher: MprisPublisher,
    /// Spawned local task running `player.run()`. Dropping this entry
    /// drops the `Publisher`, which drops the `Player`, which closes the
    /// D-Bus connection → the spawned task completes naturally.
    _handle: tokio::task::JoinHandle<()>,
}

impl PlayerManager {
    pub fn new() -> Self {
        Self {
            players: HashMap::new(),
        }
    }

    /// Get or create a player for the given source_id.
    /// Each source gets a stable bus name derived from the source_id.
    /// Get or create a player for the given source_id.
    /// Returns None if creation fails.
    pub async fn ensure_player(
        &mut self,
        source_id: &str,
        cmd_tx: &mpsc::Sender<TaggedCommand>,
    ) -> Option<&MprisPublisher> {
        use std::collections::hash_map::Entry;
        match self.players.entry(source_id.to_string()) {
            Entry::Occupied(entry) => Some(&entry.into_mut().publisher),
            Entry::Vacant(entry) => {
                let suffix = make_player_suffix(source_id);
                match MprisPublisher::new(&suffix, source_id, cmd_tx.clone()).await {
                    Ok(publisher) => {
                        let run_task = publisher.run_task();
                        let handle = tokio::task::spawn_local(run_task);
                        info!("Created MPRIS player for source {source_id} → {}", publisher.bus_name());
                        Some(&entry.insert(PlayerEntry { publisher, _handle: handle }).publisher)
                    }
                    Err(e) => {
                        warn!("Failed to create MPRIS player for {source_id}: {e}");
                        None
                    }
                }
            }
        }
    }

    /// Get publisher by source_id (without creating).
    pub fn get(&self, source_id: &str) -> Option<&MprisPublisher> {
        self.players.get(source_id).map(|e| &e.publisher)
    }

    /// Remove a player when its source is gone.
    pub fn remove_player(&mut self, source_id: &str) {
        if self.players.remove(source_id).is_some() {
            debug!("Removed MPRIS player for {source_id}");
        }
    }

    /// Remove players for sources that are no longer alive.
    pub fn retain(&mut self, alive_sources: &[String]) {
        self.players.retain(|id, _| alive_sources.contains(id));
    }

    pub fn player_count(&self) -> usize {
        self.players.len()
    }

    pub fn bus_name_for(&self, source_id: &str) -> Option<&str> {
        self.players
            .get(source_id)
            .map(|e| e.publisher.bus_name())
    }
}

fn make_player_suffix(source_id: &str) -> String {
    // Stable hash of source_id for the bus name suffix
    // Replace non-alphanumeric chars with underscores for D-Bus compliance
    let hash = simple_hash(source_id);
    format!("web_{hash}")
}

fn simple_hash(input: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:08x}", hasher.finish())
}

/// Wraps an MPRIS server player, handling property updates.
pub struct MprisPublisher {
    player: Arc<Player>,
    bus_name: String,
    /// Track the current track ID to detect changes.
    current_track_id: std::sync::Mutex<String>,
}

impl MprisPublisher {
    /// Create a new MPRIS publisher and register on D-Bus.
    /// `cmd_tx` forwards play/pause/next/prev etc, tagged with `source_id`.
    pub async fn new(
        bus_name_suffix: &str,
        source_id: &str,
        cmd_tx: mpsc::Sender<TaggedCommand>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let full_suffix = format!("mprisence_{bus_name_suffix}");
        let bus_name = format!("org.mpris.MediaPlayer2.{full_suffix}");

        let player = Player::builder(&full_suffix)
            .can_play(true)
            .can_pause(true)
            .can_go_next(false)
            .can_go_previous(false)
            .can_seek(false)
            .can_raise(false)
            .build()
            .await?;

        let arc_player = Arc::new(player);

        // Wire D-Bus method callbacks to forward commands to the extension
        let sid = source_id.to_string();
        let tx = cmd_tx.clone();
        arc_player.connect_play_pause(move |_| { let _ = tx.try_send((sid.clone(), MprisCommand::PlayPause)); });

        let sid = source_id.to_string();
        let tx = cmd_tx.clone();
        arc_player.connect_next(move |_| { let _ = tx.try_send((sid.clone(), MprisCommand::Next)); });

        let sid = source_id.to_string();
        let tx = cmd_tx.clone();
        arc_player.connect_previous(move |_| { let _ = tx.try_send((sid.clone(), MprisCommand::Previous)); });

        let sid = source_id.to_string();
        let tx = cmd_tx.clone();
        arc_player.connect_seek(move |_player, offset: Time| {
            let _ = tx.try_send((sid.clone(), MprisCommand::Seek(offset.as_micros())));
        });

        let sid = source_id.to_string();
        let tx = cmd_tx.clone();
        arc_player.connect_set_position(move |_player, _track_id: &TrackId, position: Time| {
            let _ = tx.try_send((sid.clone(), MprisCommand::SetPosition(position.as_micros())));
        });

        let sid = source_id.to_string();
        let tx = cmd_tx.clone();
        arc_player.connect_play(move |_| { let _ = tx.try_send((sid.clone(), MprisCommand::Play)); });

        let sid = source_id.to_string();
        let tx = cmd_tx.clone();
        arc_player.connect_pause(move |_| { let _ = tx.try_send((sid.clone(), MprisCommand::Pause)); });

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

    pub fn bus_name(&self) -> &str {
        &self.bus_name
    }
}

fn format_site_name(site: &str) -> String {
    match site {
        "youtube_music" => "YouTube Music".into(),
        "you_tube" => "YouTube".into(),
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
            if tid.starts_with('/') {
                return tid.clone();
            }
            return format!("/mprisence/track/{tid}");
        }
    }
    let hash_input = format!("{source_id}+{}", title.unwrap_or(""));
    format!("/mprisence/track/{}", simple_hash(&hash_input))
}
