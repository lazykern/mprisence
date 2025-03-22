use log::trace;

use crate::error::{ServiceInitError, ServiceRuntimeError};

use super::*;

pub struct Service {
    player_manager: Arc<TokioMutex<player::PlayerManager>>,
    presence_manager: presence::PresenceManager,
    event_rx: mpsc::Receiver<event::Event>,
    event_tx: mpsc::Sender<event::Event>,
    config_rx: config::ConfigChangeReceiver,
    pending_events: SmallVec<[event::Event; 16]>,
}

impl Service {
    pub fn new() -> Result<Self, ServiceInitError> {
        info!("Initializing service components");

        let (event_tx, event_rx) = mpsc::channel(128);

        debug!("Creating template manager");
        let config = config::get();
        let template_manager = template::TemplateManager::new(&config)?;

        debug!("Creating player manager");
        let player_manager = Arc::new(TokioMutex::new(player::PlayerManager::new(
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
            config_rx: config::get().subscribe(),
            pending_events: SmallVec::new(),
        })
    }

    pub async fn run(&mut self) -> Result<(), ServiceRuntimeError> {
        info!("Starting service main loop");

        let mut interval =
            tokio::time::interval(Duration::from_millis(config::get().interval()));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    trace!("Checking players");
                    let mut player_manager = self.player_manager.lock().await;
                    if let Err(e) = player_manager.check_players().await {
                        error!("Error checking players: {}", e);
                    }
                },

                Ok(change) = self.config_rx.recv() => {
                    match change {
                        config::ConfigChange::Updated | config::ConfigChange::Reloaded => {
                            info!("Config change detected");
                            interval = tokio::time::interval(Duration::from_millis(config::get().interval()));

                            if let Err(e) = self.event_tx.send(event::Event::ConfigChanged).await {
                                error!("Failed to send config changed event: {}", e);
                            }

                            if let Err(e) = self.reload_components().await {
                                error!("Failed to reload components: {}", e);
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
                    for _ in 0..9 {
                        match self.event_rx.try_recv() {
                            Ok(event) => {
                                debug!("Batched event: {}", event);
                                self.pending_events.push(event);
                            },
                            Err(_) => break,
                        }
                    }

                    // Process all collected events
                    for event in self.pending_events.drain(..) {
                        trace!("Handling event: {:?}", event);
                        if let Err(e) = self.presence_manager.handle_event(event).await {
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
        let config = config::get();

        // Only create a new template manager and pass it to presence manager
        let template_manager = template::TemplateManager::new(&config)?;

        // Update presence manager with new templates instead of recreating it
        if let Err(e) = self.presence_manager.update_templates(template_manager) {
            error!("Failed to update templates: {}", e);
        }

        Ok(())
    }
}
