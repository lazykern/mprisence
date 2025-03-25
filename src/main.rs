#[cfg(not(target_os = "linux"))]
compile_error!("mprisence only supports Linux systems as it relies on MPRIS (Media Player Remote Interfacing Specification)");

use clap::Parser;
use config::{get_config, ConfigManager};
use cover::CoverManager;
use error::MprisenceError;
use log::{debug, error, info, trace, warn};
use mpris::PlayerFinder;
use player::PlayerIdentifier;
use presence::Presence;
use std::{
    alloc::System, collections::HashMap, sync::Arc, time::Duration,
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

/// mprisence - Discord Rich Presence for MPRIS-compatible media players
/// 
/// This application only works on Linux systems as it relies on MPRIS
/// (Media Player Remote Interfacing Specification), which is a standard
/// D-Bus interface for controlling media players on Linux/Unix systems.
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
    mpris_finder: PlayerFinder,
    media_players: HashMap<PlayerIdentifier, Presence>,
    template_manager: Arc<template::TemplateManager>,
    cover_manager: Arc<CoverManager>,
    config_rx: config::ConfigChangeReceiver,
    config: Arc<ConfigManager>,
}

impl Mprisence {
    pub fn new() -> Result<Self, MprisenceError> {
        info!("Initializing service components");

        let config = get_config();

        trace!("Creating template manager");
        let template_manager = Arc::new(template::TemplateManager::new(&config)?);

        trace!("Creating cover manager");
        let cover_manager = Arc::new(CoverManager::new(&config)?);

        trace!("Creating MPRIS finder");
        let mpris_finder = PlayerFinder::new()?;

        debug!("Service initialization complete");
        Ok(Self {
            mpris_finder,
            media_players: HashMap::new(),
            template_manager,
            cover_manager,
            config_rx: config.subscribe(),
            config,
        })
    }

    async fn handle_config_change(&mut self) -> Result<(), MprisenceError> {
        info!("Configuration change detected, updating components");
        self.config = get_config();

        trace!("Updating template manager");
        self.template_manager = Arc::new(template::TemplateManager::new(&self.config)?);
        debug!("Template manager updated successfully");

        trace!("Updating cover manager");
        self.cover_manager = Arc::new(CoverManager::new(&self.config)?);
        debug!("Cover manager updated successfully");

        trace!("Checking for players affected by configuration changes");
        self.media_players.retain(|id, presence| {
            let player_config = self.config.get_player_config(&id.identity);
            let keep = !player_config.ignore;
            if !keep {
                debug!("Removing player due to ignore setting: {}", id.identity);
                let _ = presence.destroy_discord_client();
            } else {
                presence.update_managers(
                    self.template_manager.clone(),
                    self.cover_manager.clone(),
                    self.config.clone(),
                );
            }
            keep
        });

        debug!("All media players updated with new configuration");
        Ok(())
    }

    pub async fn update(&mut self) {
        trace!("Starting Discord presence update cycle");

        // Check if Discord is running
        let discord_running = discord::is_discord_running();
        if !discord_running {
            trace!("Discord is not running, destroying all Discord clients");
            for presence in self.media_players.values_mut() {
                let _ = presence.destroy_discord_client();
            }
            return;
        }

        let mut current_ids = std::collections::HashSet::new();

        trace!("Scanning for active media players");
        for player in self.mpris_finder.iter_players().unwrap() {
            let player = player.unwrap();
            let id = PlayerIdentifier::from(&player);

            // Check if player should be ignored
            let player_config = self.config.get_player_config(&id.identity);
            if player_config.ignore {
                trace!("Skipping ignored player: {}", id.identity);
                continue;
            }

            current_ids.insert(id.clone());

            trace!("Processing player {}", id);
            if let Some(presence) = self.media_players.get_mut(&id) {
                // Initialize Discord client if needed
                let _ = presence.initialize_discord_client();
                let _ = presence.update(player).await;
            } else {
                debug!("New media player detected: {}", id.identity);
                let mut presence = Presence::new(
                    player,
                    self.template_manager.clone(),
                    self.cover_manager.clone(),
                    self.config.clone(),
                );
                let _ = presence.initialize_discord_client();
                self.media_players.insert(id, presence);
            }
        }

        // Now remove players that no longer exist or are now ignored
        self.media_players.retain(|id, presence| {
            let player_config = self.config.get_player_config(&id.identity);
            let keep = current_ids.contains(id) && !player_config.ignore;
            if !keep {
                let reason = if !current_ids.contains(id) {
                    "player no longer exists"
                } else {
                    "player is now ignored"
                };
                debug!(
                    "Media player removed from tracking: {} ({})",
                    id.identity, reason
                );
                let _ = presence.destroy_discord_client();
            }
            keep
        });
    }

    pub async fn run(&mut self) -> Result<(), MprisenceError> {
        info!("Starting mprisence service");

        let mut interval = tokio::time::interval(Duration::from_millis(self.config.interval()));
        // Add cache cleanup interval - run every 6 hours
        let mut cache_cleanup_interval = tokio::time::interval(Duration::from_secs(6 * 60 * 60));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    trace!("Running periodic presence update");
                    self.update().await;
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
                            // Update interval with new config
                            interval = tokio::time::interval(Duration::from_millis(self.config.interval()));

                            // Handle config change
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
