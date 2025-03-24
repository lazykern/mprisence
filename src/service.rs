use std::{
    collections::HashMap,
    sync::Arc,
    time::Duration,
};

use crate::{
    config::{self, get_config},
    cover::CoverManager,
    error::{ServiceInitError, ServiceRuntimeError},
    mprisence::Mprisence,
    player::PlayerId,
    template,
};
use log::{debug, error, info, trace, warn};
use mpris::PlayerFinder;

pub struct Service {
    player_finder: PlayerFinder,
    mprisences: HashMap<PlayerId, Mprisence>,
    template_manager: Arc<template::TemplateManager>,
    cover_manager: Arc<CoverManager>,
    config_rx: config::ConfigChangeReceiver,
}

impl Service {
    pub fn new() -> Result<Self, ServiceInitError> {
        info!("Initializing service components");

        debug!("Creating template manager");
        let config = get_config();
        let template_manager = Arc::new(template::TemplateManager::new(&config)?);

        debug!("Creating cover manager");
        let cover_manager = Arc::new(CoverManager::new(&config)?);

        debug!("Creating player finder");
        let player_finder = PlayerFinder::new()?;

        info!("Service initialization complete");
        Ok(Self {
            player_finder,
            mprisences: HashMap::new(),
            template_manager,
            cover_manager,
            config_rx: get_config().subscribe(),
        })
    }

    async fn handle_config_change(&mut self) -> Result<(), ServiceRuntimeError> {
        Ok(())
    }

    pub async fn update(&mut self) {
        debug!("Updating Discord presence");
        let mut current_ids = std::collections::HashSet::new();

        trace!("Finding players");
        for player in self.player_finder.iter_players().unwrap() {
            let player = player.unwrap();
            let id = PlayerId::from(&player);
            current_ids.insert(id.clone());

            debug!("Updating player {}", id);
            if let Some(mprisence) = self.mprisences.get_mut(&id) {
                if let Err(e) = mprisence.update(player).await {
                    debug!("Failed to update player {}: {}", id.identity, e);
                }
            } else {
                debug!("New player added: {}", id.identity);
                self.mprisences.insert(
                    id,
                    Mprisence::new(
                        player,
                        self.template_manager.clone(),
                        self.cover_manager.clone(),
                    ),
                );
            }
        }

        // Now remove players that no longer exist
        self.mprisences.retain(|id, mprisence| {
            let keep = current_ids.contains(id);
            if !keep {
                debug!("Player removed: {}", id.identity);
                let _ = mprisence.destroy();
            }
            keep
        });
    }

    pub async fn run(&mut self) -> Result<(), ServiceRuntimeError> {
        info!("Starting service main loop");

        let mut interval = tokio::time::interval(Duration::from_millis(get_config().interval()));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                  self.update().await;
                },
                Ok(change) = self.config_rx.recv() => {
                    match change {
                        config::ConfigChange::Updated | config::ConfigChange::Reloaded => {
                            info!("Config change detected");
                            // Update interval with new config
                            interval = tokio::time::interval(Duration::from_millis(get_config().interval()));

                            // Handle config change
                            if let Err(e) = self.handle_config_change().await {
                                error!("Failed to handle config change: {}", e);
                            }
                        },
                        config::ConfigChange::Error(e) => {
                            error!("Config error: {}", e);
                        }
                    }
                },

                else => {
                    warn!("All event sources have closed, shutting down");
                    break;
                }
            }
        }

        Ok(())
    }
}
