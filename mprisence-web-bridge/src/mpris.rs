use crate::protocol::{MediaMetadata, SourceState, Status};
use log::{debug, info, warn};
use mpris_server::{
    zbus::zvariant::ObjectPath, Metadata, Player, Time, TrackId,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Metadata fields that feed the D-Bus `Metadata` property. Compared as a unit:
/// any difference means the whole `Metadata` is rebuilt and re-emitted.
#[derive(Debug, Default, Clone, PartialEq)]
struct MetaSnapshot {
    track_id: String,
    title: String,
    artists: Vec<String>,
    album: String,
    album_artists: Vec<String>,
    art_url: String,
    length_us: i64,
    url: String,
}

/// Capability flags (`CanPlay`, `CanGoNext`, ...).
#[derive(Debug, Default, Clone, PartialEq)]
struct CapsSnapshot {
    can_play_pause: bool,
    can_next: bool,
    can_previous: bool,
    can_seek: bool,
}

/// Exact MPRIS state last pushed to D-Bus for one player.
#[derive(Debug, Default, Clone)]
struct PublishedSnapshot {
    identity: String,
    status: Option<Status>,
    meta: MetaSnapshot,
    caps: CapsSnapshot,
}

/// Which MPRIS property groups changed between two snapshots.
#[derive(Debug, Default, PartialEq)]
struct PublishDecision {
    identity: bool,
    status: bool,
    metadata: bool,
    caps: bool,
}

impl PublishDecision {
    fn any(&self) -> bool {
        self.identity || self.status || self.metadata || self.caps
    }
}

/// Pure diff: compare the desired snapshot against the last published one.
fn compute_publish_decision(prev: &PublishedSnapshot, next: &PublishedSnapshot) -> PublishDecision {
    PublishDecision {
        identity: prev.identity != next.identity,
        status: prev.status != next.status,
        metadata: prev.meta != next.meta,
        caps: prev.caps != next.caps,
    }
}

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
    /// Each source gets a stable bus name derived from the source_id + site.
    /// Returns None if creation fails.
    pub async fn ensure_player(
        &mut self,
        source_id: &str,
        site: &str,
        cmd_tx: &mpsc::Sender<TaggedCommand>,
    ) -> Option<&MprisPublisher> {
        use std::collections::hash_map::Entry;
        match self.players.entry(source_id.to_string()) {
            Entry::Occupied(entry) => Some(&entry.into_mut().publisher),
            Entry::Vacant(entry) => {
                let suffix = make_player_suffix(source_id, site);
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

/// Stable config key all bridge MPRIS players resolve to.
pub const BRIDGE_CONFIG_KEY: &str = "mprisence_web";

fn make_player_suffix(source_id: &str, site: &str) -> String {
    // Parseable bus: mprisence_web.<site>.<hexhash>
    // site is D-Bus-safe already (lowercase, underscore-separated).
    let hash = simple_hash(source_id);
    format!("web.{site}.{hash}")
}

fn simple_hash(input: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    input.hash(&mut hasher);
    format!("{:08x}", hasher.finish())
}

/// Returns the MPRIS bus suffix for a bridge player.
/// Produces `mprisence_web.<site>.<hash>` so `canonical_player_bus_name`
/// can extract the stable `mprisence_web` prefix.
pub fn bridge_player_suffix(_source_id: &str, _site: &str) -> String {
    make_player_suffix(_source_id, _site)
}

/// Escape a value for inclusion in an MPRIS metadata key.
/// Follows D-Bus object path rules (only [A-Za-z0-9_]).
#[allow(dead_code)]
fn dbus_safe_value(raw: &str) -> String {
    raw.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect::<String>()
}

/// Wraps an MPRIS server player, handling property updates.
pub struct MprisPublisher {
    player: Arc<Player>,
    bus_name: String,
    /// Last state pushed to D-Bus. The diffing publisher compares against this.
    last_snapshot: std::sync::Mutex<PublishedSnapshot>,
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
            last_snapshot: std::sync::Mutex::new(PublishedSnapshot::default()),
        })
    }

    /// Return the `!Send` MPRIS server run task.
    pub fn run_task(&self) -> mpris_server::LocalServerRunTask {
        self.player.run()
    }

    /// Update the MPRIS player state from a source, emitting D-Bus property
    /// changes only for the groups that actually changed since the last call.
    pub async fn publish(&self, source: Option<&SourceState>) {
        let player = &self.player;
        let next = build_snapshot(source);

        let decision = {
            let prev = self.last_snapshot.lock().unwrap();
            compute_publish_decision(&prev, &next)
        };

        if decision.identity {
            let _ = player.set_identity(&next.identity).await;
        }
        if decision.status {
            let status = next
                .status
                .map(to_mpris_status)
                .unwrap_or(mpris_server::PlaybackStatus::Stopped);
            let _ = player.set_playback_status(status).await;
        }
        if decision.metadata {
            let _ = player.set_metadata(build_metadata(source)).await;
        }
        if decision.caps {
            let _ = player.set_can_play(next.caps.can_play_pause).await;
            let _ = player.set_can_pause(next.caps.can_play_pause).await;
            let _ = player.set_can_go_next(next.caps.can_next).await;
            let _ = player.set_can_go_previous(next.caps.can_previous).await;
            let _ = player.set_can_seek(next.caps.can_seek).await;
        }

        // `Position` is signal-exempt by the MPRIS spec; `set_position` is sync
        // and emits no D-Bus signal. Set it unconditionally.
        let position_us = source
            .map(|s| {
                s.playback
                    .position_ms
                    .saturating_mul(1_000)
                    .min(i64::MAX as u64) as i64
            })
            .unwrap_or(0);
        player.set_position(Time::from_micros(position_us));

        if decision.any() {
            debug!(
                "MPRIS player {bus} emitted {decision:?}",
                bus = self.bus_name
            );
        }

        *self.last_snapshot.lock().unwrap() = next;
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

fn to_mpris_status(status: Status) -> mpris_server::PlaybackStatus {
    match status {
        Status::Playing => mpris_server::PlaybackStatus::Playing,
        Status::Paused => mpris_server::PlaybackStatus::Paused,
        Status::Stopped => mpris_server::PlaybackStatus::Stopped,
    }
}

/// Build the desired `PublishedSnapshot` from a source (or default when absent).
fn build_snapshot(source: Option<&SourceState>) -> PublishedSnapshot {
    let Some(s) = source else {
        return PublishedSnapshot::default();
    };
    let meta = &s.metadata;
    let length_us = {
        let d = s.playback.duration_ms;
        if d > 0 && d < 86_400_000 { (d * 1000) as i64 } else { 0 }
    };
    let art_url = meta
        .art_url
        .clone()
        .filter(|u| u.starts_with("http://") || u.starts_with("https://"))
        .unwrap_or_default();
    PublishedSnapshot {
        identity: format_site_name(&s.site),
        status: Some(s.playback.status),
        meta: MetaSnapshot {
            track_id: make_track_id(s, meta),
            title: meta.title.clone().unwrap_or_default(),
            artists: meta.artist.clone(),
            album: meta.album.clone().unwrap_or_default(),
            album_artists: meta.album_artist.clone(),
            art_url,
            length_us,
            url: select_best_url(s),
        },
        caps: CapsSnapshot {
            can_play_pause: s.capabilities.play_pause,
            can_next: s.capabilities.next,
            can_previous: s.capabilities.previous,
            can_seek: s.capabilities.seek || s.capabilities.set_position,
        },
    }
}

fn build_metadata(source: Option<&SourceState>) -> Metadata {
    let mut builder = Metadata::builder();

    // Always include the bridge marker so mprisence can detect bridge players.
    builder = builder.other("mprisence:bridge", "true");

    if let Some(s) = source {
        let meta = &s.metadata;

        // ── Custom mprisence metadata (stable keys only) ───────
        builder = builder.other("mprisence:sourceId", s.source_id.clone());
        builder = builder.other("mprisence:site", s.site.clone());
        builder = builder.other("mprisence:origin", s.origin.clone());
        builder = builder.other("mprisence:pageUrl", s.url.clone());
        if let Some(ref cu) = s.canonical_url {
            if !cu.is_empty() && !cu.starts_with("blob:") {
                builder = builder.other("mprisence:canonicalUrl", cu.clone());
            }
        }
        // Browser is the first ':'-segment of the source_id (e.g. "firefox:tab:12:0").
        let browser = s.source_id.split(':').next().unwrap_or("unknown").to_string();
        builder = builder.other("mprisence:browser", browser);

        // ── Standard MPRIS metadata ────────────────────────────
        let track_id_str = make_track_id(s, meta);
        if let Ok(path) = ObjectPath::try_from(track_id_str.as_str()) {
            builder = builder.trackid(path);
        }

        if let Some(title) = &meta.title {
            if !title.trim().is_empty() {
                builder = builder.title(title);
            }
        }
        if let Some(album) = &meta.album {
            if !album.trim().is_empty() {
                builder = builder.album(album);
            }
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
            // Only publish art URL if it's an http/https URL mprisence can actually fetch.
            if art_url.starts_with("http://") || art_url.starts_with("https://") {
                builder = builder.art_url(art_url);
            }
        }

        // Length: only publish when finite and > 0 (avoid garbage like 0 or NaN).
        let dur_ms = s.playback.duration_ms;
        if dur_ms > 0 && dur_ms < 86_400_000 {
            // cap at 24h to avoid overflow
            let length_us = dur_ms * 1000;
            builder = builder.length(Time::from_micros(length_us as i64));
        }

        // URL: prefer canonical URL, then page URL, but never blob:
        let best_url = select_best_url(s);
        if !best_url.is_empty() && !best_url.starts_with("blob:") {
            builder = builder.url(&best_url);
        }
    }

    builder.build()
}

/// Pick the best URL for `xesam:url`:
/// 1. Provider canonical URL (track page, not mini-player)
/// 2. Page URL (if not blob:)
fn select_best_url(s: &SourceState) -> String {
    if let Some(ref cu) = s.canonical_url {
        if !cu.is_empty() && !cu.starts_with("blob:") {
            return cu.clone();
        }
    }
    if !s.url.starts_with("blob:") {
        return s.url.clone();
    }
    s.origin.clone()
}

fn make_track_id(s: &SourceState, meta: &MediaMetadata) -> String {
    // 1. Provider track_id (most stable)
    if let Some(ref tid) = meta.track_id {
        if !tid.is_empty() {
            if tid.starts_with('/') {
                return tid.clone();
            }
            return format!("/mprisence/track/{tid}");
        }
    }
    // 2. Canonical URL (stable across page navigations)
    if let Some(ref cu) = s.canonical_url {
        if !cu.is_empty() {
            return format!("/mprisence/track/{}", simple_hash(cu));
        }
    }
    // 3. Page URL (less stable, but better than title-only)
    if !s.url.is_empty() && !s.url.starts_with("blob:") {
        return format!("/mprisence/track/{}", simple_hash(&s.url));
    }
    // 4. Fallback: source_id + title hash
    let hash_input = format!("{}+{}", s.source_id, meta.title.as_deref().unwrap_or(""));
    format!("/mprisence/track/{}", simple_hash(&hash_input))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap() -> PublishedSnapshot {
        PublishedSnapshot {
            identity: "YouTube".into(),
            status: Some(Status::Playing),
            meta: MetaSnapshot {
                track_id: "/mprisence/track/abc".into(),
                title: "Song".into(),
                artists: vec!["Artist".into()],
                album: "Album".into(),
                album_artists: vec![],
                art_url: "https://x/a.jpg".into(),
                length_us: 200_000_000,
                url: "https://x/watch?v=abc".into(),
            },
            caps: CapsSnapshot {
                can_play_pause: true,
                can_next: false,
                can_previous: false,
                can_seek: false,
            },
        }
    }

    #[test]
    fn identical_snapshots_decide_nothing() {
        let d = compute_publish_decision(&snap(), &snap());
        assert!(!d.any());
    }

    #[test]
    fn status_change_decides_status_only() {
        let mut next = snap();
        next.status = Some(Status::Paused);
        let d = compute_publish_decision(&snap(), &next);
        assert_eq!(d, PublishDecision { status: true, ..Default::default() });
    }

    #[test]
    fn title_change_decides_metadata_only() {
        let mut next = snap();
        next.meta.title = "Different".into();
        let d = compute_publish_decision(&snap(), &next);
        assert_eq!(d, PublishDecision { metadata: true, ..Default::default() });
    }

    #[test]
    fn caps_change_decides_caps_only() {
        let mut next = snap();
        next.caps.can_next = true;
        let d = compute_publish_decision(&snap(), &next);
        assert_eq!(d, PublishDecision { caps: true, ..Default::default() });
    }

    #[test]
    fn identity_change_decides_identity_only() {
        let mut next = snap();
        next.identity = "SoundCloud".into();
        let d = compute_publish_decision(&snap(), &next);
        assert_eq!(d, PublishDecision { identity: true, ..Default::default() });
    }
}
