use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use crate::{
    config::{self, get_config},
    cover::CoverArtManager,
    error::{ServiceInitError, ServiceRuntimeError},
    player::{PlayerId, PlayerManager, PlayerState, PlayerStateChange},
    presence, template, utils,
};
use discord_presence::models::Activity;
use log::{debug, error, info, trace, warn};
use mpris::{Metadata, PlaybackStatus};
use tokio::sync::Mutex;

pub struct Service {
    player_manager: Arc<Mutex<PlayerManager>>,
    presence_manager: presence::PresenceManager,
    template_manager: template::TemplateManager,
    cover_art_manager: CoverArtManager,
    config_rx: config::ConfigChangeReceiver,
}

impl Service {
    pub fn new() -> Result<Self, ServiceInitError> {
        info!("Initializing service components");

        debug!("Creating template manager");
        let config = get_config();
        let template_manager = template::TemplateManager::new(&config)?;

        debug!("Creating player manager");
        let player_manager = Arc::new(Mutex::new(PlayerManager::new()?));

        debug!("Creating presence manager");
        let presence_manager = presence::PresenceManager::new()?;

        debug!("Creating cover art manager");
        let cover_art_manager = CoverArtManager::new(&config)?;

        info!("Service initialization complete");
        Ok(Self {
            player_manager,
            presence_manager,
            template_manager,
            cover_art_manager,
            config_rx: get_config().subscribe(),
        })
    }

    async fn check_players(&mut self) -> Result<(), ServiceRuntimeError> {
        let mut player_manager = self.player_manager.lock().await;
        let state_changes = player_manager.check_players().await?;

        // Process state changes
        for change in state_changes {
            match change {
                PlayerStateChange::Updated(id, state) => {
                    // Check if we should clear on pause
                    if state.playback_status == PlaybackStatus::Paused
                        && get_config().clear_on_pause()
                    {
                        if let Err(e) = self.presence_manager.clear_activity(&id) {
                            error!("Failed to clear activity: {}", e);
                        }
                        continue;
                    }

                    // Skip empty activities for stopped players
                    if state.playback_status == PlaybackStatus::Stopped {
                        debug!("Skipping stopped player: {}", id);
                        continue;
                    }

                    // Get full metadata
                    let metadata = player_manager.get_metadata(&id)?;

                    // Create activity
                    let activity = self.create_activity(&id, &state, &metadata).await?;

                    // Update presence with activity only if it contains data
                    if activity.details.is_some()
                        || activity.state.is_some()
                        || activity.assets.is_some()
                    {
                        if let Err(e) = self.presence_manager.update_presence(&id, activity).await {
                            error!("Failed to update presence: {}", e);
                        }
                    } else {
                        debug!("Skipping empty activity for player: {}", id);
                    }
                }
                PlayerStateChange::Removed(id) => {
                    if let Err(e) = self.presence_manager.remove_presence(&id) {
                        error!("Failed to remove presence: {}", e);
                    }
                }
                PlayerStateChange::Cleared(id) => {
                    if let Err(e) = self.presence_manager.clear_activity(&id) {
                        error!("Failed to clear activity: {}", e);
                    }
                }
            }
        }

        Ok(())
    }

    async fn handle_config_change(&mut self) -> Result<(), ServiceRuntimeError> {
        debug!("Handling config change");
        // Reload components that depend on config
        if let Err(e) = self.template_manager.reload(&get_config()) {
            error!("Failed to reload templates: {}", e);
        }

        // Update all active players to reflect new config
        self.check_players().await
    }

    async fn create_activity(
        &self,
        player_id: &PlayerId,
        state: &PlayerState,
        metadata: &Metadata,
    ) -> Result<Activity, ServiceRuntimeError> {
        // Don't show activity if player is stopped
        if state.playback_status == PlaybackStatus::Stopped {
            debug!("Player is stopped, returning empty activity");
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

            let start_dur = now
                .checked_sub(Duration::from_secs(
                    state.position.unwrap_or_default() as u64
                ))
                .unwrap_or_default();
            let start_s = start_dur.as_secs() as i64;

            let mut end_s = None;
            if !as_elapsed && !length.is_zero() {
                let end = start_dur.checked_add(length).unwrap_or_default();
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

        let activity_texts = self
            .template_manager
            .render_activity_texts(player_id, state, metadata)?;

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

        // Get cover art URL using cover art manager
        let cover_art_url = match self.cover_art_manager.get_cover_art(metadata).await {
            Ok(Some(url)) => {
                info!("Found cover art URL for Discord");
                debug!("Using cover art URL: {}", url);
                Some(url)
            }
            Ok(None) => {
                debug!("No cover art URL available for Discord");
                None
            }
            Err(e) => {
                warn!("Failed to get cover art: {}", e);
                debug!(
                    "Discord requires HTTP/HTTPS URLs for images, not file paths or base64 data"
                );
                None
            }
        };

        activity = activity.assets(|a| {
            let mut assets = a;

            // Set large image (album art) if available
            if let Some(img_url) = &cover_art_url {
                debug!("Setting Discord large image to: {}", img_url);
                assets = assets.large_image(img_url);
                if !activity_texts.large_text.is_empty() {
                    assets = assets.large_text(&activity_texts.large_text);
                }
            }

            // Set small image (player icon) if enabled
            if player_config.show_icon {
                debug!(
                    "Setting Discord small image to player icon: {}",
                    player_config.icon
                );
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

        let mut interval = tokio::time::interval(Duration::from_millis(get_config().interval()));
        let mut client_check_interval = tokio::time::interval(Duration::from_secs(10)); // Check more frequently

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    trace!("Checking players");
                    if let Err(e) = self.check_players().await {
                        error!("Error checking players: {}", e);
                    }
                },

                _ = client_check_interval.tick() => {
                    self.presence_manager.check_clients().await;
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
