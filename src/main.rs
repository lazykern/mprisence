// #[cfg(not(target_os = "linux"))]
// compile_error!("mprisence only supports Linux systems as it relies on MPRIS (Media Player Remote Interfacing Specification)");

use clap::Parser;
use config::{get_config, ConfigManager};
use cover::CoverManager;
use error::MprisenceError;
use log::{debug, error, info, trace, warn};
use mpris::PlayerFinder;
use player::{is_playerctld_no_active_error, select_winner_idx, PlayerIdentifier};
use presence::Presence;
use smol_str::SmolStr;
use std::{
    alloc::System,
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

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
            let player_config = self
                .config
                .get_player_config(&pid.identity, &pid.player_bus_name);
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
            !self
                .config
                .get_player_config(&pid.identity, &pid.player_bus_name)
                .ignore
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
                trace!("Skipping ignored player: {}", id.identity);
                continue;
            }

            let norm_id = SmolStr::new(utils::normalize_player_identity(&id.identity));
            candidates.entry(norm_id).or_default().push(player);
        }

        // Phase 2: for each identity group select one winner and update/create
        // the corresponding Presence.  This ensures at most one Discord IPC
        // connection per logical player identity.
        let mut current_norm_ids = HashSet::new();
        let mut duplicate_norm_ids = HashSet::new();

        for (norm_id, mut group) in candidates {
            current_norm_ids.insert(norm_id.clone());

            let ids: Vec<PlayerIdentifier> =
                group.iter().map(|p| PlayerIdentifier::from(p)).collect();

            let current_bus = self
                .media_players
                .get(&norm_id)
                .map(|presence| presence.player_id().player_bus_name.clone());

            let winner_idx = select_winner_idx(&ids, current_bus.as_deref());

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
                self.media_players.insert(norm_id, presence);
            }
        }

        // Phase 3: remove entries whose player has gone away or been reconfigured.
        self.media_players.retain(|norm_id, presence| {
            let (identity, player_bus_name) = {
                let pid = presence.player_id();
                (pid.identity.clone(), pid.player_bus_name.clone())
            };
            let player_config = self.config.get_player_config(&identity, &player_bus_name);
            let allowed = self.config.is_player_allowed(&identity, &player_bus_name);
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
}
