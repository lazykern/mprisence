use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc,
    },
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
        canonical_player_bus_name, cmus, health,
        events::{self, EventOutcome, PlayerEvent, PlayerEventKind},
        PlaybackState, PlayerIdentifier,
    },
    template::TemplateManager,
    utils,
};

use health::TrackFingerprint;

/// Bundled inputs to `Presence::build_and_push_activity`. Borrows everywhere
/// except the small `Copy` enums to avoid clones on the hot path.
struct ActivityFraming<'a> {
    texts: &'a crate::template::ActivityTexts,
    /// `(start_s, Option<end_s>)`; absent when no timestamp should be sent.
    timing: Option<(u64, Option<u64>)>,
    cover_art_url: Option<&'a str>,
    player_config: &'a PlayerConfig,
    activity_type: ActivityType,
    status_display_type: StatusDisplayType,
}

#[derive(Debug, Clone)]
struct UpdateSnapshot {
    playback_status: PlaybackStatus,
    track: health::TrackFingerprint,
}

impl UpdateSnapshot {
    fn from_mpris(playback_status: PlaybackStatus, metadata: &MprisMetadata) -> Self {
        Self {
            playback_status,
            track: health::TrackFingerprint::from_mpris(metadata),
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
    /// Unified staleness state machine. Replaces the 6 scattered fields that
    /// previously tracked stale YouTube art, stalled position, startup confirmation, etc.
    health: parking_lot::Mutex<health::PlayerHealth>,
    /// Tracks which generation's background cover-fetch is in flight.
    /// 0 = no fetch in flight, non-zero = generation number of the in-flight fetch.
    /// Used to allow newer tracks to preempt older in-flight cover fetches.
    cover_fetch_generation: parking_lot::Mutex<u64>,
    /// Last cover URL successfully resolved for the current update generation.
    /// Used so later same-track activity refreshes don't clear artwork while
    /// quarantine disables normal cache reads.
    last_resolved_cover_art: Arc<Mutex<Option<(u64, String)>>>,
    /// Track identifier of the last pushed track (used in polling mode to detect
    /// track changes and bump `update_generation`).
    last_pushed_track_id: Mutex<Option<String>>,
    /// URL of the last pushed track. Used alongside `last_pushed_track_id` to
    /// detect track changes (plasma-browser-integration reuses the same track_id)
    /// and to compute `same_url_as_prev` for the health state machine.
    last_pushed_track_url: Mutex<Option<String>>,
    config: Arc<ConfigManager>,
    /// Cancellation flag for the per-player event listener thread (event-driven mode only).
    listener_cancel: Option<Arc<AtomicBool>>,
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
        let is_browser = Self::is_browser_source(player.bus_name(), player.identity());
        let health = if is_browser {
            parking_lot::Mutex::new(health::PlayerHealth::confirming(0))
        } else {
            parking_lot::Mutex::new(health::PlayerHealth::healthy(0))
        };
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
            health,
            cover_fetch_generation: parking_lot::Mutex::new(0),
            last_resolved_cover_art: Arc::new(Mutex::new(None)),
            last_pushed_track_id: parking_lot::Mutex::new(None),
            last_pushed_track_url: parking_lot::Mutex::new(None),
            config,
            listener_cancel: None,
            listener_bus: None,
        }
    }

    /// Determine if this player is a browser-based source (e.g. Plasma Browser
    /// Integration, Firefox, Chromium). Browser sources get tighter stall
    /// thresholds and a confirming state that blocks the first push until
    /// position moves.
    fn is_browser_source(bus_name: &str, identity: &str) -> bool {
        let bus = bus_name.to_ascii_lowercase();
        let identity = identity.to_ascii_lowercase();

        bus.contains("plasma-browser-integration")
            || bus.contains("firefox")
            || bus.contains("chromium")
            || bus.contains("chrome")
            || identity == "firefox"
            || identity == "chromium"
            || identity == "chrome"
            || identity == "brave"
            || identity == "vivaldi"
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
        // Resolve the app id with URL/title context so a tab covered by a
        // `[website.*]` override opens its IPC connection with the correct
        // app id directly. Without this, the URL-aware resolution later in
        // `update_activity` would tear the just-opened connection down and
        // reopen it, racing the first `set_activity` write.
        let url = self.current_url();
        let title = self.current_title();
        let (player_config, _suffix) = self.config.get_player_config_with_title_fallback(
            self.player.identity(),
            &canonical_player_bus_name(self.player.bus_name()),
            url.as_deref(),
            title.as_deref(),
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

        // The caller (main.rs discovery loop) keys presences by normalized
        // identity (`norm_id`) and may also merge groups that share the same
        // `xesam:url` (e.g. plasma-browser-integration "Zen Browser" and the
        // native "Mozilla zen" endpoint exposing the same tab). After such a
        // merge the winner passed in here can carry a different verbatim
        // identity, bus name, or unique connection name than what we last
        // stored. Treat any of those changes as a connection handoff: refresh
        // our cached player reference and reset playback state so the next
        // cycle pushes a full update with the new identity's app id.
        if player.identity() != self.player.identity()
            || player.bus_name() != self.player.bus_name()
            || player.unique_name() != self.player.unique_name()
        {
            let new_id = PlayerIdentifier::from(&player);
            debug!(
                "Player connection changed: {} ({}:{}) -> {} ({}:{})",
                self.player.identity(),
                self.player_id.player_bus_name,
                self.player_id.unique_name,
                player.identity(),
                new_id.player_bus_name,
                new_id.unique_name,
            );
            self.player_id = new_id;
            self.player = player;
            self.last_player_state = None;
            *self.last_cmus_track_id.lock() = None;
            *self.last_cmus_path.lock() = None;
            *self.last_pushed_track_url.lock() = None;
            *self.last_resolved_cover_art.lock() = None;
            self.cmus_error_logged.store(false, Ordering::Relaxed);
            // Reset health on connection handoff (new underlying player).
            let is_browser = Self::is_browser_source(
                self.player.bus_name(),
                self.player.identity(),
            );
            *self.health.lock() = if is_browser {
                health::PlayerHealth::confirming(self.update_generation.load(Ordering::Relaxed))
            } else {
                health::PlayerHealth::healthy(self.update_generation.load(Ordering::Relaxed))
            };
            // Skip Discord update this cycle; full update happens next poll.
            return Ok(());
        }

        let Some(_discord_client) = &self.discord_client else {
            return Ok(());
        };

        self.ensure_connection()?;

        let playback_status = self.player.get_playback_status().map_err(|err| {
            error!("Failed to get playback status: {}", err);
            DiscordError::ActivityError(format!("Failed to get playback status: {}", err))
        })?;
        let metadata = match self.player.get_metadata() {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to get metadata for player: {}", e);
                return Ok(());
            }
        };
        let track = health::TrackFingerprint::from_mpris(&metadata);
        let position = self.player.get_position().unwrap_or_default();
        let now = Instant::now();
        let generation = self.update_generation.load(Ordering::Relaxed);
        let is_browser = Self::is_browser_source(
            self.player.bus_name(),
            self.player.identity(),
        );

        // Detect if this track has the same URL as the previous push.
        // Used by the health state machine to decide whether to enter
        // ArtQuarantined (stale YouTube art suppression).
        let same_url = {
            let prev_url = self.last_pushed_track_url.lock();
            prev_url.as_deref().is_some_and(|prev| {
                prev == track.url.as_deref().unwrap_or("")
            })
        };

        let input = health::HealthCheckInput {
            playback_status,
            position,
            track: &track,
            track_length: track.length,
            is_browser_source: is_browser,
            generation,
            now,
            last_event: now, // latest event is right now
            same_url_as_prev: same_url,
        };

        // --- Early-exit: skip Discord pushes if nothing changed since last
        //     tick, but still let health observe normal position progress so
        //     browser sources don't look "silent" after long playback.
        let start_time = Instant::now();
        let new_state = PlaybackState::from(&self.player);
        let dbus_delay = start_time.elapsed();
        let effective_interval = if self.config.event_driven() {
            self.config.fallback_poll_interval()
        } else {
            self.config.interval()
        };
        let significant_change = self
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

        if !significant_change {
            trace!("Skipping Discord push - no significant changes detected");
            self.health.lock().observe_progress(&input);
            self.last_player_state = Some(new_state);
            return Ok(());
        }

        let outcome = {
            let mut health = self.health.lock();
            health.transition(&input)
        };

        match outcome {
            health::TransitionOutcome::Push { art_decision } => {
                self.last_player_state = Some(new_state);
                let art_gen = Some(generation);
                self.update_activity(art_gen, art_decision).await.map_err(|err| {
                    if matches!(err, DiscordError::ActivityError(_)) {
                        if !self.error_logged.load(Ordering::Relaxed) {
                            warn!("Discord connection error, will attempt to reconnect next update");
                            self.error_logged.store(true, Ordering::Relaxed);
                        }
                        self.last_player_state = None;
                        self.needs_reconnection.store(true, Ordering::Relaxed);
                    }
                    err
                })?;
                // Update previous URL reference for next tick's same_url_as_prev check.
                if let Some(ref url) = track.url {
                    *self.last_pushed_track_url.lock() = Some(url.clone());
                }
            }
            health::TransitionOutcome::Clear => {
                self.clear_discord_activity_with_reason(&format!(
                    "Clearing Discord activity - player {} is stalled/stopped/paused",
                    self.player.identity()
                ))?;
                self.last_player_state = None;
            }
            health::TransitionOutcome::Noop => {
                trace!("Noop from health state machine, skipping");
            }
        }

        Ok(())
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
        self.update_activity(None, health::ArtDecision::default()).await
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

    fn clear_discord_activity_with_reason(&self, reason: &str) -> Result<(), DiscordError> {
        if !self.error_logged.load(Ordering::Relaxed) {
            info!("{}", reason);
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
        Ok(())
    }

    async fn update_activity(
        &mut self,
        generation: Option<u64>,
        art_decision: health::ArtDecision,
    ) -> Result<(), DiscordError> {
        if self.discord_client.is_none() {
            return Ok(());
        }

        let playback_status = self.player.get_playback_status().map_err(|err| {
            error!("Failed to get playback status: {}", err);
            DiscordError::ActivityError(format!("Failed to get playback status: {}", err))
        })?;

        if playback_status == PlaybackStatus::Stopped || playback_status == PlaybackStatus::Paused {
            self.clear_discord_activity_with_reason(&format!(
                "Clearing Discord activity - player {} is {}",
                self.player.identity(),
                if playback_status == PlaybackStatus::Stopped {
                    "stopped"
                } else {
                    "paused"
                }
            ))?;
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
        let update_snapshot = UpdateSnapshot::from_mpris(playback_status, &metadata);
        trace!("Metadata: {:?}", metadata);

        // Detect a track change relative to the last push and bump the
        // generation counter so any in-flight cover-art task spawned for the
        // previous track aborts before re-pushing its (now stale) result.
        // Event-driven mode already bumps from the listener thread; this path
        // makes the same guarantee hold in polling mode.
        let track_changed = {
            let track_id = metadata.track_id().map(|id| id.to_string());
            let track_url = metadata.url().map(|url| url.to_string());
            let mut last_id = self.last_pushed_track_id.lock();
            let mut last_url = self.last_pushed_track_url.lock();
            let id_changed = last_id.as_deref() != track_id.as_deref();
            let url_changed = last_url.as_deref() != track_url.as_deref();
            if id_changed || url_changed {
                *last_id = track_id;
                *last_url = track_url;
                true
            } else {
                false
            }
        };
        if track_changed {
            self.update_generation.fetch_add(1, Ordering::Relaxed);
            self.update_notify.notify_waiters();
            // Allow the new track to spawn its own cover-art background task.
            // Setting to 0 means "no fetch in flight" — a new track can always spawn.
            *self.cover_fetch_generation.lock() = 0;
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

        if art_decision.newly_quarantined {
            warn!(
                "Suppressing stale YouTube MPRIS artwork for {}: url={:?}, art_url={:?}, allow_mpris_art_url={}",
                self.player.identity(),
                update_snapshot.track.url,
                update_snapshot.track.art_url,
                art_decision.source_options.allow_mpris_art_url
            );
        } else if art_decision.source_options != metadata::ArtSourceOptions::default() {
            trace!(
                "Continuing stale YouTube MPRIS artwork quarantine for {}: url={:?}, art_url={:?}, allow_mpris_art_url={}",
                self.player.identity(),
                update_snapshot.track.url,
                update_snapshot.track.art_url,
                art_decision.source_options.allow_mpris_art_url
            );
        }

        trace!("--- Raw Metadata Start ---");
        if let Some(mpris_meta) = metadata_source.mpris_metadata() {
            trace!("MPRIS Metadata Map:");
            for (key, value) in mpris_meta.iter() {
                trace!(
                    "  MPRIS Key: '{}', Value: {}",
                    key,
                    summarize_log_value(key, value)
                );
            }
        } else {
            trace!("No MPRIS Metadata available in source.");
        }
        if let Some(lofty_tag) = metadata_source.lofty_tag() {
            trace!("Lofty Primary Tag ({:?}):", lofty_tag.file_type());
            if let Some(tag) = lofty_tag.primary_tag() {
                for item in tag.items() {
                    trace!("  Lofty Key: {:?}, Value: {:?}", item.key(), item.value());
                }
            } else {
                trace!("  No primary tag found by Lofty.");
            }
            trace!("Lofty Properties: {:?}", lofty_tag.properties());
        } else {
            trace!(
                "No Lofty TaggedFile available in source (likely not a local file or read failed)."
            );
        }
        trace!("--- Raw Metadata End ---");

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
        let current_generation =
            generation.unwrap_or_else(|| self.update_generation.load(Ordering::Relaxed));
        let cached_cover = if art_decision.read_cache {
            self.cover_manager.try_cached_cover_art(&metadata_source)
        } else {
            self.cover_manager
                .try_cached_cover_art_with_options(&metadata_source, false)
        };
        if let Some(cover_url) = cached_cover.as_deref() {
            debug!("Serving cached cover art on fast path: {}", cover_url);
            *self.last_resolved_cover_art.lock() =
                Some((current_generation, cover_url.to_string()));
        }

        // When quarantine disables normal cache reads, later same-generation
        // activity refreshes must keep the cover that the background task
        // already resolved. Otherwise a position/timestamp refresh pushes
        // cover_art=None and clears Discord's artwork.
        let remembered_cover = if cached_cover.is_none() {
            self.last_resolved_cover_art
                .lock()
                .as_ref()
                .and_then(|(cover_generation, cover_url)| {
                    (*cover_generation == current_generation).then(|| cover_url.clone())
                })
        } else {
            None
        };
        if let Some(cover_url) = remembered_cover.as_deref() {
            debug!(
                "Reusing remembered cover art for current generation {}: {}",
                current_generation, cover_url
            );
        }
        let cover_for_push = cached_cover.as_deref().or(remembered_cover.as_deref());
        if cover_for_push.is_none() {
            debug!("Artwork source for push: none (placeholder)");
        } else if cached_cover.is_some() {
            debug!("Artwork source for push: cached_cover");
        } else if remembered_cover.is_some() {
            debug!("Artwork source for push: remembered_cover");
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
            debug!(
                "Activity payload: details={:?}, state={:?}, large_text={:?}, small_text={:?}, cover_art={:?}, activity_type={:?}",
                activity_texts.details,
                activity_texts.state,
                activity_texts.large_text,
                activity_texts.small_text,
                cover_for_push,
                activity_type,
            );
        }
        Self::build_and_push_activity(
            &discord_client,
            &ActivityFraming {
                texts: &activity_texts,
                timing: start_s.map(|s| (s, end_s)),
                cover_art_url: cover_for_push,
                player_config: &player_config,
                activity_type,
                status_display_type,
            },
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
        // Guard: skip if a background task is already in flight for a newer or
        // equal generation.  Uses generation-based gating so rapid track skips
        // allow the newest track to preempt older in-flight fetches.
        let spawn_gen = current_generation;
        if cover_for_push.is_none() && {
            let mut in_flight = self.cover_fetch_generation.lock();
            if *in_flight == 0 || *in_flight < spawn_gen {
                *in_flight = spawn_gen;
                true
            } else {
                false
            }
        } {
            let cover_manager = Arc::clone(&self.cover_manager);
            let discord_client_for_task = Arc::clone(&discord_client);
            let update_generation = Arc::clone(&self.update_generation);
            let update_notify = Arc::clone(&self.update_notify);
            let last_resolved_cover_art_for_task = Arc::clone(&self.last_resolved_cover_art);
            let texts_for_task = activity_texts.clone();
            let player_config_for_task = player_config.clone();
            let identity_for_task = self.player.identity().to_string();
            let metadata_source_for_task = metadata_source;
            let art_source_options_for_task = art_decision.source_options;
            let read_cache_for_task = art_decision.read_cache;
            let cover_fetch_gen = Arc::new(parking_lot::Mutex::new(spawn_gen));
            // Always use the freshly-loaded generation (post-bump) so this task
            // self-cancels on any subsequent track change in either run mode.
            let fetch_gen = spawn_gen;

            tokio::spawn(async move {
                // Ensure the in-flight flag is always reset when the task exits.
                struct InFlightGuard {
                    gen: Arc<parking_lot::Mutex<u64>>,
                    expected: u64,
                }
                impl Drop for InFlightGuard {
                    fn drop(&mut self) {
                        let mut current = self.gen.lock();
                        if *current == self.expected {
                            *current = 0;
                        }
                        // If generation advanced (newer track), leave it — our
                        // guard is stale and the newer fetch's guard will reset.
                    }
                }
                let _guard = InFlightGuard {
                    gen: cover_fetch_gen,
                    expected: fetch_gen,
                };
                let art_source =
                    if art_source_options_for_task == metadata::ArtSourceOptions::default() {
                        metadata_source_for_task.art_source()
                    } else {
                        metadata_source_for_task.art_source_with_options(art_source_options_for_task)
                    };
                let cover_art_result = tokio::select! {
                    result = async {
                        if read_cache_for_task {
                            cover_manager
                                .get_cover_art(art_source.clone(), &metadata_source_for_task)
                                .await
                        } else {
                            cover_manager
                                .get_cover_art_with_options(
                                    art_source.clone(),
                                    &metadata_source_for_task,
                                    false,
                                )
                                .await
                        }
                    } => result,
                    _ = update_notify.notified() => {
                        if update_generation.load(Ordering::Relaxed) != spawn_gen {
                            trace!(
                                "background cover fetch cancelled: newer track arrived for {}",
                                identity_for_task
                            );
                            return;
                        }
                        if read_cache_for_task {
                            cover_manager
                                .get_cover_art(art_source.clone(), &metadata_source_for_task)
                                .await
                        } else {
                            cover_manager
                                .get_cover_art_with_options(
                                    art_source.clone(),
                                    &metadata_source_for_task,
                                    false,
                                )
                                .await
                        }
                    }
                };

                let cover_url = match cover_art_result {
                    Ok(Some(url)) => url,
                    Ok(None) => {
                        debug!(
                            "Background cover fetch produced no art for {}",
                            identity_for_task
                        );
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

                trace!("Found cover art URL for Discord presence: {}", cover_url);
                debug!(
                    "Artwork source for push: background_fetch generation={} url={}",
                    spawn_gen, cover_url
                );
                *last_resolved_cover_art_for_task.lock() = Some((spawn_gen, cover_url.clone()));
                if let Err(err) = Self::build_and_push_activity(
                    &discord_client_for_task,
                    &ActivityFraming {
                        texts: &texts_for_task,
                        timing: start_s.map(|s| (s, end_s)),
                        cover_art_url: Some(cover_url.as_str()),
                        player_config: &player_config_for_task,
                        activity_type,
                        status_display_type,
                    },
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
        framing: &ActivityFraming<'_>,
    ) -> Result<(), DiscordError> {
        let mut activity = Activity::default()
            .activity_type(framing.activity_type.into())
            .status_display_type(framing.status_display_type.into());

        if !framing.texts.details.is_empty() {
            activity = activity.details(&framing.texts.details);
        }
        if !framing.texts.state.is_empty() {
            activity = activity.state(&framing.texts.state);
        }

        if let Some((start, end)) = framing.timing {
            activity = activity.timestamps({
                let ts = Timestamps::default().start(start as i64);
                if let Some(end) = end {
                    ts.end(end as i64)
                } else {
                    ts
                }
            });
        }

        let mut assets = Assets::default();
        if let Some(img_url) = framing.cover_art_url {
            assets = assets.large_image(img_url);
            if !framing.texts.large_text.is_empty() {
                assets = assets.large_text(&framing.texts.large_text);
            }
            if framing.player_config.show_icon {
                assets = assets.small_image(framing.player_config.icon.as_str());
                if !framing.texts.small_text.is_empty() {
                    assets = assets.small_text(&framing.texts.small_text);
                }
            }
        } else {
            assets = assets.large_image(framing.player_config.icon.as_str());
            if !framing.texts.large_text.is_empty() {
                assets = assets.large_text(&framing.texts.large_text);
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
        *self.last_pushed_track_url.lock() = None;
        *self.last_resolved_cover_art.lock() = None;
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

        // Get current track's art decision from the health state machine.
        // For TrackChanged events, run a full health transition so quarantine
        // can be entered when the URL hasn't changed (stale YouTube art).
        // Other events keep the existing snapshot behaviour.
        let art_decision;
        if is_track_change {
            let metadata = match self.player.get_metadata() {
                Ok(m) => m,
                Err(e) => {
                    warn!("Failed to get metadata in handle_event: {}", e);
                    return Ok(EventOutcome::Continue);
                }
            };
            let track = health::TrackFingerprint::from_mpris(&metadata);
            let position = self.player.get_position().unwrap_or_default();
            let now = Instant::now();
            let playback_status = self.player.get_playback_status().unwrap_or(PlaybackStatus::Playing);
            let is_browser = Self::is_browser_source(
                self.player.bus_name(),
                self.player.identity(),
            );
            let gen = generation.unwrap_or(0);
            let same_url = {
                let prev_url = self.last_pushed_track_url.lock();
                prev_url.as_deref().is_some_and(|prev| {
                    prev == track.url.as_deref().unwrap_or("")
                })
            };
            let input = health::HealthCheckInput {
                playback_status,
                position,
                track: &track,
                track_length: track.length,
                is_browser_source: is_browser,
                generation: gen,
                now,
                last_event: now,
                same_url_as_prev: same_url,
            };
            let outcome = {
                let mut h = self.health.lock();
                h.transition(&input)
            };
            match outcome {
                health::TransitionOutcome::Push {
                    art_decision: ad, ..
                } => {
                    art_decision = ad;
                }
                health::TransitionOutcome::Clear => {
                    self.clear_discord_activity_with_reason(&format!(
                        "Clearing Discord activity - player {} is stalled/stopped/paused (event)",
                        self.player.identity()
                    ))?;
                    return Ok(EventOutcome::Continue);
                }
                health::TransitionOutcome::Noop => {
                    return Ok(EventOutcome::Continue);
                }
            }
        } else {
            // Non-TrackChanged: snapshot existing art_decision.
            let current_track = self
                .player
                .get_metadata()
                .ok()
                .as_ref()
                .map(health::TrackFingerprint::from_mpris);
            art_decision = current_track
                .as_ref()
                .map(|t| self.health.lock().art_decision(t))
                .unwrap_or_default();
        }
        if let Err(err) = self.update_activity(generation, art_decision).await {
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
        // The JoinHandle is intentionally dropped — the listener thread blocks
        // on a D-Bus call we can't interrupt, so it's detached and exits on its
        // own once `cancel` flips or the player disappears.
        let _ = events::spawn_listener(
            current_bus.clone(),
            norm_id,
            tx,
            cancel.clone(),
            self.update_generation.clone(),
            self.update_notify.clone(),
        );
        self.listener_cancel = Some(cancel);
        self.listener_bus = Some(current_bus);
    }

    /// Cancel the listener thread. NOTE: cancellation is best-effort — the
    /// thread blocks on `mpris::Player::events()` and only checks the cancel
    /// flag on the next D-Bus event, so it may live on briefly after this
    /// returns. The trailing `ListenerExited` event is dropped silently by
    /// `Mprisence::handle_player_event` when the norm_id is no longer tracked.
    pub fn stop_listener(&mut self) {
        if let Some(cancel) = self.listener_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
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

    fn player_config_with(
        app_id: &str,
        status_display_type: StatusDisplayType,
    ) -> PlayerConfig {
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
        let default_app_state =
            player_config_with(DEFAULT_PLAYER_APP_ID, StatusDisplayType::State);
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
