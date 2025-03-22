use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use discord_presence::{
    models::{Activity, ActivityType as DiscordActivityType},
    Client as DiscordClient,
};
use log::{debug, error, info, trace, warn};
use mpris::PlaybackStatus;
use tokio::sync::Mutex;

use crate::{
    config::{self, get_config, ActivityType},
    cover::CoverArtManager,
    error::PresenceError,
    player::{PlayerId, PlayerManager, PlayerState},
    template, utils,
};

pub struct PresenceManager {
    discord_clients: HashMap<PlayerId, DiscordClient>,
    template_manager: template::TemplateManager,
    has_activity: HashMap<PlayerId, bool>,
    cover_art_manager: CoverArtManager,
    player_states: HashMap<PlayerId, PlayerState>,
    player_manager: Arc<Mutex<PlayerManager>>,
}

impl PresenceManager {
    pub fn new(
        template_manager: template::TemplateManager,
        player_manager: Arc<Mutex<PlayerManager>>,
    ) -> Result<Self, PresenceError> {
        info!("Initializing PresenceManager");
        let config = get_config();

        let cover_art_manager = CoverArtManager::new(&config).map_err(|e| {
            PresenceError::General(format!("Failed to initialize cover art manager: {}", e))
        })?;

        Ok(Self {
            discord_clients: HashMap::new(),
            template_manager,
            has_activity: HashMap::new(),
            cover_art_manager,
            player_states: HashMap::new(),
            player_manager,
        })
    }

    pub async fn update_presence(
        &mut self,
        player_id: &PlayerId,
        state: &PlayerState,
    ) -> Result<(), PresenceError> {
        // Don't show activity if player is stopped
        if state.playback_status == PlaybackStatus::Stopped {
            return self.clear_activity(player_id);
        }

        // Save player state for later reference
        self.player_states.insert(player_id.clone(), state.clone());
        self.has_activity.insert(player_id.clone(), true);

        let config = get_config();
        let player_config = config.player_config(player_id.identity.as_str());
        let as_elapsed = config.time_config().as_elapsed;

        // Get full metadata on demand
        let full_metadata = {
            let player_manager = self.player_manager.lock().await;
            player_manager
                .get_metadata(player_id)
                .map_err(|e| PresenceError::Update(format!("Failed to get metadata: {}", e)))?
        };

        let length = full_metadata.length().unwrap_or_default();

        // Get cover art using full metadata
        let cover_art_url = match self.cover_art_manager.get_cover_art(&full_metadata).await {
            Ok(url) => url,
            Err(e) => {
                warn!("Failed to get cover art: {}", e);
                None
            }
        };

        // Create activity using template manager

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

        let content_type = utils::get_content_type_from_metadata(&full_metadata);
        let activity_type = player_config.activity_type(content_type.as_deref());

        activity = activity._type(activity_type.into());

        let activity_texts = self
            .template_manager
            .render_activity_texts(
                player_id,
                state,
                &full_metadata,
            )
            .map_err(|e| PresenceError::Update(format!("Activity creation error: {}", e)))?;

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

        // Apply activity to Discord
        self.update_activity(player_id, activity, &player_config.app_id)
    }

    pub fn clear_activity(&mut self, player_id: &PlayerId) -> Result<(), PresenceError> {
        if self.has_activity.get(player_id).copied().unwrap_or(false) {
            if let Some(client) = self.discord_clients.get_mut(player_id) {
                if let Err(e) = client.clear_activity() {
                    warn!("Failed to clear activity for {}: {}", player_id, e);
                } else {
                    debug!("Cleared activity for {}", player_id);
                    self.has_activity.insert(player_id.clone(), false);
                }
            }
        }
        Ok(())
    }

    pub fn remove_presence(&mut self, player_id: &PlayerId) -> Result<(), PresenceError> {
        debug!("Removing Discord client for player: {}", player_id);
        self.has_activity.remove(player_id);
        self.player_states.remove(player_id);

        if let Some(_client) = self.discord_clients.remove(player_id) {
            debug!("Removed Discord client for player: {}", player_id);
        }

        Ok(())
    }

    pub fn reload_config(&mut self) -> Result<(), PresenceError> {
        debug!("Reloading config in presence manager");
        let config = get_config();
        self.template_manager
            .reload(&config)
            .map_err(|e| PresenceError::Update(format!("Failed to reload templates: {}", e)))?;
        Ok(())
    }

    // Method to update activity with Discord-specific logic
    fn update_activity(
        &mut self,
        player_id: &PlayerId,
        activity: Activity,
        app_id: &str,
    ) -> Result<(), PresenceError> {
        debug!("Updating activity for player: {}", player_id);

        // Get or create the Discord client
        if !self.discord_clients.contains_key(player_id) {
            match Self::create_client(app_id) {
                Ok(mut client) => {
                    // Setup error handler
                    client
                        .on_error(move |ctx| {
                            error!("Discord error: {:?}", ctx.event);
                        })
                        .persist();

                    // Start the client
                    client.start();

                    self.discord_clients.insert(player_id.clone(), client);
                }
                Err(e) => return Err(e),
            }
        }

        let client = self
            .discord_clients
            .get_mut(player_id)
            .ok_or_else(|| PresenceError::Update("Client unexpectedly missing".to_string()))?;


        // Set the activity using the builder pattern
        client
            .set_activity(|mut _act| activity)
            .map_err(|e| PresenceError::Update(format!("Failed to update presence: {}", e)))?;

        Ok(())
    }

    fn create_client(app_id: &str) -> Result<DiscordClient, PresenceError> {
        debug!("Creating new Discord client with app_id: {}", app_id);

        // Parse app_id from string to u64
        let app_id_u64 = app_id
            .parse::<u64>()
            .map_err(|e| PresenceError::Connection(format!("Invalid app_id: {}", e)))?;

        let client = DiscordClient::new(app_id_u64);
        info!("Successfully created Discord client");

        Ok(client)
    }

    pub fn update_templates(
        &mut self,
        new_templates: template::TemplateManager,
    ) -> Result<(), PresenceError> {
        debug!("Updating templates in presence manager");
        self.template_manager = new_templates;
        Ok(())
    }
}
