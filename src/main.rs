use clap::Parser;
use config::{get_config, ConfigManager};
use cover::CoverManager;
use error::MprisenceError;
use log::{debug, info, trace, warn, error};
use mpris::PlayerFinder;
use player::PlayerIdentifier;
use presence::Presence;

mod cli;
mod config;
mod cover;
mod error;
mod presence;
mod utils;

use std::{alloc::System, collections::HashMap, sync::Arc, time::Duration};

#[global_allocator]
static GLOBAL: System = System;

mod player;
mod template;

use crate::cli::Cli;

#[tokio::main]
async fn main() -> Result<(), error::Error> {
    env_logger::init();

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
    player_finder: PlayerFinder,
    presences: HashMap<PlayerIdentifier, Presence>,
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

        trace!("Creating player finder");
        let player_finder = PlayerFinder::new()?;

        debug!("Service initialization complete");
        Ok(Self {
            player_finder,
            presences: HashMap::new(),
            template_manager,
            cover_manager,
            config_rx: config.subscribe(),
            config,
        })
    }

    async fn handle_config_change(&mut self) -> Result<(), MprisenceError> {
        info!("Configuration change detected, updating components");
        self.config = get_config();

        // Update template manager
        trace!("Updating template manager");
        self.template_manager = Arc::new(template::TemplateManager::new(&self.config)?);
        debug!("Template manager updated successfully");

        // Update cover manager
        trace!("Updating cover manager");
        self.cover_manager = Arc::new(CoverManager::new(&self.config)?);
        debug!("Cover manager updated successfully");

        // Update all presences with new managers
        trace!("Updating all presences with new managers");
        for presence in self.presences.values_mut() {
            presence.update_managers(
                self.template_manager.clone(),
                self.cover_manager.clone(),
                self.config.clone(),
            );
        }
        debug!("All presences updated with new configuration");

        Ok(())
    }

    pub async fn update(&mut self) {
        trace!("Starting Discord presence update cycle");
        let mut current_ids = std::collections::HashSet::new();

        trace!("Scanning for active media players");
        for player in self.player_finder.iter_players().unwrap() {
            let player = player.unwrap();
            let id = PlayerIdentifier::from(&player);
            current_ids.insert(id.clone());

            trace!("Processing player {}", id);
            if let Some(presence) = self.presences.get_mut(&id) {
                if let Err(e) = presence.update(player).await {
                    warn!("Failed to update player {}: {}", id.identity, e);
                }
            } else {
                debug!("New media player detected: {}", id.identity);
                self.presences.insert(
                    id,
                    Presence::new(
                        player,
                        self.template_manager.clone(),
                        self.cover_manager.clone(),
                        self.config.clone(),
                    ),
                );
            }
        }

        // Now remove players that no longer exist
        self.presences.retain(|id, presence| {
            let keep = current_ids.contains(id);
            if !keep {
                debug!("Media player removed from tracking: {}", id.identity);
                let _ = presence.destroy();
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