use std::{
    collections::HashMap,
    sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex},
    time::{Duration, Instant},
};

use discord_presence::{models::Activity, Client as DiscordClient};
use log::{debug, error, info, warn};

use crate::{config::get_config, error::PresenceError, player::PlayerId};

pub struct DiscordClientState {
    client: Arc<Mutex<DiscordClient>>,
    is_ready: Arc<AtomicBool>,
    last_used: Instant,
}

pub struct PresenceManager {
    discord_clients: HashMap<PlayerId, DiscordClientState>,
    has_activity: HashMap<PlayerId, bool>,
    client_timeout: Duration,
}

impl PresenceManager {
    pub fn new() -> Result<Self, PresenceError> {
        info!("Initializing PresenceManager");
        Ok(Self {
            discord_clients: HashMap::new(),
            has_activity: HashMap::new(),
            client_timeout: Duration::from_secs(300), // 5 minutes timeout
        })
    }

    pub async fn update_presence(
        &mut self,
        player_id: &PlayerId,
        activity: Activity,
    ) -> Result<(), PresenceError> {
        self.has_activity.insert(player_id.clone(), true);

        let config = get_config();
        let player_config = config.player_config(player_id.identity.as_str());

        self.update_activity(player_id, activity, &player_config.app_id).await
    }

    async fn update_activity(
        &mut self,
        player_id: &PlayerId,
        activity: Activity,
        app_id: &str,
    ) -> Result<(), PresenceError> {
        debug!("Updating activity for player: {}", player_id);

        // Get or create the Discord client
        if !self.discord_clients.contains_key(player_id) {
            let client_state = self.create_client_state(app_id, activity.clone())?;
            self.discord_clients.insert(player_id.clone(), client_state);
        } else if let Some(client_state) = self.discord_clients.get_mut(player_id) {
            client_state.last_used = Instant::now();

            if client_state.is_ready.load(Ordering::Relaxed) {
                if let Ok(mut client) = client_state.client.lock() {
                    client.set_activity(|_| activity)
                        .map_err(|e| PresenceError::Update(format!("Failed to update presence: {}", e)))?;
                }
            }
        }

        Ok(())
    }

    pub fn clear_activity(&mut self, player_id: &PlayerId) -> Result<(), PresenceError> {
        if self.has_activity.get(player_id).copied().unwrap_or(false) {
            if let Some(client_state) = self.discord_clients.get_mut(player_id) {
                if let Ok(mut client) = client_state.client.lock() {
                    if let Err(e) = client.clear_activity() {
                        warn!("Failed to clear activity for {}: {}", player_id, e);
                    } else {
                        debug!("Cleared activity for {}", player_id);
                        self.has_activity.insert(player_id.clone(), false);
                    }
                }
            }
        }
        Ok(())
    }

    fn create_client_state(&self, app_id: &str, initial_activity: Activity) -> Result<DiscordClientState, PresenceError> {
        debug!("Creating new Discord client with app_id: {}", app_id);

        let app_id_u64 = app_id
            .parse::<u64>()
            .map_err(|e| PresenceError::Connection(format!("Invalid app_id: {}", e)))?;

        let client = Arc::new(Mutex::new(DiscordClient::new(app_id_u64)));
        let is_ready = Arc::new(AtomicBool::new(false));
        
        // Setup handlers - just handle state changes
        if let Ok(mut discord_client) = client.lock() {
            discord_client.on_ready({
                let ready_flag = is_ready.clone();
                let client_for_ready = client.clone();
                let activity = initial_activity;
                move |ctx| {
                    info!("Discord client ready: {:?}", ctx);
                    ready_flag.store(true, Ordering::Release);
                    
                    // Apply initial activity immediately when ready
                    debug!("Applying initial activity");
                    if let Ok(mut client) = client_for_ready.lock() {
                        if let Err(e) = client.set_activity(|_| activity.clone()) {
                            error!("Failed to apply initial activity: {}", e);
                        }
                    }
                }
            }).persist();

            discord_client.on_connected(|ctx| {
                info!("Discord client connected: {:?}", ctx);
            }).persist();

            discord_client.on_disconnected({
                let ready_flag = is_ready.clone();
                move |_| {
                    info!("Discord client disconnected");
                    ready_flag.store(false, Ordering::Release);
                }
            }).persist();

            discord_client.on_error({
                let ready_flag = is_ready.clone();
                move |ctx| {
                    error!("Discord error: {:?}", ctx);
                    ready_flag.store(false, Ordering::Release);
                }
            }).persist();

            discord_client.start();
        }

        Ok(DiscordClientState {
            client,
            is_ready,
            last_used: Instant::now(),
        })
    }

    pub async fn cleanup_inactive_clients(&mut self) {
        let now = Instant::now();
        let timeout = self.client_timeout;

        // Only remove clients that are both inactive AND have no activity
        let to_remove: Vec<_> = self.discord_clients
            .iter()
            .filter(|(player_id, state)| {
                let is_inactive = now.duration_since(state.last_used) > timeout;
                let has_no_activity = !self.has_activity.get(player_id).copied().unwrap_or(false);
                is_inactive && has_no_activity
            })
            .map(|(id, _)| id.clone())
            .collect();

        // Remove each inactive client that has no activity
        for id in to_remove {
            debug!("Cleaning up inactive Discord client for {} (no activity)", id);
            // No need to clear activity since we only remove clients without activity
            self.discord_clients.remove(&id);
        }
    }

    pub fn remove_presence(&mut self, player_id: &PlayerId) -> Result<(), PresenceError> {
        debug!("Removing Discord client for player: {}", player_id);
        self.has_activity.remove(player_id);

        if let Some(state) = self.discord_clients.remove(player_id) {
            // Clear activity before removing
            if let Ok(mut client) = state.client.lock() {
                if let Err(e) = client.clear_activity() {
                    warn!("Error clearing activity for {}: {}", player_id, e);
                }
            }
            debug!("Removed Discord client for player: {}", player_id);
        }

        Ok(())
    }

    // Simplify to just cleanup
    pub async fn check_clients(&mut self) {
        self.cleanup_inactive_clients().await;
    }
}
