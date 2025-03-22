use std::{
    collections::HashMap,
    sync::Arc,
};

use discord_presence::{
    models::Activity,
    Client as DiscordClient,
};
use log::{debug, error, info, warn};
use mpris::Metadata;
use tokio::sync::Mutex;

use crate::{
    config::get_config,
    cover::CoverArtManager,
    error::PresenceError,
    player::{PlayerId, PlayerState, PlayerManager},
    template,
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
        activity: Activity,
    ) -> Result<(), PresenceError> {
        // Save player state for later reference
        self.has_activity.insert(player_id.clone(), true);

        let config = get_config();
        let player_config = config.player_config(player_id.identity.as_str());

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

    // Accessor methods for Service
    pub fn get_template_manager(&self) -> &template::TemplateManager {
        &self.template_manager
    }

    pub async fn get_cover_art_url(&self, metadata: &Metadata) -> Result<Option<String>, PresenceError> {
        self.cover_art_manager.get_cover_art(metadata).await.map_err(|e| {
            PresenceError::General(format!("Failed to get cover art: {}", e))
        })
    }
}
