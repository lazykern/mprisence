use std::collections::HashMap;

use discord_presence::{
    models::Activity,
    Client as DiscordClient,
};
use log::{debug, error, info, warn};

use crate::{
    config::get_config,
    error::PresenceError,
    player::PlayerId,
};

pub struct PresenceManager {
    discord_clients: HashMap<PlayerId, DiscordClient>,
    has_activity: HashMap<PlayerId, bool>,
}

impl PresenceManager {
    pub fn new() -> Result<Self, PresenceError> {
        info!("Initializing PresenceManager");

        Ok(Self {
            discord_clients: HashMap::new(),
            has_activity: HashMap::new(),
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

        if let Some(_client) = self.discord_clients.remove(player_id) {
            debug!("Removed Discord client for player: {}", player_id);
        }

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
}
