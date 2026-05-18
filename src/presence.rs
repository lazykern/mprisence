use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use discord_rich_presence::{
    activity::{Activity, Assets, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use log::{debug, error, info, trace, warn};
use mime_guess::mime;
use mpris::{Event as MprisEvent, Metadata as MprisMetadata, PlaybackStatus, Player};
use parking_lot::Mutex;
use smol_str::SmolStr;
use tokio::sync::{mpsc, Notify};
use url::Url;

use lofty::file::AudioFile as _;
use lofty::prelude::TaggedFileExt as _;

use crate::{
    config::{
        schema::{
            ActivityType, ActivityTypesConfig, PlayerConfig, StatusDisplayType,
            DEFAULT_PLAYER_APP_ID,
        },
        ConfigManager,
    },
    cover::CoverManager,
    error::DiscordError,
    metadata,
    player::{
        canonical_player_bus_name, cmus,
        events::{self, EventOutcome, PlayerEvent, PlayerEventKind},
        PlaybackState, PlayerIdentifier,
    },
    template::TemplateManager,
    utils,
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct TrackFingerprint {
    track_id: Option<String>,
    url: Option<String>,
    title: Option<String>,
    artists: Vec<String>,
    length: Option<Duration>,
}

impl TrackFingerprint {
    fn from_mpris(metadata: &MprisMetadata) -> Self {
        Self {
            track_id: metadata.track_id().map(|id| id.to_string()),
            url: metadata.url().map(|url| url.to_string()),
            title: metadata.title().map(|title| title.to_string()),
            artists: metadata
                .artists()
                .map(|artists| artists.iter().map(|artist| artist.to_string()).collect())
                .unwrap_or_default(),
            length: metadata.length(),
        }
    }
}

#[derive(Debug, Clone)]
struct UpdateSnapshot {
    playback_status: PlaybackStatus,
    track: TrackFingerprint,
}

impl UpdateSnapshot {
    fn from_mpris(playback_status: PlaybackStatus, metadata: &MprisMetadata) -> Self {
        Self {
            playback_status,
            track: TrackFingerprint::from_mpris(metadata),
        }
    }
}

fn resolve_status_display_type(player_config: &PlayerConfig) -> StatusDisplayType {
    if player_config.app_id == DEFAULT_PLAYER_APP_ID
        && player_config.status_display_type == StatusDisplayType::Name
    {
        StatusDisplayType::State
    } else {
        player_config.status_display_type
    }
}

fn summarize_log_value(key: &str, value: &dyn std::fmt::Debug) -> String {
    const MAX_LOG_VALUE_CHARS: usize = 240;

    let rendered = format!("{:?}", value);
    let looks_like_embedded_art = key.eq_ignore_ascii_case("mpris:artUrl")
        && (rendered.starts_with("\"data:")
            || rendered.contains(";base64,")
            || rendered.len() > MAX_LOG_VALUE_CHARS);

    if looks_like_embedded_art || rendered.len() > MAX_LOG_VALUE_CHARS {
        let truncated: String = rendered.chars().take(MAX_LOG_VALUE_CHARS).collect();
        format!(
            "{}… [truncated, {} chars total]",
            truncated,
            rendered.chars().count()
        )
    } else {
        rendered
    }
}

pub struct Presence {
    player: Player,
    /// Cached identifier for the currently active player connection.
    /// Updated whenever the underlying bus name or unique connection changes.
    player_id: PlayerIdentifier,
    template_manager: Arc<TemplateManager>,
    cover_manager: Arc<CoverManager>,
    last_player_state: Option<PlaybackState>,
    last_cmus_track_id: Mutex<Option<Box<str>>>,
    last_cmus_path: Mutex<Option<PathBuf>>,
    cmus_error_logged: AtomicBool,
    discord_client: Option<Arc<Mutex<DiscordIpcClient>>>,
    /// The Discord application id the current `discord_client` was opened with.
    /// Each cycle re-resolves the effective app id (player + website overlay).
    /// A mismatch triggers IPC client recycling so the new app's icon/name
    /// takes effect.
    last_effective_app_id: Mutex<Option<String>>,
    needs_initial_connection: AtomicBool,
    needs_reconnection: AtomicBool,
    error_logged: AtomicBool,
    last_reconnect_attempt: Mutex<Instant>,
    /// Monotonically increasing counter, incremented on every TrackChanged event
    /// (event-driven mode) AND on every polling-mode track change detected inside
    /// `update_activity`. Background cover-art tasks capture this value at spawn
    /// time and abort if it has changed when their fetch completes — that's what
    /// prevents a slow cover from overwriting a newer track's activity.
    update_generation: Arc<AtomicU64>,
    update_notify: Arc<Notify>,
    /// Fingerprint of the last track for which `update_activity` ran its push
    /// path. Used in polling mode to detect track changes (which then bumps
    /// `update_generation` so in-flight cover fetches for the old track can
    /// self-cancel).
    last_pushed_track: Mutex<Option<TrackFingerprint>>,
    /// Guard to prevent spawning multiple simultaneous background cover-art
    /// fetch tasks for the same track. Set to `true` before spawning; the task
    /// resets it to `false` on completion (success or failure).
    cover_fetch_in_flight: Arc<AtomicBool>,
    config: Arc<ConfigManager>,
    /// Cancellation flag for the per-player event listener thread (event-driven mode only).
    listener_cancel: Option<Arc<AtomicBool>>,
    /// Handle to the listener thread; kept so future code can join, but currently detached on drop.
    #[allow(dead_code)]
    listener_handle: Option<JoinHandle<()>>,
    /// The MPRIS bus name the active listener is bound to (used to detect winner-bus handoff).
    listener_bus: Option<SmolStr>,
}

impl Presence {
    pub fn new(
        player: Player,
        template_manager: Arc<TemplateManager>,
        cover_manager: Arc<CoverManager>,
        config: Arc<ConfigManager>,
    ) -> Self {
        let player_bus_name = canonical_player_bus_name(player.bus_name());
        let player_config = config.get_player_config(player.identity(), &player_bus_name);
        info!("Initializing presence for player: {}", player.identity());
        trace!("Using Discord application ID: {}", player_config.app_id);
        trace!("Player configuration: {:#?}", player_config);
        let player_id = PlayerIdentifier::from(&player);
        Self {
            player,
            player_id,
            template_manager,
            cover_manager,
            last_player_state: None,
            last_cmus_track_id: Mutex::new(None),
            last_cmus_path: Mutex::new(None),
            cmus_error_logged: AtomicBool::new(false),
            discord_client: None,
            last_effective_app_id: Mutex::new(None),
            needs_initial_connection: AtomicBool::new(true),
            needs_reconnection: AtomicBool::new(false),
            error_logged: AtomicBool::new(false),
            last_reconnect_attempt: Mutex::new(Instant::now()),
            update_generation: Arc::new(AtomicU64::new(0)),
            update_notify: Arc::new(Notify::new()),
            last_pushed_track: Mutex::new(None),
            cover_fetch_in_flight: Arc::new(AtomicBool::new(false)),
            config,
            listener_cancel: None,
            listener_handle: None,
            listener_bus: None,
        }
    }

    /// Returns the identifier of the currently tracked player connection.
    pub fn player_id(&self) -> &PlayerIdentifier {
        &self.player_id
    }

    /// Returns the current track's URL (xesam:url) if available.
    pub fn current_url(&self) -> Option<String> {
        self.player
            .get_metadata()
            .ok()
            .and_then(|m| m.url().map(|s| s.to_string()))
    }

    pub fn current_title(&self) -> Option<String> {
        self.player
            .get_metadata()
            .ok()
            .and_then(|m| m.title().map(|s| s.to_string()))
    }

    pub fn initialize_discord_client(&mut self) -> Result<(), DiscordError> {
        if self.discord_client.is_some() {
            return Ok(());
        }
        let player_config = self.config.get_player_config(
            self.player.identity(),
            &canonical_player_bus_name(self.player.bus_name()),
        );
        self.initialize_discord_client_with_app_id(&player_config.app_id)
    }

    fn initialize_discord_client_with_app_id(
        &mut self,
        app_id: &str,
    ) -> Result<(), DiscordError> {
        let client = DiscordIpcClient::new(app_id);
        self.discord_client = Some(Arc::new(Mutex::new(client)));
        self.needs_initial_connection.store(true, Ordering::Relaxed);
        *self.last_effective_app_id.lock() = Some(app_id.to_string());
        Ok(())
    }

    pub fn destroy_discord_client(&mut self) -> Result<(), DiscordError> {
        if let Some(client) = &self.discord_client {
            debug!(
                "Closing Discord connection for player: {}",
                self.player.identity()
            );
            {
                let mut discord_client = client.lock();
                if let Err(err) = discord_client.clear_activity() {
                    debug!(
                        "Failed to clear Discord activity before closing connection: {}",
                        err
                    );
                }

                discord_client.close().map_err(|err| {
                    error!("Failed to close Discord connection: {}", err);
                    DiscordError::CloseError(err.to_string())
                })?;
            }
            trace!("Discord connection closed successfully");
            self.discord_client = None;
            *self.last_effective_app_id.lock() = None;
        }
        Ok(())
    }

    pub async fn update(&mut self, player: Player) -> Result<(), DiscordError> {
        trace!("Updating presence for player: {}", player.identity());

        // Validate identity — only the identity must match; bus name and unique
        // connection name are allowed to change (e.g. playerctld handoff, player
        // restart, or deduplication winner switch).
        if player.identity() != self.player.identity() {
            error!(
                "Player identity mismatch. Expected: {}, got: {}",
                self.player.identity(),
                player.identity()
            );
            return Err(DiscordError::InvalidPlayer(format!(
                "Expected {}, got {}",
                self.player.identity(),
                player.identity()
            )));
        }

        // If the bus name or unique D-Bus connection changed, update the stored
        // reference and reset playback state so the next cycle does a full update.
        if player.bus_name() != self.player.bus_name()
            || player.unique_name() != self.player.unique_name()
        {
            let new_id = PlayerIdentifier::from(&player);
            debug!(
                "Player '{}' connection changed: {}:{} -> {}:{}",
                self.player.identity(),
                self.player_id.player_bus_name,
                self.player_id.unique_name,
                new_id.player_bus_name,
                new_id.unique_name,
            );
            self.player_id = new_id;
            self.player = player;
            self.last_player_state = None;
            *self.last_cmus_track_id.lock() = None;
            *self.last_cmus_path.lock() = None;
            self.cmus_error_logged.store(false, Ordering::Relaxed);
            // Skip Discord update this cycle; full update happens next poll.
            return Ok(());
        }

        let Some(_discord_client) = &self.discord_client else {
            return Ok(());
        };

        self.ensure_connection()?;

        let start_time = Instant::now();
        let new_state = PlaybackState::from(&player);
        let dbus_delay = start_time.elapsed();
        trace!("D-Bus interaction took: {:?}", dbus_delay);

        // In event-driven mode the fallback poll fires at `fallback_poll_interval`
        // (default 5 s), not `interval` (default 2 s). Using the wrong value
        // causes every tick to be detected as a position jump.
        let effective_interval = if self.config.event_driven() {
            self.config.fallback_poll_interval()
        } else {
            self.config.interval()
        };

        let should_update = self
            .last_player_state
            .as_ref()
            .map(|previous_state| {
                new_state.has_significant_changes(previous_state)
                    || new_state.has_position_jump(
                        previous_state,
                        Duration::from_millis(effective_interval),
                        dbus_delay,
                    )
            })
            .unwrap_or(true);

        if !should_update {
            trace!("Skipping update - no significant changes detected");
            self.last_player_state = Some(new_state);
            return Ok(());
        }

        trace!("Updating Discord presence");
        self.last_player_state = Some(new_state);
        self.update_activity(None).await.map_err(|err| {
            if matches!(err, DiscordError::ActivityError(_)) {
                if !self.error_logged.load(Ordering::Relaxed) {
                    warn!("Discord connection error, will attempt to reconnect next update");
                    self.error_logged.store(true, Ordering::Relaxed);
                }
                self.last_player_state = None;
                self.needs_reconnection.store(true, Ordering::Relaxed);
            }
            err
        })
    }

    /// Push current player state to Discord without consuming the player.
    /// Used on initial discovery in event-driven mode — signals only fire on changes,
    /// so the current state must be pushed once when a new player is first seen.
    pub async fn update_from_current_state(&mut self) -> Result<(), DiscordError> {
        let Some(_) = &self.discord_client else {
            return Ok(());
        };
        self.ensure_connection()?;
        // Seed `last_player_state` so the next polling tick's diff sees no change
        // and skips re-pushing (and re-fetching cover art) for the same track.
        self.last_player_state = Some(PlaybackState::from(&self.player));
        self.update_activity(None).await
    }

    fn ensure_connection(&mut self) -> Result<(), DiscordError> {
        const MIN_RECONNECT_INTERVAL: Duration = Duration::from_secs(10);

        let Some(discord_client) = &self.discord_client else {
            return Ok(());
        };

        if self.needs_initial_connection.load(Ordering::Relaxed) {
            debug!("Establishing initial Discord connection");
            discord_client.lock().connect().map_err(|err| {
                if !self.error_logged.load(Ordering::Relaxed) {
                    error!("Failed to establish Discord connection: {}", err);
                    self.error_logged.store(true, Ordering::Relaxed);
                }
                *self.last_reconnect_attempt.lock() = Instant::now();
                DiscordError::ConnectionError(err.to_string())
            })?;
            debug!("Discord connection established successfully");
            self.needs_initial_connection
                .store(false, Ordering::Relaxed);
            self.error_logged.store(false, Ordering::Relaxed);
        }

        if self.needs_reconnection.load(Ordering::Relaxed) {
            let now = Instant::now();
            let last_attempt = *self.last_reconnect_attempt.lock();

            if now.duration_since(last_attempt) < MIN_RECONNECT_INTERVAL {
                return Ok(());
            }

            if !self.error_logged.load(Ordering::Relaxed) {
                debug!("Attempting to reconnect to Discord");
            }

            *self.last_reconnect_attempt.lock() = now;

            discord_client.lock().reconnect().map_err(|err| {
                if !self.error_logged.load(Ordering::Relaxed) {
                    error!("Failed to reconnect to Discord: {}", err);
                    self.error_logged.store(true, Ordering::Relaxed);
                }
                DiscordError::ReconnectionError(err.to_string())
            })?;
            debug!("Discord reconnection successful");
            self.needs_reconnection.store(false, Ordering::Relaxed);
            self.error_logged.store(false, Ordering::Relaxed);
            self.last_player_state = None;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn force_reconnect(&mut self) {
        debug!(
            "Forcing Discord reconnection for player: {}",
            self.player.identity()
        );
        self.needs_reconnection.store(true, Ordering::Relaxed);
        self.last_player_state = None;
    }

    fn determine_activity_type(
        &self,
        activity_type_config: &ActivityTypesConfig,
        player_config: &PlayerConfig,
        url: Option<&str>,
    ) -> ActivityType {
        trace!(
            "Determining activity type for player: {}",
            self.player.identity()
        );

        if let Some(override_type) = player_config.override_activity_type {
            debug!("Using overridden activity type: {:?}", override_type);
            return override_type;
        }

        if activity_type_config.use_content_type && url.is_some() {
            trace!("Attempting to determine activity type from content type");
            if let Some(content_type) = url.and_then(utils::get_content_type_from_metadata) {
                match content_type.type_() {
                    mime::AUDIO => {
                        debug!("Content type is audio, using Listening activity type");
                        return ActivityType::Listening;
                    }
                    mime::VIDEO | mime::IMAGE => {
                        debug!("Content type is video/image, using Watching activity type");
                        return ActivityType::Watching;
                    }
                    _ => {
                        trace!("Unrecognized content type, falling back to default");
                    }
                }
            }
        }

        debug!(
            "Using default activity type: {:?}",
            activity_type_config.default
        );
        activity_type_config.default
    }

    fn generation_matches(&self, generation: Option<u64>) -> bool {
        generation
            .map(|gen| self.update_generation.load(Ordering::Relaxed) == gen)
            .unwrap_or(true)
    }

    fn update_snapshot_matches_current(
        &self,
        player: &Player,
        expected: &UpdateSnapshot,
        checkpoint: &str,
    ) -> bool {
        let current_status = match player.get_playback_status() {
            Ok(status) => status,
            Err(err) => {
                warn!(
                    "{}: failed to re-check playback status; discarding stale-safe update: {}",
                    checkpoint, err
                );
                return false;
            }
        };

        if current_status != expected.playback_status {
            trace!(
                "{}: playback status changed from {:?} to {:?}; discarding stale update",
                checkpoint,
                expected.playback_status,
                current_status
            );
            return false;
        }

        let current_metadata = match player.get_metadata() {
            Ok(metadata) => metadata,
            Err(err) => {
                warn!(
                    "{}: failed to re-check metadata; discarding stale-safe update: {}",
                    checkpoint, err
                );
                return false;
            }
        };
        let current_track = TrackFingerprint::from_mpris(&current_metadata);

        if current_track != expected.track {
            trace!(
                "{}: track changed while building Discord activity; discarding stale update. expected={:?}, current={:?}",
                checkpoint,
                expected.track,
                current_track
            );
            return false;
        }

        true
    }

    fn should_discard_stale_update(
        &self,
        player: &Player,
        generation: Option<u64>,
        snapshot: &UpdateSnapshot,
        checkpoint: &str,
    ) -> bool {
        if !self.generation_matches(generation) {
            trace!(
                "{}: generation mismatch; discarding stale update",
                checkpoint
            );
            return true;
        }

        !self.update_snapshot_matches_current(player, snapshot, checkpoint)
    }

    async fn update_activity(
        &mut self,
        generation: Option<u64>,
    ) -> Result<(), DiscordError> {
        if self.discord_client.is_none() {
            return Ok(());
        }

        let playback_status = self.player.get_playback_status().map_err(|err| {
            error!("Failed to get playback status: {}", err);
            DiscordError::ActivityError(format!("Failed to get playback status: {}", err))
        })?;

        if playback_status == PlaybackStatus::Stopped || playback_status == PlaybackStatus::Paused {
            if !self.error_logged.load(Ordering::Relaxed) {
                info!(
                    "Clearing Discord activity - player {} is {}",
                    self.player.identity(),
                    if playback_status == PlaybackStatus::Stopped {
                        "stopped"
                    } else {
                        "paused"
                    }
                );
            }
            if let Some(discord_client) = self.discord_client.clone() {
                discord_client.lock().clear_activity().map_err(|err| {
                    if !self.error_logged.load(Ordering::Relaxed) {
                        error!("Failed to clear Discord activity: {}", err);
                        self.error_logged.store(true, Ordering::Relaxed);
                    }
                    DiscordError::ActivityError(err.to_string())
                })?;
            }
            return Ok(());
        }

        trace!(
            "Building Discord activity for player: {}",
            self.player.identity()
        );
        let metadata = match self.player.get_metadata() {
            Ok(metadata) => metadata,
            Err(e) => {
                warn!("Failed to get metadata for player: {}", e);
                return Ok(());
            }
        };
        let update_snapshot = UpdateSnapshot::from_mpris(playback_status.clone(), &metadata);
        trace!("Metadata: {:?}", metadata);

        // Detect a track change relative to the last push and bump the
        // generation counter so any in-flight cover-art task spawned for the
        // previous track aborts before re-pushing its (now stale) result.
        // Event-driven mode already bumps from the listener thread; this path
        // makes the same guarantee hold in polling mode.
        let track_changed = {
            let mut last = self.last_pushed_track.lock();
            let changed = last.as_ref() != Some(&update_snapshot.track);
            if changed {
                *last = Some(update_snapshot.track.clone());
            }
            changed
        };
        if track_changed {
            self.update_generation.fetch_add(1, Ordering::Relaxed);
            self.update_notify.notify_waiters();
            // Allow the new track to spawn its own cover-art background task.
            self.cover_fetch_in_flight.store(false, Ordering::Release);
        }

        let player_bus_name = canonical_player_bus_name(self.player.bus_name());
        let is_cmus =
            player_bus_name == "cmus" || self.player.identity().eq_ignore_ascii_case("cmus");
        let cmus_override_url = if is_cmus {
            let track_token = metadata
                .track_id()
                .map(|id| id.to_string())
                .or_else(|| metadata.url().map(|url| url.to_string()))
                .or_else(|| metadata.title().map(|title| title.to_string()));
            let track_changed = {
                let guard = self.last_cmus_track_id.lock();
                track_token.as_deref() != guard.as_deref()
            };

            if track_changed {
                *self.last_cmus_track_id.lock() = track_token.map(|token| token.into_boxed_str());
                *self.last_cmus_path.lock() = None;
                self.cmus_error_logged.store(false, Ordering::Relaxed);
            }

            if self.last_cmus_path.lock().is_none() {
                match cmus::get_current_track_path().await {
                    Ok(Some(path)) => {
                        *self.last_cmus_path.lock() = Some(path);
                    }
                    Ok(None) => {}
                    Err(err) => {
                        if !self.cmus_error_logged.load(Ordering::Relaxed) {
                            warn!("cmus-remote failed: {}", err);
                            self.cmus_error_logged.store(true, Ordering::Relaxed);
                        }
                    }
                }
            }

            let cmus_path = self.last_cmus_path.lock().clone();
            if let Some(path) = cmus_path {
                match Url::from_file_path(&path) {
                    Ok(url) => Some(url.to_string()),
                    Err(_) => {
                        if !self.cmus_error_logged.load(Ordering::Relaxed) {
                            warn!("cmus-remote returned non-file path: {:?}", path);
                            self.cmus_error_logged.store(true, Ordering::Relaxed);
                        }
                        None
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        let metadata_source = match cmus_override_url {
            Some(url) => {
                metadata::MetadataSource::from_mpris_with_override(metadata.clone(), Some(url))
            }
            None => metadata::MetadataSource::from_mpris(metadata.clone()),
        };

        debug!("--- Raw Metadata Start ---");
        if let Some(mpris_meta) = metadata_source.mpris_metadata() {
            debug!("MPRIS Metadata Map:");
            for (key, value) in mpris_meta.iter() {
                debug!(
                    "  MPRIS Key: '{}', Value: {}",
                    key,
                    summarize_log_value(key, value)
                );
            }
        } else {
            debug!("No MPRIS Metadata available in source.");
        }
        if let Some(lofty_tag) = metadata_source.lofty_tag() {
            debug!("Lofty Primary Tag ({:?}):", lofty_tag.file_type());
            if let Some(tag) = lofty_tag.primary_tag() {
                for item in tag.items() {
                    debug!("  Lofty Key: {:?}, Value: {:?}", item.key(), item.value());
                }
            } else {
                debug!("  No primary tag found by Lofty.");
            }
            debug!("Lofty Properties: {:?}", lofty_tag.properties());
        } else {
            debug!(
                "No Lofty TaggedFile available in source (likely not a local file or read failed)."
            );
        }
        debug!("--- Raw Metadata End ---");

        let mut media_metadata = metadata_source.to_media_metadata();
        let track_url: Option<String> = metadata_source.url();
        let track_url_ref = track_url.as_deref();

        let (player_config, title_suffix) = self.config.get_player_config_with_title_fallback(
            self.player.identity(),
            &player_bus_name,
            track_url_ref,
            media_metadata.title.as_deref(),
        );

        // Strip the matched title suffix (e.g. " | YouTube Music") from the
        // displayed title so Discord shows only the track name.
        if let Some(ref suffix) = title_suffix {
            if let Some(ref mut title) = media_metadata.title {
                if let Some(stripped) = title.strip_suffix(suffix.as_str()) {
                    *title = stripped.trim_end().to_string();
                }
            }
        }

        // Reconcile the Discord IPC client when the resolved app id changes
        // (e.g. the active [website.*] overlay matched a different service).
        let new_app_id = player_config.app_id.clone();
        let needs_app_swap = {
            let last = self.last_effective_app_id.lock();
            last.as_deref() != Some(new_app_id.as_str())
        };
        if needs_app_swap {
            debug!(
                "Effective Discord app id changed to {} (recycling IPC client)",
                new_app_id
            );
            self.destroy_discord_client()?;
            self.initialize_discord_client_with_app_id(&new_app_id)?;
            self.ensure_connection()?;
        }

        let Some(discord_client) = self.discord_client.clone() else {
            return Ok(());
        };

        if !player_config.allow_streaming && track_url_ref.is_some_and(utils::is_streaming_url) {
            info!(
                "Skipping Discord activity - streaming source blocked for player {}",
                self.player.identity()
            );
            discord_client.lock().clear_activity().map_err(|err| {
                if !self.error_logged.load(Ordering::Relaxed) {
                    error!("Failed to clear Discord activity: {}", err);
                    self.error_logged.store(true, Ordering::Relaxed);
                }
                DiscordError::ActivityError(err.to_string())
            })?;
            return Ok(());
        }
        let as_elapsed = self.config.time_config().as_elapsed;

        let (start_s, end_s) = if playback_status == PlaybackStatus::Playing {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards");

            let position = self.player.get_position().unwrap_or_default();
            trace!("Player position: {:?}", position);
            let start_dur = now.checked_sub(position).unwrap_or_default();
            trace!("Start duration: {:?}", start_dur);
            let start_s = Some(start_dur.as_secs());
            trace!("Start seconds: {:?}", start_s);

            let length = metadata.length().unwrap_or_default();
            trace!("Length: {:?}", length);
            let end_s = if !as_elapsed && !length.is_zero() {
                start_dur.checked_add(length).map(|end| {
                    trace!("End duration: {:?}", end); // Fix: Log the end duration
                    end.as_secs()
                })
            } else {
                None
            };
            trace!("End seconds: {:?}", end_s);

            (start_s, end_s)
        } else {
            (None, None)
        };

        let activity_texts = self
            .template_manager
            .render_activity_texts(&self.player, media_metadata, player_config.name.as_deref())?;

        // Checkpoint 1: abort before any cover lookup if a newer TrackChanged
        // has already superseded this update.
        if !self.generation_matches(generation) {
            trace!("before cover lookup: generation mismatch; discarding stale update");
            return Ok(());
        }

        let activity_type = self.determine_activity_type(
            &self.config.activity_type_config(),
            &player_config,
            track_url_ref,
        );
        let status_display_type = resolve_status_display_type(&player_config);

        // Fast path: try a sync, in-process cache lookup so a cached cover
        // attaches to the very first push. Cache miss → push immediately with
        // the player icon, then fetch the real cover in the background.
        let cached_cover = self.cover_manager.try_cached_cover_art(&metadata_source);
        if let Some(cover_url) = cached_cover.as_deref() {
            debug!("Serving cached cover art on fast path: {}", cover_url);
        } else {
            trace!("No cached cover; pushing placeholder and spawning background fetch");
        }

        // Final checkpoint right before the Discord write so a stale push
        // cannot overwrite a newer song/status.
        if self.should_discard_stale_update(
            &self.player,
            generation,
            &update_snapshot,
            "before Discord update",
        ) {
            return Ok(());
        }

        if !self.error_logged.load(Ordering::Relaxed) {
            debug!("Updating Discord activity");
            info!(
                "Activity payload: details={:?}, state={:?}, large_text={:?}, small_text={:?}, cover_art={:?}, activity_type={:?}",
                activity_texts.details,
                activity_texts.state,
                activity_texts.large_text,
                activity_texts.small_text,
                cached_cover.as_deref(),
                activity_type,
            );
        }
        Self::build_and_push_activity(
            &discord_client,
            &activity_texts,
            start_s,
            end_s,
            cached_cover.as_deref(),
            &player_config,
            activity_type,
            status_display_type,
        )
        .map_err(|err| {
            if !self.error_logged.load(Ordering::Relaxed) {
                error!("Failed to set Discord activity: {}", err);
                self.error_logged.store(true, Ordering::Relaxed);
            }
            err
        })?;
        if !self.error_logged.load(Ordering::Relaxed) {
            info!(
                "Updated Discord activity for {} - {} ({:?})",
                self.player.identity(),
                activity_texts.details,
                playback_status
            );
        }
        self.error_logged.store(false, Ordering::Relaxed);

        // Slow path: cache miss → background cover fetch + second push when ready.
        // Guard: skip if a background task is already in flight for this track
        // (prevents duplicate uploads when position-jump ticks re-enter this path).
        if cached_cover.is_none()
            && !self
                .cover_fetch_in_flight
                .swap(true, Ordering::AcqRel)
        {
            let cover_manager = Arc::clone(&self.cover_manager);
            let discord_client_for_task = Arc::clone(&discord_client);
            let update_generation = Arc::clone(&self.update_generation);
            let update_notify = Arc::clone(&self.update_notify);
            let texts_for_task = activity_texts.clone();
            let player_config_for_task = player_config.clone();
            let identity_for_task = self.player.identity().to_string();
            let metadata_source_for_task = metadata_source;
            let cover_in_flight = Arc::clone(&self.cover_fetch_in_flight);
            // Always use the freshly-loaded generation (post-bump) so this task
            // self-cancels on any subsequent track change in either run mode.
            let spawn_gen = update_generation.load(Ordering::Relaxed);

            tokio::spawn(async move {
                // Ensure the in-flight flag is always reset when the task exits.
                struct InFlightGuard(Arc<AtomicBool>);
                impl Drop for InFlightGuard {
                    fn drop(&mut self) {
                        self.0.store(false, Ordering::Release);
                    }
                }
                let _guard = InFlightGuard(cover_in_flight);
                let art_source = metadata_source_for_task.art_source();
                let cover_art_result = tokio::select! {
                    result = cover_manager.get_cover_art(art_source.clone(), &metadata_source_for_task) => result,
                    _ = update_notify.notified() => {
                        if update_generation.load(Ordering::Relaxed) != spawn_gen {
                            trace!(
                                "background cover fetch cancelled: newer track arrived for {}",
                                identity_for_task
                            );
                            return;
                        }
                        cover_manager
                            .get_cover_art(art_source.clone(), &metadata_source_for_task)
                            .await
                    }
                };

                let cover_url = match cover_art_result {
                    Ok(Some(url)) => url,
                    Ok(None) => {
                        debug!("Background cover fetch produced no art for {}", identity_for_task);
                        return;
                    }
                    Err(err) => {
                        warn!(
                            "Background cover fetch failed for {}: {}",
                            identity_for_task, err
                        );
                        return;
                    }
                };

                if update_generation.load(Ordering::Relaxed) != spawn_gen {
                    trace!(
                        "background cover result discarded: newer track for {}",
                        identity_for_task
                    );
                    return;
                }

                debug!("Found cover art URL for Discord presence: {}", cover_url);
                if let Err(err) = Self::build_and_push_activity(
                    &discord_client_for_task,
                    &texts_for_task,
                    start_s,
                    end_s,
                    Some(cover_url.as_str()),
                    &player_config_for_task,
                    activity_type,
                    status_display_type,
                ) {
                    warn!(
                        "Failed to push cover art update for {}: {}",
                        identity_for_task, err
                    );
                } else {
                    info!(
                        "Updated Discord cover art for {} - {}",
                        identity_for_task, texts_for_task.details
                    );
                }
            });
        }

        Ok(())
    }

    /// Build a Discord `Activity` and push it. Callable from both the fast
    /// path (event handler) and the slow path (background cover fetch task)
    /// because it captures no `&self` state — all inputs are owned or
    /// `Arc`-shared.
    fn build_and_push_activity(
        discord_client: &Arc<Mutex<DiscordIpcClient>>,
        texts: &crate::template::ActivityTexts,
        start_s: Option<u64>,
        end_s: Option<u64>,
        cover_art_url: Option<&str>,
        player_config: &PlayerConfig,
        activity_type: ActivityType,
        status_display_type: StatusDisplayType,
    ) -> Result<(), DiscordError> {
        let mut activity = Activity::default()
            .activity_type(activity_type.into())
            .status_display_type(status_display_type.into());

        if !texts.details.is_empty() {
            activity = activity.details(&texts.details);
        }
        if !texts.state.is_empty() {
            activity = activity.state(&texts.state);
        }

        if let Some(start) = start_s {
            activity = activity.timestamps({
                let ts = Timestamps::default().start(start as i64);
                if let Some(end) = end_s {
                    ts.end(end as i64)
                } else {
                    ts
                }
            });
        }

        let mut assets = Assets::default();
        if let Some(img_url) = cover_art_url {
            assets = assets.large_image(img_url);
            if !texts.large_text.is_empty() {
                assets = assets.large_text(&texts.large_text);
            }
            if player_config.show_icon {
                assets = assets.small_image(player_config.icon.as_str());
                if !texts.small_text.is_empty() {
                    assets = assets.small_text(&texts.small_text);
                }
            }
        } else {
            assets = assets.large_image(player_config.icon.as_str());
            if !texts.large_text.is_empty() {
                assets = assets.large_text(&texts.large_text);
            }
        }
        activity = activity.assets(assets);

        discord_client
            .lock()
            .set_activity(activity)
            .map_err(|err| DiscordError::ActivityError(err.to_string()))?;
        Ok(())
    }

    pub fn update_managers(
        &mut self,
        template_manager: Arc<TemplateManager>,
        cover_manager: Arc<CoverManager>,
        config: Arc<ConfigManager>,
    ) {
        trace!(
            "Updating presence managers for player: {}",
            self.player.identity()
        );
        self.template_manager = template_manager;
        self.cover_manager = cover_manager;
        self.config = config;
        trace!("Presence managers updated successfully");

        self.last_player_state = None;
        *self.last_cmus_track_id.lock() = None;
        *self.last_cmus_path.lock() = None;
        self.cmus_error_logged.store(false, Ordering::Relaxed);
    }

    /// Event-driven mode entry point. Returns `ShouldRemove` when the listener has terminated
    /// and the presence entry should be dropped from the registry.
    pub async fn handle_event(
        &mut self,
        kind: PlayerEventKind,
    ) -> Result<EventOutcome, DiscordError> {
        trace!("handling {:?} for {}", kind, self.player.identity());

        let mut is_track_change = false;
        match kind {
            PlayerEventKind::Mpris(MprisEvent::PlayerShutDown)
            | PlayerEventKind::ListenerExited => {
                debug!(
                    "player {} reported shutdown via event stream",
                    self.player.identity()
                );
                return Ok(EventOutcome::ShouldRemove);
            }
            PlayerEventKind::ListenerError(msg) => {
                warn!("listener error for {}: {}", self.player.identity(), msg);
                return Ok(EventOutcome::Continue);
            }
            PlayerEventKind::Mpris(MprisEvent::VolumeChanged(_))
            | PlayerEventKind::Mpris(MprisEvent::LoopingChanged(_))
            | PlayerEventKind::Mpris(MprisEvent::ShuffleToggled(_))
            | PlayerEventKind::Mpris(MprisEvent::PlaybackRateChanged(_))
            | PlayerEventKind::Mpris(MprisEvent::TrackAdded(_))
            | PlayerEventKind::Mpris(MprisEvent::TrackRemoved(_))
            | PlayerEventKind::Mpris(MprisEvent::TrackMetadataChanged { .. })
            | PlayerEventKind::Mpris(MprisEvent::TrackListReplaced) => {
                trace!("ignoring event variant (no Discord-relevant change)");
                return Ok(EventOutcome::Continue);
            }
            PlayerEventKind::Mpris(MprisEvent::TrackChanged(_)) => {
                // Force the next polling-style diff (if event_driven flips off) to detect a change.
                self.last_player_state = None;
                is_track_change = true;
            }
            PlayerEventKind::Mpris(MprisEvent::Playing)
            | PlayerEventKind::Mpris(MprisEvent::Paused)
            | PlayerEventKind::Mpris(MprisEvent::Stopped)
            | PlayerEventKind::Mpris(MprisEvent::Seeked { .. }) => {
                // Fall through to update.
            }
        }

        // Listener threads increment this before queuing TrackChanged, so an in-flight
        // cover-art lookup can be cancelled even while the main event loop is awaiting it.
        let generation = if is_track_change {
            Some(self.update_generation.load(Ordering::Relaxed))
        } else {
            None
        };

        let Some(_discord_client) = &self.discord_client else {
            return Ok(EventOutcome::Continue);
        };

        self.ensure_connection()?;

        if let Err(err) = self.update_activity(generation).await {
            if matches!(err, DiscordError::ActivityError(_)) {
                if !self.error_logged.load(Ordering::Relaxed) {
                    warn!("Discord connection error, will attempt to reconnect next event");
                    self.error_logged.store(true, Ordering::Relaxed);
                }
                self.last_player_state = None;
                self.needs_reconnection.store(true, Ordering::Relaxed);
            }
            return Err(err);
        }

        Ok(EventOutcome::Continue)
    }

    /// Spawn a listener if none exists, or replace the existing one when the underlying
    /// player bus name has changed (e.g. playerctld handoff or player restart).
    pub fn ensure_listener(&mut self, tx: mpsc::Sender<PlayerEvent>, norm_id: SmolStr) {
        let current_bus = SmolStr::new(self.player.bus_name());
        if let Some(existing) = &self.listener_bus {
            if existing == &current_bus {
                return;
            }
            debug!(
                "listener bus changed for {}: {} -> {}; restarting",
                norm_id, existing, current_bus
            );
            self.stop_listener();
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let handle = events::spawn_listener(
            current_bus.clone(),
            norm_id,
            tx,
            cancel.clone(),
            self.update_generation.clone(),
            self.update_notify.clone(),
        );
        self.listener_cancel = Some(cancel);
        self.listener_handle = Some(handle);
        self.listener_bus = Some(current_bus);
    }

    /// Cancel the listener thread (detached; thread exits on the next event or D-Bus tick).
    pub fn stop_listener(&mut self) {
        if let Some(cancel) = self.listener_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        // Drop the JoinHandle without joining — the blocking D-Bus call cannot be interrupted
        // synchronously, so the thread is left to exit on its own when the next event arrives
        // or the player disappears.
        self.listener_handle.take();
        self.listener_bus = None;
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        self.stop_listener();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player_config_with(app_id: &str, status_display_type: StatusDisplayType) -> PlayerConfig {
        PlayerConfig {
            app_id: app_id.to_string(),
            status_display_type,
            ..PlayerConfig::default()
        }
    }

    #[test]
    fn default_app_name_status_falls_back_to_state() {
        let config = player_config_with(DEFAULT_PLAYER_APP_ID, StatusDisplayType::Name);

        assert_eq!(
            resolve_status_display_type(&config),
            StatusDisplayType::State
        );
    }

    #[test]
    fn custom_app_keeps_name_status() {
        let config = player_config_with("123456789012345678", StatusDisplayType::Name);

        assert_eq!(
            resolve_status_display_type(&config),
            StatusDisplayType::Name
        );
    }

    #[test]
    fn explicit_non_name_status_is_unchanged() {
        let default_app_state = player_config_with(DEFAULT_PLAYER_APP_ID, StatusDisplayType::State);
        let default_app_details =
            player_config_with(DEFAULT_PLAYER_APP_ID, StatusDisplayType::Details);

        assert_eq!(
            resolve_status_display_type(&default_app_state),
            StatusDisplayType::State
        );
        assert_eq!(
            resolve_status_display_type(&default_app_details),
            StatusDisplayType::Details
        );
    }
}
