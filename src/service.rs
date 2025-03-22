use std::{sync::Arc, time::Duration};

use log::{debug, error, info, trace, warn};
use smallvec::SmallVec;
use tokio::sync::{mpsc, Mutex};

use crate::{config::{self, get_config}, error::{ServiceInitError, ServiceRuntimeError}, event::Event, player::PlayerManager, presence, template};

pub struct Service {
    player_manager: Arc<Mutex<PlayerManager>>,
    presence_manager: presence::PresenceManager,
    event_rx: mpsc::Receiver<Event>,
    event_tx: mpsc::Sender<Event>,
    config_rx: config::ConfigChangeReceiver,
    pending_events: SmallVec<[Event; 16]>,
}

impl Service {
    pub fn new() -> Result<Self, ServiceInitError> {
        info!("Initializing service components");

        let (event_tx, event_rx) = mpsc::channel(128);

        debug!("Creating template manager");
        let config = get_config();
        let template_manager = template::TemplateManager::new(&config)?;

        debug!("Creating player manager");
        let player_manager = Arc::new(Mutex::new(PlayerManager::new(
            event_tx.clone(),
        )?));

        debug!("Creating presence manager");
        let presence_manager =
            presence::PresenceManager::new(template_manager, player_manager.clone())?;

        info!("Service initialization complete");
        Ok(Self {
            player_manager,
            presence_manager,
            event_rx,
            event_tx,
            config_rx: get_config().subscribe(),
            pending_events: SmallVec::new(),
        })
    }

    async fn check_players(&self) -> Result<(), ServiceRuntimeError> {
        let mut player_manager = self.player_manager.lock().await;
        Ok(player_manager.check_players().await?)
    }

    async fn handle_event(&mut self, event: Event) -> Result<(), ServiceRuntimeError> {
        debug!("Handling event: {:?}", event);
        match event {
            Event::PlayerUpdate(id, state) => {
                if let Err(e) = self.presence_manager.update_presence(&id, &state).await {
                    error!("Failed to update presence: {}", e);
                }
            }
            Event::PlayerRemove(id) => {
                if let Err(e) = self.presence_manager.remove_presence(&id) {
                    error!("Failed to remove presence: {}", e);
                }
            }
            Event::ClearActivity(id) => {
                if let Err(e) = self.presence_manager.clear_activity(&id) {
                    error!("Failed to clear activity: {}", e);
                }
            }
            Event::ConfigChanged => {
                debug!("Handling config change event");
                if let Err(e) = self.reload_components().await {
                    error!("Failed to reload components: {}", e);
                }

                // After reloading components, update all active players
                if let Err(e) = self.check_players().await {
                    error!("Failed to check players after config change: {}", e);
                }
            }
        }
        Ok(())
    }

    pub async fn run(&mut self) -> Result<(), ServiceRuntimeError> {
        info!("Starting service main loop");

        let mut interval =
            tokio::time::interval(Duration::from_millis(get_config().interval()));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    trace!("Checking players");
                    if let Err(e) = self.check_players().await {
                        error!("Error checking players: {}", e);
                    }
                },

                Ok(change) = self.config_rx.recv() => {
                    match change {
                        config::ConfigChange::Updated | config::ConfigChange::Reloaded => {
                            info!("Config change detected");
                            interval = tokio::time::interval(Duration::from_millis(get_config().interval()));

                            if let Err(e) = self.event_tx.send(Event::ConfigChanged).await {
                                error!("Failed to send config changed event: {}", e);
                            }
                        },
                        config::ConfigChange::Error(e) => {
                            error!("Config error: {}", e);
                        }
                    }
                },

                Some(event) = self.event_rx.recv() => {
                    debug!("Received event: {}", event);

                    // Add first event to SmallVec
                    self.pending_events.push(event);

                    // Try to collect more events
                    while let Ok(event) = self.event_rx.try_recv() {
                        debug!("Batched event: {}", event);
                        self.pending_events.push(event);
                        if self.pending_events.len() >= 10 {
                            break;
                        }
                    }

                    // Take events out of pending_events to avoid multiple mutable borrows
                    let events: SmallVec<[Event; 16]> = self.pending_events.drain(..).collect();
                    for event in events {
                        if let Err(e) = self.handle_event(event).await {
                            error!("Error handling event: {}", e);
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

    async fn reload_components(&mut self) -> Result<(), ServiceRuntimeError> {
        debug!("Reloading service components based on configuration changes");
        let config = get_config();

        // Only create a new template manager and pass it to presence manager
        let template_manager = template::TemplateManager::new(&config)?;

        // Update presence manager with new templates
        if let Err(e) = self.presence_manager.update_templates(template_manager) {
            error!("Failed to update templates: {}", e);
        }

        // Reload presence manager config
        if let Err(e) = self.presence_manager.reload_config() {
            error!("Failed to reload presence manager config: {}", e);
        }

        Ok(())
    }
}
