use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering, AtomicUsize},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

use discord_presence::{models::Activity, Client as DiscordClient, Event};
use log::{debug, error, info, warn};

use crate::{config::get_config, error::PresenceError, player::PlayerId};

#[derive(Clone)]
pub struct DiscordClientState {
    client: Arc<Mutex<DiscordClient>>,
    got_ready_event: Arc<AtomicBool>,
    connecting: Arc<AtomicBool>,
    connected: Arc<AtomicBool>,
    activity: Arc<Mutex<Option<Activity>>>,
    pending_activity: Arc<Mutex<Option<Activity>>>,
    error_occurred: Arc<AtomicBool>,
    last_used: Arc<Mutex<Instant>>,
    last_error: Arc<Mutex<Instant>>,
    reconnect_attempts: Arc<AtomicUsize>,
}

pub struct PresenceManager {
    discord_clients: HashMap<PlayerId, DiscordClientState>,
    client_timeout: Duration,
}

impl DiscordClientState {
    fn set_activity(&self, activity: Activity) -> Result<(), PresenceError> {
        // Always store the activity, whether we can apply it now or not
        if let Ok(mut current_activity) = self.activity.lock() {
            *current_activity = Some(activity.clone());
        }

        if self.got_ready_event.load(Ordering::Relaxed) {
            if let Ok(mut client) = self.client.lock() {
                debug!("Setting activity immediately");
                let payload = client.set_activity(|_| activity.clone()).map_err(|e| {
                    PresenceError::Update(format!("Failed to update presence: {}", e))
                })?;
                Ok(())
            } else {
                Err(PresenceError::Update("Failed to lock client".to_string()))
            }
        } else {
            // Store as pending activity if client is not ready
            debug!("Storing activity as pending");
            if let Ok(mut pending) = self.pending_activity.lock() {
                *pending = Some(activity);
                Ok(())
            } else {
                Err(PresenceError::Update("Failed to store pending activity".to_string()))
            }
        }
    }

    fn clear_activity(&self) -> Result<(), PresenceError> {
        if let Ok(mut client) = self.client.lock() {
            let payload = client.clear_activity()?;
            if let Ok(mut current_activity) = self.activity.lock() {
                *current_activity = payload.data;
            }
            
            // Also clear any pending activity
            if let Ok(mut pending) = self.pending_activity.lock() {
                *pending = None;
            }
            
            Ok(())
        } else {
            Err(PresenceError::Update("Failed to lock client".to_string()))
        }
    }

    fn apply_pending_activity(&self) {
        if let Ok(mut pending_guard) = self.pending_activity.lock() {
            if let Some(activity) = pending_guard.take() {
                debug!("Applying pending activity");
                if let Ok(mut client) = self.client.lock() {
                    if let Err(e) = client.set_activity(|_| activity) {
                        error!("Failed to apply pending activity: {}", e);
                    }
                }
            }
        }
    }

    fn start(&self) {
        if let Ok(mut client) = self.client.lock() {
            if !self.connecting.load(Ordering::Relaxed) {
                self.connecting.store(true, Ordering::Release);
                let attempts = self.reconnect_attempts.fetch_add(1, Ordering::Relaxed);
                debug!("Starting Discord client (attempt {})", attempts + 1);
                client.start();
            }
        }
    }

    fn shutdown(&self) -> Result<(), PresenceError> {
        let _ = self.client.lock().unwrap().clone().shutdown().map_err(|e| PresenceError::Update(format!("Failed to shutdown client: {}", e)))?;
        self.connecting.store(false, Ordering::Release);
        Ok(())
    }

    fn update_last_used(&self) {
        if let Ok(mut last_used) = self.last_used.lock() {
            *last_used = Instant::now();
        }
    }

    fn get_last_used(&self) -> Option<Instant> {
        self.last_used.lock().ok().map(|guard| *guard)
    }

    fn got_ready_event(&self) -> bool {
        self.got_ready_event.load(Ordering::Relaxed)
    }

    fn has_error(&self) -> bool {
        self.error_occurred.load(Ordering::Relaxed)
    }

    fn mark_ready(&self) {
        self.got_ready_event.store(true, Ordering::Release);
        self.error_occurred.store(false, Ordering::Release);
    }

    fn mark_error(&self) {
        self.got_ready_event.store(false, Ordering::Release);
        self.error_occurred.store(true, Ordering::Release);
    }

    fn activity(&self) -> Option<Activity> {
        self.activity.lock().map(|guard| guard.clone()).unwrap_or(None)
    }

    fn should_attempt_reconnect(&self) -> bool {
        let now = Instant::now();
        let attempts = self.reconnect_attempts.load(Ordering::Relaxed);
        
        // Get time since last error
        let error_delay = if let Ok(last_error) = self.last_error.lock() {
            now.duration_since(*last_error)
        } else {
            Duration::from_secs(0)
        };

        // Exponential backoff: wait longer between attempts as the number of attempts increases
        let required_delay = Duration::from_secs(2u64.pow(attempts.min(6) as u32));
        
        !self.connected.load(Ordering::Relaxed) 
            && !self.connecting.load(Ordering::Relaxed)
            && error_delay >= required_delay
    }

    fn new(app_id: u64, initial_activity: Activity) -> Result<Self, PresenceError> {
        debug!("Creating new Discord client with app_id: {}", app_id);

        let client = Arc::new(Mutex::new(DiscordClient::new(app_id)));
        let is_ready = Arc::new(AtomicBool::new(false));
        let error_occurred = Arc::new(AtomicBool::new(false));
        let activity = Arc::new(Mutex::new(None));
        let pending_activity = Arc::new(Mutex::new(Some(initial_activity)));
        let last_used = Arc::new(Mutex::new(Instant::now()));
        let last_error = Arc::new(Mutex::new(Instant::now()));
        let connecting = Arc::new(AtomicBool::new(false));
        let connected = Arc::new(AtomicBool::new(false));
        let got_ready_event = Arc::new(AtomicBool::new(false));
        let reconnect_attempts = Arc::new(AtomicUsize::new(0));

        // Setup handlers before creating the final state
        if let Ok(discord_client) = client.lock() {
            discord_client
                .on_ready({
                    let error_occurred = error_occurred.clone();
                    let client = client.clone();
                    let pending_activity = pending_activity.clone();
                    let activity = activity.clone();
                    let got_ready_event = got_ready_event.clone();
                    let reconnect_attempts = reconnect_attempts.clone();
                    let connected = connected.clone();
                    move |ctx| {
                        info!("Discord client ready: {:?}", ctx);
                        got_ready_event.store(true, Ordering::Release);
                        error_occurred.store(false, Ordering::Release);
                        reconnect_attempts.store(0, Ordering::Release);
                        connected.store(true, Ordering::Release);

                        // First try to apply any pending activity
                        let mut activity_to_apply = None;
                        if let Ok(mut pending_guard) = pending_activity.lock() {
                            activity_to_apply = pending_guard.take();
                        }

                        // If no pending activity, try to use the last known activity
                        if activity_to_apply.is_none() {
                            if let Ok(current_activity) = activity.lock() {
                                activity_to_apply = current_activity.clone();
                            }
                        }

                        // Apply the activity if we have one
                        if let Some(activity_to_apply) = activity_to_apply {
                            debug!("Applying activity after ready event");
                            if let Ok(mut client) = client.lock() {
                                match client.set_activity(|_| activity_to_apply.clone()) {
                                    Ok(payload) => {
                                        if let Ok(mut current_activity) = activity.lock() {
                                            *current_activity = payload.data;
                                        }
                                    }
                                    Err(e) => {
                                        error!("Failed to apply activity after ready: {}", e);
                                    }
                                }
                            }
                        }
                    }
                })
                .persist();

            discord_client
                .on_connected({
                    let connected = connected.clone();
                    let connecting = connecting.clone();
                    let got_ready_event = got_ready_event.clone();
                    move |ctx| {
                        info!("Discord connected event: {:?}", ctx);
                        connected.store(true, Ordering::Release);
                        connecting.store(false, Ordering::Release);
                    }
                })
                .persist();

            discord_client
                .on_disconnected({
                    let connected = connected.clone();
                    let connecting = connecting.clone();
                    let got_ready_event = got_ready_event.clone();
                    move |_| {
                        info!("Discord disconnected event");
                        connected.store(false, Ordering::Release);
                        connecting.store(false, Ordering::Release);
                        got_ready_event.store(false, Ordering::Release);
                    }
                })
                .persist();

            discord_client
                .on_error({
                    let error_occurred = error_occurred.clone();
                    let last_error = last_error.clone();
                    let got_ready_event = got_ready_event.clone();
                    let connected = connected.clone();
                    let connecting = connecting.clone();
                    move |ctx| {
                        error!("Discord error event: {:?}", ctx);
                        error_occurred.store(true, Ordering::Release);
                        got_ready_event.store(false, Ordering::Release);
                        connected.store(false, Ordering::Release);
                        connecting.store(false, Ordering::Release);
                        if let Ok(mut last_error) = last_error.lock() {
                            *last_error = Instant::now();
                        }
                    }
                })
                .persist();
        } else {
            return Err(PresenceError::Update("Failed to lock client for handler setup".to_string()));
        }

        Ok(Self {
            client,
            connecting,
            connected,
            activity,
            got_ready_event,
            pending_activity,
            error_occurred,
            last_used,
            last_error,
            reconnect_attempts,
        })
    }
}

impl PresenceManager {
    pub fn new() -> Result<Self, PresenceError> {
        info!("Initializing PresenceManager");
        Ok(Self {
            discord_clients: HashMap::new(),
            client_timeout: Duration::from_secs(300), // 5 minutes timeout
        })
    }

    pub async fn update_presence(
        &mut self,
        player_id: &PlayerId,
        activity: Activity,
    ) -> Result<(), PresenceError> {

        let config = get_config();
        let player_config = config.player_config(player_id.identity.as_str());
        let app_id: u64 = player_config
            .app_id
            .parse()
            .map_err(|e| PresenceError::Update(format!("Invalid app ID: {}", e)))?;

        self.update_activity(player_id, activity, app_id).await?;

        Ok(())
    }

    async fn update_activity(
        &mut self,
        player_id: &PlayerId,
        activity: Activity,
        app_id: u64,
    ) -> Result<(), PresenceError> {
        debug!("Updating activity for {} with {:?}", player_id, activity);

        // Get or create the Discord client
        if !self.discord_clients.contains_key(player_id) {
            let client_state = DiscordClientState::new(app_id, activity.clone())?;
            client_state.start();
            self.discord_clients.insert(player_id.clone(), client_state);
        } else if let Some(client_state) = self.discord_clients.get(player_id) {
            client_state.update_last_used();
            
            // If client is in error state, try to restart it
            if client_state.has_error() {
                debug!("Restarting client that had an error: {}", player_id);
                client_state.error_occurred.store(false, Ordering::Release);
                client_state.start();
                // Store as pending activity to be applied when ready
                if let Ok(mut pending) = client_state.pending_activity.lock() {
                    *pending = Some(activity.clone());
                }
            }
            
            client_state.set_activity(activity)?;
        }

        Ok(())
    }

    pub fn clear_activity(&mut self, player_id: &PlayerId) -> Result<(), PresenceError> {
        if let Some(client_state) = self.discord_clients.get_mut(player_id) {
            client_state.clear_activity()?;
            debug!("Cleared activity for {}", player_id);
        }
        Ok(())
    }

    pub async fn cleanup_inactive_clients(&mut self) {
        let now = Instant::now();
        let timeout = self.client_timeout;

        // Remove clients that are either inactive, have no activity, or have encountered persistent errors
        let to_remove: Vec<_> = self
            .discord_clients
            .iter()
            .filter(|(id, state)| {
                let timeout_without_activity = state
                    .get_last_used()
                    .map(|last_used| now.duration_since(last_used) > timeout)
                    .unwrap_or(true) && state.activity().is_none();
                
                // Only remove clients with errors if they've been in error state for a while
                let persistent_error = state.has_error() && state
                    .get_last_used()
                    .map(|last_used| now.duration_since(last_used) > Duration::from_secs(60))
                    .unwrap_or(true);
                
                let is_connecting = state.connecting.load(Ordering::Relaxed);

                (!is_connecting && timeout_without_activity) || persistent_error
            })
            .map(|(id, _)| id.clone())
            .collect();

        // Remove each client that meets the removal criteria
        for id in to_remove {
            if let Some(client) = self.discord_clients.remove(&id) {
                if client.has_error() {
                    debug!("Removing client due to persistent error: {}", id);
                } else {
                    debug!("Removing client due to timeout without activity: {}", id);
                }
                let _ = client.shutdown();
            }
        }
    }

    pub fn remove_presence(&mut self, player_id: &PlayerId) -> Result<(), PresenceError> {
        let client = self.discord_clients.remove(player_id);
        if let Some(client) = client {
            let _ = client.shutdown();
        }

        Ok(())
    }

    pub async fn check_clients(&mut self) {
        self.cleanup_inactive_clients().await;
        
        // Re-apply activities for all active clients to handle Discord reconnection
        for (id, client_state) in &self.discord_clients {
            // Check if we should attempt reconnection
            if client_state.should_attempt_reconnect() {
                debug!("Attempting to reconnect client: {} (attempt {})", id, 
                    client_state.reconnect_attempts.load(Ordering::Relaxed) + 1);
                client_state.start();
                continue;
            }
            
            // Only re-apply activity if client is ready
            if client_state.got_ready_event() {
                if let Some(activity) = client_state.activity() {
                    debug!("Re-applying activity for {}", id);
                    if let Err(e) = client_state.set_activity(activity) {
                        error!("Failed to re-apply activity: {}", e);
                    }
                }
            }
        }
    }
}
