use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use discord_presence::models::Activity;
use log::{debug, error, info, trace, warn};
use mpris::{Metadata, PlaybackStatus};
use smallvec::SmallVec;
use tokio::sync::{mpsc, Mutex};

use crate::{
    config::{self, get_config},
    error::{ServiceInitError, ServiceRuntimeError},
    event::Event,
    player::{PlayerManager, PlayerId, PlayerState},
    presence,
    template,
    utils,
};

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
                // Get full metadata
                let metadata = {
                    let player_manager = self.player_manager.lock().await;
                    player_manager
                        .get_metadata(&id)
                        .map_err(|e| ServiceRuntimeError::General(format!("Failed to get metadata: {}", e)))?
                };

                // Create activity
                let activity = self.create_activity(&id, &state, &metadata).await?;

                // Update presence with activity
                if let Err(e) = self.presence_manager.update_presence(&id, activity).await {
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

    async fn create_activity(
        &self,
        player_id: &PlayerId,
        state: &PlayerState,
        metadata: &Metadata,
    ) -> Result<Activity, ServiceRuntimeError> {
        // Don't show activity if player is stopped
        if state.playback_status == PlaybackStatus::Stopped {
            return Ok(Activity::default());
        }

        let config = get_config();
        let player_config = config.player_config(player_id.identity.as_str());
        let as_elapsed = config.time_config().as_elapsed;

        let length = metadata.length().unwrap_or_default();

        // Calculate timestamps if playing
        let (start_s, end_s) = if state.playback_status == PlaybackStatus::Playing {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards");

            let start_dur = now.checked_sub(Duration::from_secs(state.position as u64)).unwrap_or_default();
            let start_s = start_dur.as_secs() as i64;

            let mut end_s = None;
            if !as_elapsed && !length.is_zero() {
                let end = start_dur
                    .checked_add(length)
                    .unwrap_or_default();
                end_s = Some(end.as_secs() as i64);
            }

            (Some(start_s as u64), end_s.map(|s| s as u64))
        } else {
            (None, None)
        };

        let mut activity = Activity::default();

        let content_type = utils::get_content_type_from_metadata(metadata);
        let activity_type = player_config.activity_type(content_type.as_deref());

        activity = activity._type(activity_type.into());

        let activity_texts = self.presence_manager
            .get_template_manager()
            .render_activity_texts(
                player_id,
                state,
                metadata,
            )
            .map_err(|e| ServiceRuntimeError::General(format!("Activity creation error: {}", e)))?;

        if !activity_texts.details.is_empty() {
            activity = activity.details(&activity_texts.details);
        }

        if !activity_texts.state.is_empty() {
            activity = activity.state(&activity_texts.state);
        }

        if let Some(start) = start_s {
            activity = activity.timestamps(|ts| {
                if let Some(end) = end_s {
                    ts.start(start).end(end)
                } else {
                    ts.start(start)
                }
            });
        }

        // Get cover art URL from presence manager
        let cover_art_url = self.presence_manager.get_cover_art_url(metadata).await.unwrap_or_default();

        activity = activity.assets(|a| {
            let mut assets = a;

            // Set large image (album art) if available
            if let Some(img_url) = &cover_art_url {
                assets = assets.large_image(img_url);
                if !activity_texts.large_text.is_empty() {
                    assets = assets.large_text(&activity_texts.large_text);
                }
            }

            // Set small image (player icon) if enabled
            if player_config.show_icon {
                assets = assets.small_image(player_config.icon);
                if !activity_texts.small_text.is_empty() {
                    assets = assets.small_text(&activity_texts.small_text);
                }
            }

            assets
        });

        Ok(activity)
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
