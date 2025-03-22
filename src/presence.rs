use std::{collections::HashMap, sync::Arc};

use discord_presence::Client as DiscordClient;
use log::{debug, error, info, trace, warn};
use mpris::PlaybackStatus;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;

use crate::{config::{self, get_config}, cover::CoverArtManager, error::PresenceError, player::{self, PlayerId, PlayerManager, PlayerState}, template, utils};

use super::utils::format_duration;

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

        // Get cover art using full metadata
        let cover_art_url = match self.cover_art_manager.get_cover_art(&full_metadata).await {
            Ok(url) => url,
            Err(e) => {
                warn!("Failed to get cover art: {}", e);
                None
            }
        };

        // Create template data with additional metadata
        let mut template_data = template::TemplateManager::create_data(player_id, state);

        // Add additional metadata fields
        if let Some(length) = full_metadata.length() {
            template_data.insert("length".to_string(), format_duration(length.as_secs()));
        }
        if let Some(track_number) = full_metadata.track_number() {
            template_data.insert("track_number".to_string(), track_number.to_string());
        }
        if let Some(disc_number) = full_metadata.disc_number() {
            template_data.insert("disc_number".to_string(), disc_number.to_string());
        }
        if let Some(album_name) = full_metadata.album_name() {
            template_data.insert("album".to_string(), album_name.to_string());
        }
        if let Some(album_artists) = full_metadata.album_artists() {
            template_data.insert("album_artists".to_string(), album_artists.join(", "));
        }

        // Render templates with full metadata
        let details = self
            .template_manager
            .render("detail", &template_data)
            .map_err(|e| PresenceError::Update(format!("Template render error: {}", e)))?;

        let state_text = self
            .template_manager
            .render("state", &template_data)
            .map_err(|e| PresenceError::Update(format!("Template render error: {}", e)))?;

        let large_text = self
            .template_manager
            .render("large_text", &template_data)
            .map_err(|e| PresenceError::Update(format!("Template render error: {}", e)))?;

        let small_text = self
            .template_manager
            .render("small_text", &template_data)
            .map_err(|e| PresenceError::Update(format!("Template render error: {}", e)))?;

        // Determine content type from full metadata
        let content_type = utils::get_content_type_from_metadata(&full_metadata);

        // Determine activity type based on content type or player configuration
        let activity_type = player_config.activity_type(content_type.as_deref());

        trace!("Preparing Discord activity update: {}", details);

        self.update_activity(
            player_id,
            details,
            state_text,
            large_text,
            small_text,
            activity_type,
            state.playback_status,
            Duration::from_secs(state.position as u64),
            full_metadata.length().unwrap_or_default(),
            as_elapsed,
            cover_art_url,
            player_config.show_icon,
            player_config.icon.clone(),
            &player_config.app_id,
        )
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

        if let Some(client) = self.discord_clients.remove(player_id) {
            debug!("Removed Discord client for player: {}", player_id);
        }

        Ok(())
    }

    pub fn reload_config(&mut self) -> Result<(), PresenceError> {
        debug!("Reloading config in presence manager");
        let config = get_config();
        self.template_manager.reload(&config).map_err(|e| PresenceError::Update(format!("Failed to reload templates: {}", e)))?;
        Ok(())
    }

    fn update_activity(
        &mut self,
        player_id: &player::PlayerId,
        details: String,
        state: String,
        large_text: String,
        small_text: String,
        activity_type: config::ActivityType,
        playback_status: PlaybackStatus,
        position: Duration,
        length: Duration,
        show_elapsed: bool,
        large_image: Option<String>,
        show_small_image: bool,
        small_image: String,
        app_id: &str,
    ) -> Result<(), PresenceError> {
        debug!("Updating activity for player: {}", player_id);

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

        // Calculate timestamps
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards");

        let start_dur = now.checked_sub(position).unwrap_or_default();
        let start_s = start_dur.as_secs() as i64;

        let mut end_s = None;
        if !show_elapsed {
            let end = start_dur.checked_add(length).unwrap_or_default();
            end_s = Some(end.as_secs() as i64);
        }

        // Set the activity using the builder pattern
        client
            .set_activity(|mut act| {
                act = act._type(activity_type.into());

                // Set details and state if not empty
                if !details.is_empty() {
                    act = act.details(&details);
                }

                if !state.is_empty() {
                    act = act.state(&state);
                }

                // Set timestamps if playing
                if playback_status == PlaybackStatus::Playing {
                    act = act.timestamps(|ts| {
                        if let Some(end) = end_s {
                            ts.start(start_s as u64).end(end as u64)
                        } else {
                            ts.start(start_s as u64)
                        }
                    });
                }

                // Set assets (images and their tooltips)
                act = act.assets(|a| {
                    let mut assets = a;

                    // Set large image (album art) if available
                    if let Some(img_url) = &large_image {
                        assets = assets.large_image(img_url);
                        if !large_text.is_empty() {
                            assets = assets.large_text(&large_text);
                        }
                    }

                    // Set small image (player icon) if enabled
                    if show_small_image {
                        assets = assets.small_image(&small_image);
                        if !small_text.is_empty() {
                            assets = assets.small_text(&small_text);
                        }
                    }

                    assets
                });

                act
            })
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
