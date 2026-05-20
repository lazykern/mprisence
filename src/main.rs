use clap::Parser;
use config::{get_config, ConfigManager};
use cover::CoverManager;
use error::MprisenceError;
use log::{debug, error, info, trace, warn};
use mpris::Event as MprisEvent;
use mpris::PlayerFinder;
use player::{
    compute_presence_migrations,
    events::{EventOutcome, PlayerEvent, PlayerEventKind},
    is_playerctld_no_active_error, merge_url_duplicates, select_richest_player, select_winner_idx,
    BucketSummary, PlayerIdentifier,
};
use presence::Presence;
use smol_str::SmolStr;
use std::{
    alloc::System,
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::sync::mpsc;

#[global_allocator]
static GLOBAL: System = System;

mod cli;
mod config;
mod cover;
mod discord;
mod error;
mod metadata;
mod player;
mod presence;
mod template;
mod utils;

use crate::cli::Cli;

#[tokio::main]
async fn main() -> Result<(), error::Error> {
    env_logger::init();

    info!("Starting mprisence");

    config::initialize()?;

    let cli = Cli::parse();

    match cli.command {
        Some(cmd) => cmd.execute().await?,
        None => {
            let mut mprisence = Mprisence::new()?;
            mprisence.run().await?;
        }
    }

    Ok(())
}

pub struct Mprisence {
    /// Keyed by normalised player identity (e.g. `"music_player_daemon"`).
    /// Enforces at most one Discord presence per logical player, regardless of
    /// how many D-Bus bus names expose the same underlying player.
    media_players: HashMap<SmolStr, Presence>,
    /// Last seen deduplication selection for identities that currently expose
    /// multiple bus names.
    dedup_selection: HashMap<SmolStr, DedupSelection>,
    template_manager: Arc<template::TemplateManager>,
    cover_manager: Arc<CoverManager>,
    config_rx: config::ConfigChangeReceiver,
    config: Arc<ConfigManager>,
}

#[derive(Clone, Debug)]
struct DedupSelection {
    buses_signature: String,
    winner_bus: SmolStr,
}

impl Mprisence {
    pub fn new() -> Result<Self, MprisenceError> {
        info!("Initializing service components");

        let config = get_config();

        trace!("Creating template manager");
        let template_manager = Arc::new(template::TemplateManager::new(&config)?);

        trace!("Creating cover manager");
        let cover_manager = Arc::new(CoverManager::new(&config)?);

        debug!("Service initialization complete");
        Ok(Self {
            media_players: HashMap::new(),
            dedup_selection: HashMap::new(),
            template_manager,
            cover_manager,
            config_rx: config.subscribe(),
            config,
        })
    }

    async fn handle_config_change(&mut self) -> Result<(), MprisenceError> {
        info!("Configuration change detected, updating components");
        self.config = get_config();

        self.template_manager = Arc::new(template::TemplateManager::new(&self.config)?);
        self.cover_manager = Arc::new(CoverManager::new(&self.config)?);
        debug!("Template and cover managers updated successfully");

        for (_norm_id, presence) in self.media_players.iter_mut() {
            let pid = presence.player_id();
            let url = presence.current_url();
            let title = presence.current_title();
            let (player_config, _) = self.config.get_player_config_with_title_fallback(
                &pid.identity,
                &pid.player_bus_name,
                url.as_deref(),
                title.as_deref(),
            );
            let is_allowed = self
                .config
                .is_player_allowed(&pid.identity, &pid.player_bus_name);
            if player_config.ignore || !is_allowed {
                debug!(
                    "Player now {}: {}",
                    if player_config.ignore {
                        "ignored"
                    } else {
                        "disallowed"
                    },
                    pid.identity
                );
                let _ = presence.destroy_discord_client();
            } else {
                presence.update_managers(
                    self.template_manager.clone(),
                    self.cover_manager.clone(),
                    self.config.clone(),
                );
            }
        }

        self.media_players.retain(|_norm_id, presence| {
            let pid = presence.player_id();
            let url = presence.current_url();
            let title = presence.current_title();
            let (player_config, _) = self.config.get_player_config_with_title_fallback(
                &pid.identity,
                &pid.player_bus_name,
                url.as_deref(),
                title.as_deref(),
            );
            !player_config.ignore
                && self
                    .config
                    .is_player_allowed(&pid.identity, &pid.player_bus_name)
        });

        debug!("All media players updated with new configuration");
        Ok(())
    }

    pub async fn update(&mut self) -> Result<(), MprisenceError> {
        trace!("Starting Discord presence update cycle");

        let discord_running = discord::is_discord_running();
        if !discord_running {
            trace!("Discord is not running, destroying all Discord clients");
            for presence in self.media_players.values_mut() {
                if let Err(e) = presence.destroy_discord_client() {
                    warn!("Failed to destroy Discord client: {}", e);
                }
            }
            return Ok(());
        }

        trace!("Scanning for active media players");

        let mut player_finder = PlayerFinder::new()?;
        player_finder.set_player_timeout_ms(5000);

        // Phase 1: collect all allowed, non-ignored players and group them by
        // normalised identity so that multiple bus names for the same underlying
        // player (e.g. `mpd` and `playerctld`) are treated as one logical player.
        let mut candidates: HashMap<SmolStr, Vec<mpris::Player>> = HashMap::new();

        for player in player_finder.iter_players()? {
            let player = match player {
                Ok(p) => p,
                Err(err) if is_playerctld_no_active_error(&err) => {
                    debug!("Skipping playerctld proxy without an active player during discovery");
                    continue;
                }
                Err(err) => return Err(err.into()),
            };

            let id = PlayerIdentifier::from(&player);

            if !self
                .config
                .is_player_allowed(&id.identity, &id.player_bus_name)
            {
                trace!("Skipping disallowed player: {}", id.identity);
                continue;
            }

            let player_config = self
                .config
                .get_player_config(&id.identity, &id.player_bus_name);
            if player_config.ignore {
                // Before skipping, check if a website override un-ignores this player
                // (e.g. SoundCloud in a browser that is ignored by default).
                let meta = player.get_metadata().ok();
                let url = meta.as_ref().and_then(|m| m.url().map(|s| s.to_string()));
                let title = meta.as_ref().and_then(|m| m.title().map(|s| s.to_string()));
                let (effective_config, _suffix) =
                    self.config.get_player_config_with_title_fallback(
                        &id.identity,
                        &id.player_bus_name,
                        url.as_deref(),
                        title.as_deref(),
                    );
                if effective_config.ignore {
                    trace!("Skipping ignored player: {}", id.identity);
                    continue;
                }
                trace!(
                    "Player '{}' ignored at player level but un-ignored by website override (url: {:?}, title: {:?})",
                    id.identity,
                    url,
                    title,
                );
            }

            let norm_id = SmolStr::new(utils::normalize_player_identity(&id.identity));
            candidates.entry(norm_id).or_default().push(player);
        }

        // Phase 1.5: merge identity groups that share the same xesam:url, so
        // that e.g. plasma-browser-integration and the native browser MPRIS
        // endpoint exposing the same tab are tracked as one logical player.
        let candidates = merge_url_duplicates(candidates);

        // Phase 1.6: presence-key migration. When URL-merge changes the
        // post-merge norm_id of an existing logical player (e.g. plasma
        // begins or ceases to expose a tab the native bus also reports),
        // re-key the existing Presence so its Discord IPC client, listener
        // thread, and cached playback state survive instead of being torn
        // down and reconstructed.
        {
            let existing: HashMap<SmolStr, SmolStr> = self
                .media_players
                .iter()
                .map(|(k, p)| (k.clone(), p.player_id().player_bus_name.clone()))
                .collect();

            let buckets: Vec<BucketSummary> = candidates
                .iter()
                .map(|(norm_id, players)| {
                    let ids: Vec<PlayerIdentifier> =
                        players.iter().map(PlayerIdentifier::from).collect();
                    let mut bus_names: Vec<SmolStr> =
                        ids.iter().map(|id| id.player_bus_name.clone()).collect();
                    bus_names.sort_unstable();
                    let winner_idx = select_richest_player(players, None);
                    BucketSummary {
                        norm_id: norm_id.clone(),
                        bus_names,
                        winner_bus: ids[winner_idx].player_bus_name.clone(),
                    }
                })
                .collect();

            let (migrations, drops) = compute_presence_migrations(&existing, &buckets);

            for d in drops {
                if let Some(mut p) = self.media_players.remove(&d.key) {
                    warn!(
                        "Dropping duplicate presence '{}' (superseded by '{}')",
                        d.key, d.superseded_by
                    );
                    if let Err(e) = p.destroy_discord_client() {
                        warn!("Failed to destroy Discord client for {}: {}", d.key, e);
                    }
                    p.stop_listener();
                }
                self.dedup_selection.remove(&d.key);
            }

            for m in migrations {
                if let Some(presence) = self.media_players.remove(&m.from_key) {
                    debug!(
                        "Migrating presence '{}' -> '{}' (URL-merge bucket rename)",
                        m.from_key, m.to_key
                    );
                    self.media_players.insert(m.to_key.clone(), presence);
                }
                if let Some(sel) = self.dedup_selection.remove(&m.from_key) {
                    self.dedup_selection.insert(m.to_key, sel);
                }
            }
        }

        // Phase 2: for each identity group select one winner and update/create
        // the corresponding Presence.  This ensures at most one Discord IPC
        // connection per logical player identity.
        let mut current_norm_ids = HashSet::new();
        let mut duplicate_norm_ids = HashSet::new();

        for (norm_id, mut group) in candidates {
            current_norm_ids.insert(norm_id.clone());

            let current_bus = self
                .media_players
                .get(&norm_id)
                .map(|presence| presence.player_id().player_bus_name.clone());

            let ids: Vec<PlayerIdentifier> = group.iter().map(PlayerIdentifier::from).collect();

            let winner_idx = if group.len() > 1 {
                // Use metadata-aware selection for merged/duplicate groups
                select_richest_player(&group, current_bus.as_deref())
            } else {
                select_winner_idx(&ids, current_bus.as_deref())
            };

            if ids.len() > 1 {
                duplicate_norm_ids.insert(norm_id.clone());

                let mut bus_names: Vec<&str> =
                    ids.iter().map(|id| id.player_bus_name.as_str()).collect();
                bus_names.sort_unstable();

                let buses_signature = bus_names.join(", ");
                let winner_bus = SmolStr::new(ids[winner_idx].player_bus_name.as_str());

                let next = DedupSelection {
                    buses_signature: buses_signature.clone(),
                    winner_bus: winner_bus.clone(),
                };

                match self.dedup_selection.get(&norm_id) {
                    None => {
                        warn!(
                            "Duplicate MPRIS bus names detected for '{}': [{}] — selecting '{}'",
                            ids[0].identity, buses_signature, winner_bus
                        );
                        self.dedup_selection.insert(norm_id.clone(), next);
                    }
                    Some(prev)
                        if prev.buses_signature != buses_signature
                            || prev.winner_bus != winner_bus =>
                    {
                        warn!(
                            "Dedup selection changed for '{}': [{}] — now selecting '{}'",
                            ids[0].identity, buses_signature, winner_bus
                        );
                        self.dedup_selection.insert(norm_id.clone(), next);
                    }
                    Some(_) => {}
                }
            }

            let winner_player = group.remove(winner_idx);
            let winner_id = &ids[winner_idx];

            trace!("Processing player {}", winner_id);

            if let Some(presence) = self.media_players.get_mut(&norm_id) {
                if let Err(e) = presence.initialize_discord_client() {
                    warn!(
                        "Failed to initialize Discord client for {}: {}",
                        winner_id.identity, e
                    );
                }
                // In event-driven mode, signals drive most Discord updates. The discovery
                // tick still calls presence.update() so that position jumps (seeks on
                // players that don't emit Seeked) are caught by the has_position_jump
                // check inside update(). The internal diff logic prevents unnecessary
                // Discord pushes when nothing has changed.
                if let Err(e) = presence.update(winner_player).await {
                    warn!(
                        "Failed to update presence for {}: {}",
                        winner_id.identity, e
                    );
                }
            } else {
                debug!("New media player detected: {}", winner_id.identity);
                let mut presence = Presence::new(
                    winner_player,
                    self.template_manager.clone(),
                    self.cover_manager.clone(),
                    self.config.clone(),
                );
                if let Err(e) = presence.initialize_discord_client() {
                    warn!(
                        "Failed to initialize Discord client for new player {}: {}",
                        winner_id.identity, e
                    );
                }
                // Always do the initial Discord push when a new player is discovered,
                // even in event-driven mode — signals only fire on *changes*, so the
                // current state must be pushed once at creation time.
                if let Err(e) = presence.update_from_current_state().await {
                    warn!(
                        "Failed to set initial presence for {}: {}",
                        winner_id.identity, e
                    );
                }
                self.media_players.insert(norm_id, presence);
            }
        }

        // Phase 3: remove entries whose player has gone away or been reconfigured.
        self.media_players.retain(|norm_id, presence| {
            let (identity, player_bus_name) = {
                let pid = presence.player_id();
                (pid.identity.clone(), pid.player_bus_name.clone())
            };
            let allowed = self.config.is_player_allowed(&identity, &player_bus_name);
            // Use URL-aware config to respect website overrides (e.g. SoundCloud
            // un-ignoring a browser that is ignored by default).
            // Also try title-suffix inference for players without xesam:url.
            let url = presence.current_url();
            let title = presence.current_title();
            let (player_config, _suffix) = self.config.get_player_config_with_title_fallback(
                &identity,
                &player_bus_name,
                url.as_deref(),
                title.as_deref(),
            );
            let keep = current_norm_ids.contains(norm_id) && !player_config.ignore && allowed;
            if !keep {
                let reason = if !current_norm_ids.contains(norm_id) {
                    "player no longer exists"
                } else if player_config.ignore {
                    "player is now ignored"
                } else {
                    "player is not in the allowed list"
                };
                debug!(
                    "Media player removed from tracking: {} ({})",
                    identity, reason
                );
                if let Err(e) = presence.destroy_discord_client() {
                    warn!("Failed to destroy Discord client for {}: {}", identity, e);
                }
            }
            keep
        });

        self.dedup_selection.retain(|norm_id, _| {
            current_norm_ids.contains(norm_id) && duplicate_norm_ids.contains(norm_id)
        });

        Ok(())
    }

    pub async fn run(&mut self) -> Result<(), MprisenceError> {
        info!("Starting mprisence service");
        if self.config.event_driven() {
            info!(
                "Run mode: event-driven (D-Bus signal monitoring, fallback poll={}ms)",
                self.config.fallback_poll_interval()
            );
            self.run_event_driven().await
        } else {
            info!("Run mode: polling (interval={}ms)", self.config.interval());
            self.run_polling().await
        }
    }

    async fn run_polling(&mut self) -> Result<(), MprisenceError> {
        let mut interval = tokio::time::interval(Duration::from_millis(self.config.interval()));
        let mut cache_cleanup_interval = tokio::time::interval(Duration::from_secs(6 * 60 * 60));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    trace!("Running periodic presence update");
                    if let Err(e) = self.update().await {
                        error!("Failed to update presence: {}", e);
                    }
                },
                _ = cache_cleanup_interval.tick() => {
                    debug!("Starting periodic cache cleanup");
                    match cover::clean_cache().await {
                        Ok(_) => debug!("Cache cleanup completed successfully"),
                        Err(e) => error!("Cache cleanup failed: {}", e)
                    }
                },
                Ok(change) = self.config_rx.recv() => {
                    match change {
                        config::ConfigChange::Reloaded => {
                            debug!("Configuration change detected");
                            if self.config.event_driven() {
                                warn!("event_driven flag flipped at runtime; restart mprisence to switch modes");
                            }
                            interval = tokio::time::interval(Duration::from_millis(self.config.interval()));

                            if let Err(e) = self.handle_config_change().await {
                                error!("Failed to handle configuration change: {}", e);
                            }
                        },
                        config::ConfigChange::Error(e) => {
                            error!("Configuration error: {}", e);
                        }
                    }
                },
                else => {
                    warn!("All event sources have closed, initiating shutdown");
                    break;
                }
            }
        }

        Ok(())
    }

    async fn run_event_driven(&mut self) -> Result<(), MprisenceError> {
        /// Drain the mpsc channel for TrackChanged events on the same player.
        /// When the user skips rapidly, multiple TrackChanged events queue up.
        /// Processing them all wastes cover art fetches on tracks the user has
        /// already moved past. This keeps only the latest one.
        fn drain_latest_track_change(
            mut evt: PlayerEvent,
            rx: &mut tokio::sync::mpsc::Receiver<PlayerEvent>,
        ) -> (PlayerEvent, Vec<PlayerEvent>) {
            let mut deferred = Vec::new();
            if !matches!(
                evt.kind,
                PlayerEventKind::Mpris(MprisEvent::TrackChanged(_))
            ) {
                return (evt, deferred);
            }
            while let Ok(newer) = rx.try_recv() {
                if newer.norm_id == evt.norm_id
                    && matches!(
                        newer.kind,
                        PlayerEventKind::Mpris(MprisEvent::TrackChanged(_))
                    )
                {
                    trace!(
                        "drain: skipping intermediate TrackChanged for {}",
                        evt.norm_id
                    );
                    evt = newer;
                } else {
                    // Preserve non-TrackChanged events (notably TrackMetadataChanged,
                    // which can carry the corrected art URL a few seconds after a
                    // browser track switch) and other players' events.
                    trace!(
                        "drain: deferring {:?} for {} during skip drain",
                        newer.kind,
                        newer.norm_id
                    );
                    deferred.push(newer);
                }
            }
            (evt, deferred)
        }
        let (event_tx, mut event_rx) = mpsc::channel::<PlayerEvent>(64);

        let mut fallback_poll_interval =
            tokio::time::interval(Duration::from_millis(self.config.fallback_poll_interval()));
        let mut cache_cleanup_interval = tokio::time::interval(Duration::from_secs(6 * 60 * 60));

        // Prime once so listeners attach to whatever is already running.
        debug!("fallback poll (initial)");
        if let Err(e) = self.update().await {
            error!("Initial discovery failed: {}", e);
        }
        self.ensure_listeners(&event_tx);

        loop {
            tokio::select! {
                Some(evt) = event_rx.recv() => {
                    // For TrackChanged events, drain any newer ones for the same player
                    // that already queued while we were processing the previous event.
                    // This prevents Discord from cycling through every skipped track.
                    let (evt, deferred) = drain_latest_track_change(evt, &mut event_rx);
                    self.handle_player_event(evt, &event_tx).await;
                    for deferred_evt in deferred {
                        self.handle_player_event(deferred_evt, &event_tx).await;
                    }
                    // Reset the fallback poll timer so the next poll happens a full
                    // interval from now. Events are the primary update mechanism;
                    // the fallback poll exists only to catch missed events.
                    fallback_poll_interval.reset();
                },
                _ = fallback_poll_interval.tick() => {
                    trace!("fallback poll tick");
                    if let Err(e) = self.update().await {
                        error!("Failed to refresh players: {}", e);
                    }
                    self.ensure_listeners(&event_tx);
                },
                _ = cache_cleanup_interval.tick() => {
                    debug!("Starting periodic cache cleanup");
                    match cover::clean_cache().await {
                        Ok(_) => debug!("Cache cleanup completed successfully"),
                        Err(e) => error!("Cache cleanup failed: {}", e)
                    }
                },
                Ok(change) = self.config_rx.recv() => {
                    match change {
                        config::ConfigChange::Reloaded => {
                            debug!("Configuration change detected");
                            if !self.config.event_driven() {
                                warn!("event_driven flag flipped to false at runtime; restart mprisence to switch back to polling mode");
                            }
                            fallback_poll_interval = tokio::time::interval(
                                Duration::from_millis(self.config.fallback_poll_interval()),
                            );
                            if let Err(e) = self.handle_config_change().await {
                                error!("Failed to handle configuration change: {}", e);
                            }
                            self.ensure_listeners(&event_tx);
                        },
                        config::ConfigChange::Error(e) => {
                            error!("Configuration error: {}", e);
                        }
                    }
                },
                else => {
                    warn!("All event sources have closed, initiating shutdown");
                    break;
                }
            }
        }

        Ok(())
    }

    fn ensure_listeners(&mut self, tx: &mpsc::Sender<PlayerEvent>) {
        for (norm_id, presence) in self.media_players.iter_mut() {
            presence.ensure_listener(tx.clone(), norm_id.clone());
        }
    }

    async fn handle_player_event(&mut self, evt: PlayerEvent, tx: &mpsc::Sender<PlayerEvent>) {
        let PlayerEvent { norm_id, kind } = evt;
        trace!("dispatch event to {}: {:?}", norm_id, kind);

        let outcome = match self.media_players.get_mut(&norm_id) {
            Some(presence) => match presence.handle_event(kind).await {
                Ok(outcome) => outcome,
                Err(e) => {
                    warn!("handle_event failed for {}: {}", norm_id, e);
                    EventOutcome::Continue
                }
            },
            None => {
                trace!("event for unknown presence {} (already removed)", norm_id);
                return;
            }
        };

        if matches!(outcome, EventOutcome::ShouldRemove) {
            debug!(
                "removing presence {} (listener reported termination)",
                norm_id
            );
            if let Some(mut presence) = self.media_players.remove(&norm_id) {
                presence.stop_listener();
                if let Err(e) = presence.destroy_discord_client() {
                    warn!("Failed to destroy Discord client for {}: {}", norm_id, e);
                }
            }
            // Do NOT trigger immediate rediscovery here. The D-Bus name may
            // linger briefly after the player signals shutdown, causing a
            // spurious re-detection that opens a new Discord IPC connection.
            // That second connection interferes with Discord's activity cleanup
            // from the first connection, leaving a stale rich presence.
            // Instead, let the normal fallback_poll_interval tick (default 5s)
            // handle re-detection — by then the name will be fully gone, or
            // if the player genuinely restarted it will be picked up cleanly.
            self.ensure_listeners(tx);
        }
    }
}
