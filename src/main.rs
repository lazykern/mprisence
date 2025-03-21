use clap::Parser;
use log::{debug, error, info, warn};
use std::{
    collections::{HashMap, VecDeque},
    fmt::Display,
    sync::Arc,
    thread::sleep,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

// External dependencies
use discord_rich_presence::{
    activity::{Activity, ActivityType, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use handlebars::Handlebars;
use mpris::{Metadata, PlaybackStatus, Player, PlayerFinder};
use tokio::{
    sync::{broadcast, mpsc, RwLock},
    time,
};

mod cli;
mod config;
mod error;

// Re-exports
use crate::error::{
    Error, PlayerError, PresenceError, ServiceInitError, ServiceRuntimeError, TemplateError,
};

// ============================================================================
// PLAYER MODULE - Media player tracking
// ============================================================================
mod player {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    pub struct PlayerId {
        pub player_bus_name: String,
        pub identity: String,
        pub unique_name: String,
    }

    impl Display for PlayerId {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(
                f,
                "{}:{}:{}",
                self.identity, self.player_bus_name, self.unique_name
            )
        }
    }

    impl From<&Player> for PlayerId {
        fn from(player: &Player) -> Self {
            Self {
                player_bus_name: player.bus_name_player_name_part().to_string(),
                identity: player.identity().to_string(),
                unique_name: player.unique_name().to_string(),
            }
        }
    }

    #[derive(Debug, Clone)]
    pub struct PlayerState {
        pub playback_status: PlaybackStatus,
        pub metadata: Metadata,
        pub position: Duration,
        pub volume: u8,
    }

    impl TryFrom<&Player> for PlayerState {
        type Error = PlayerError;

        fn try_from(player: &Player) -> Result<Self, Self::Error> {
            Ok(Self {
                playback_status: player.get_playback_status().map_err(PlayerError::DBus)?,
                metadata: player.get_metadata().map_err(PlayerError::DBus)?,
                position: player.get_position().map_err(PlayerError::DBus)?,
                volume: (player.get_volume().map_err(PlayerError::DBus)? * 100.0) as u8,
            })
        }
    }

    impl PlayerState {
        /// Get title with a default value if not present
        pub fn title(&self) -> &str {
            self.metadata.title().unwrap_or_default()
        }

        /// Checks if metadata, status or volume has changed
        pub fn has_metadata_changes(&self, previous: &Self) -> bool {
            let metadata_changed = self.metadata.as_hashmap() != previous.metadata.as_hashmap();
            let status_changed = self.playback_status != previous.playback_status;
            let volume_changed = self.volume != previous.volume;

            // Log changes if debug is enabled
            if log::log_enabled!(log::Level::Debug)
                && (metadata_changed || status_changed || volume_changed)
            {
                self.log_changes(previous, metadata_changed, status_changed, volume_changed);
            }

            metadata_changed || status_changed || volume_changed
        }

        fn log_changes(
            &self,
            previous: &Self,
            metadata_changed: bool,
            status_changed: bool,
            volume_changed: bool,
        ) {
            if metadata_changed {
                debug!("Track metadata changed for {}", self.title());
                let old_map = previous.metadata.as_hashmap();
                let new_map = self.metadata.as_hashmap();

                // Handle only the essential changes - simplest approach
                for key in old_map
                    .keys()
                    .chain(new_map.keys())
                    .collect::<std::collections::HashSet<_>>()
                {
                    match (old_map.get(key), new_map.get(key)) {
                        (Some(old), Some(new)) if old != new => {
                            debug!("  {}: '{:?}' -> '{:?}'", key, old, new)
                        }
                        (Some(old), None) => debug!("  {} removed: '{:?}'", key, old),
                        (None, Some(new)) => debug!("  {} added: '{:?}'", key, new),
                        _ => {}
                    }
                }
            }

            if status_changed {
                debug!(
                    "Playback status changed: {:?} -> {:?}",
                    previous.playback_status, self.playback_status
                );
            }

            if volume_changed {
                debug!("Volume changed: {}% -> {}%", previous.volume, self.volume);
            }
        }

        /// Checks if there's a significant position change that's not explained by normal playback
        pub fn has_position_jump(&self, previous: &Self, polling_interval: Duration) -> bool {
            // Simple approach: detect significant jumps (more than twice the polling interval)
            // or when position decreases (seek backwards)
            
            // If position decreased, it's a jump backward
            if self.position < previous.position {
                debug!(
                    "Position jumped backward: {}s -> {}s",
                    previous.position.as_secs(),
                    self.position.as_secs()
                );
                return true;
            }
            
            // If position increased more than expected, it's a forward jump
            let elapsed = self.position - previous.position;
            let expected_max = polling_interval.saturating_mul(2); // Double polling interval as threshold
            
            if elapsed > expected_max {
                debug!(
                    "Position jumped forward: {}s -> {}s",
                    previous.position.as_secs(),
                    self.position.as_secs()
                );
                return true;
            }
            
            false
        }

        /// Determines if a presence update is needed
        pub fn requires_presence_update(
            &self,
            previous: &Self,
            polling_interval: Duration,
        ) -> bool {
            self.has_metadata_changes(previous)
                || self.has_position_jump(previous, polling_interval)
        }
    }

    pub struct PlayerManager {
        player_finder: PlayerFinder,
        player_states: HashMap<PlayerId, PlayerState>,
        event_tx: mpsc::Sender<event::Event>,
    }

    impl PlayerManager {
        pub fn new(event_tx: mpsc::Sender<event::Event>) -> Result<Self, PlayerError> {
            info!("Initializing PlayerManager");
            let finder = PlayerFinder::new().map_err(PlayerError::DBus)?;

            Ok(Self {
                player_finder: finder,
                player_states: HashMap::new(),
                event_tx,
            })
        }

        pub async fn check_players(&mut self) -> Result<(), PlayerError> {
            let config = config::get();
            let polling_interval = config.interval();
            let current = self
                .player_finder
                .find_all()
                .map_err(PlayerError::Finding)?;
            let current_ids: Vec<_> = current.iter().map(PlayerId::from).collect();

            // Find removed players
            let removed_ids: Vec<_> = self
                .player_states
                .keys()
                .filter(|id| !current_ids.iter().any(|curr_id| curr_id == *id))
                .cloned()
                .collect();

            // Process removals
            for id in removed_ids {
                info!("Player removed: {}", id);
                self.player_states.remove(&id);
                if let Err(e) = self.send_event(event::Event::PlayerRemove(id)).await {
                    error!("Failed to send removal event: {}", e);
                }
            }

            // Handle new or updated players
            for player in current {
                let id = PlayerId::from(&player);
                let player_config = config.player_config(id.identity.as_str());

                if player_config.ignore {
                    debug!("Ignoring player {} (configured to ignore)", id);
                    continue;
                }

                // Process player state
                match PlayerState::try_from(&player) {
                    Ok(player_state) => {
                        if let Err(e) = self
                            .process_player_state(id, player_state, polling_interval)
                            .await
                        {
                            error!("Failed to process player state: {}", e);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to get player state for {}: {}", id, e);
                    }
                }
            }

            Ok(())
        }

        async fn send_event(&self, event: event::Event) -> Result<(), PlayerError> {
            self.event_tx
                .send(event)
                .await
                .map_err(|e| PlayerError::General(format!("Failed to send event: {}", e)))
        }

        async fn process_player_state(
            &mut self,
            id: PlayerId,
            player_state: PlayerState,
            polling_interval: u64,
        ) -> Result<(), PlayerError> {
            let clear_on_pause = config::get().clear_on_pause();

            match self.player_states.get(&id) {
                Some(old_state) => {
                    let has_changes = player_state.requires_presence_update(
                        old_state,
                        Duration::from_millis(polling_interval),
                    );

                    if has_changes {
                        // Check if transitioning from playing to paused
                        if clear_on_pause
                            && player_state.playback_status == PlaybackStatus::Paused
                            && old_state.playback_status == PlaybackStatus::Playing
                        {
                            info!("Player {} paused: '{}'", id, player_state.title());
                            // Use PlayerRemoved instead of PlayerPaused
                            self.send_event(event::Event::PlayerRemove(id.clone()))
                                .await?;
                        } else {
                            debug!("Player {} updated: '{}'", id, player_state.title());
                            self.send_event(event::Event::PlayerUpdate(
                                id.clone(),
                                player_state.clone(),
                            ))
                            .await?;
                        }
                    }
                }
                None => {
                    info!(
                        "New player detected: {} playing '{}'",
                        id,
                        player_state.title()
                    );
                    self.send_event(event::Event::PlayerUpdate(id.clone(), player_state.clone()))
                        .await?;
                }
            }

            // Always update the state
            self.player_states.insert(id, player_state);
            Ok(())
        }
    }
}

// ============================================================================
// PRESENCE MODULE - Discord integration
// ============================================================================
mod presence {
    use log::trace;

    use super::*;

    pub struct PresenceManager {
        discord_clients: HashMap<player::PlayerId, DiscordIpcClient>,
        template_manager: template::TemplateManager,
    }

    impl PresenceManager {
        pub fn new(template_manager: template::TemplateManager) -> Self {
            info!("Initializing PresenceManager");
            Self {
                discord_clients: HashMap::new(),
                template_manager,
            }
        }

        pub async fn handle_event(
            &mut self,
            event: event::Event,
            player_manager: &player::PlayerManager,
        ) -> Result<(), PresenceError> {
            match event {
                event::Event::PlayerUpdate(id, state) => {
                    self.update_presence(&id, &state).await?;
                }
                event::Event::PlayerRemove(id) => {
                    self.remove_presence(&id)?;
                }
                event::Event::ConfigChanged => {
                    // Config changed event could trigger template reload if needed
                    debug!("Received config changed event in presence manager");
                }
            }

            Ok(())
        }

        async fn update_presence(
            &mut self,
            player_id: &player::PlayerId,
            state: &player::PlayerState,
        ) -> Result<(), PresenceError> {
            let ctx = config::get();
            let player_config = ctx.player_config(player_id.identity.as_str());
            let as_elapsed = ctx.time_config().as_elapsed;

            let title = state.title();
            let length = state.metadata.length().unwrap_or_default();

            let activity = Self::build_activity(
                title,
                state.playback_status,
                state.position,
                length,
                as_elapsed,
            );
            trace!("Preparing Discord activity update with details: {}", title);

            self.update_activity(player_id, activity, &player_config.app_id)
        }

        fn build_activity(
            title: &str,
            playback_status: PlaybackStatus,
            position: Duration,
            length: Duration,
            show_elapsed: bool,
        ) -> Activity {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards");

            let start_dur = now.checked_sub(position).unwrap_or_default();
            let end = start_dur.checked_add(length).unwrap_or_default();

            let start_s = start_dur.as_secs();
            let end_s = end.as_secs();

            // Build timestamps
            let mut timestamps = Timestamps::new();

            if !show_elapsed {
                timestamps = timestamps.end(end_s as i64);
            }

            if playback_status == PlaybackStatus::Playing {
                timestamps = timestamps.start(start_s as i64);
            }

            // Build activity with consistent builder pattern
            Activity::new()
                .activity_type(ActivityType::Listening)
                .details(title)
                .timestamps(timestamps)
        }

        fn update_activity(
            &mut self,
            player_id: &player::PlayerId,
            activity: Activity,
            app_id: &str,
        ) -> Result<(), PresenceError> {
            debug!("Updating activity for player: {}", player_id);

            // Get or create client
            if !self.discord_clients.contains_key(player_id) {
                match Self::create_client(app_id) {
                    Ok(client) => {
                        self.discord_clients.insert(player_id.clone(), client);
                    }
                    Err(e) => return Err(e),
                }
            }

            let client = self
                .discord_clients
                .get_mut(player_id)
                .ok_or_else(|| PresenceError::Update("Client unexpectedly missing".to_string()))?;

            Self::set_activity(client, activity)
        }

        fn create_client(app_id: &str) -> Result<DiscordIpcClient, PresenceError> {
            debug!("Creating new Discord client with app_id: {}", app_id);
            let mut client = DiscordIpcClient::new(app_id)
                .map_err(|e| PresenceError::Connection(format!("Connection error: {}", e)))?;

            info!("Connecting to Discord...");
            client
                .connect()
                .map_err(|e| PresenceError::Connection(format!("Connection error: {}", e)))?;
            info!("Successfully connected to Discord");

            Ok(client)
        }

        fn set_activity(
            client: &mut DiscordIpcClient,
            activity: Activity,
        ) -> Result<(), PresenceError> {
            debug!("Setting Discord activity");
            client
                .set_activity(activity)
                .map_err(|e| PresenceError::Update(format!("Update error: {}", e)))?;
            Ok(())
        }

        fn close_client(client: &mut DiscordIpcClient) -> Result<(), PresenceError> {
            debug!("Closing Discord client connection");
            client
                .close()
                .map_err(|e| PresenceError::Close(format!("Connection close error: {}", e)))?;
            Ok(())
        }

        fn remove_presence(&mut self, player_id: &player::PlayerId) -> Result<(), PresenceError> {
            debug!("Removing Discord client for player: {}", player_id);
            if let Some(mut client) = self.discord_clients.remove(player_id) {
                Self::close_client(&mut client)?;
            }
            Ok(())
        }
    }
}

// ============================================================================
// TEMPLATE MODULE - Simple templating functionality
// ============================================================================
mod template {
    use super::*;

    pub struct TemplateManager {
        handlebars: Handlebars<'static>,
    }

    impl TemplateManager {
        pub fn new(detail_template: &str) -> Result<Self, TemplateError> {
            info!("Initializing TemplateManager");
            debug!("Registering detail template: {}", detail_template);

            let mut handlebars = Handlebars::new();

            handlebars.register_template_string("detail", detail_template)?;

            info!("Template registration successful");
            Ok(Self { handlebars })
        }

        pub fn reload(&mut self, detail_template: &str) -> Result<(), TemplateError> {
            debug!("Reloading template: {}", detail_template);
            self.handlebars = Handlebars::new();
            self.handlebars
                .register_template_string("detail", detail_template)?;

            Ok(())
        }
    }
}

// ============================================================================
// EVENT MODULE - Event definitions and processing
// ============================================================================
mod event {
    use super::*;

    #[derive(Debug)]
    pub enum Event {
        PlayerUpdate(player::PlayerId, player::PlayerState),
        PlayerRemove(player::PlayerId),
        ConfigChanged,
    }
}

// ============================================================================
// SERVICE MODULE - Main application service
// ============================================================================
mod service {
    use log::trace;

    use super::*;

    pub struct Service {
        player_manager: player::PlayerManager,
        presence_manager: presence::PresenceManager,
        event_rx: mpsc::Receiver<event::Event>,
        event_tx: mpsc::Sender<event::Event>,
        config_rx: config::ConfigChangeReceiver,
    }

    impl Service {
        pub fn new() -> Result<Self, ServiceInitError> {
            info!("Initializing service components");

            let (event_tx, event_rx) = mpsc::channel(32);

            debug!("Creating template manager");
            let detail_template = config::get().template_config().detail;
            let template_manager = template::TemplateManager::new(&detail_template)?;

            debug!("Creating player manager");
            let player_manager = player::PlayerManager::new(event_tx.clone())?;

            debug!("Creating presence manager");
            let presence_manager = presence::PresenceManager::new(template_manager);

            info!("Service initialization complete");
            Ok(Self {
                player_manager,
                presence_manager,
                event_rx,
                event_tx,
                config_rx: config::get().subscribe(),
            })
        }

        pub async fn run(&mut self) -> Result<(), ServiceRuntimeError> {
            info!("Starting service main loop");

            let mut interval =
                tokio::time::interval(Duration::from_millis(config::get().interval()));

            loop {
                tokio::select! {
                    // Check players on interval tick
                    _ = interval.tick() => {
                        trace!("Checking players");
                        if let Err(e) = self.player_manager.check_players().await {
                            error!("Error checking players: {}", e);
                        }
                    },

                    // Handle config changes
                    Ok(change) = self.config_rx.recv() => {
                        match change {
                            config::ConfigChange::Updated | config::ConfigChange::Reloaded => {
                                info!("Config change detected");
                                // Update interval
                                interval = tokio::time::interval(Duration::from_millis(config::get().interval()));

                                // Send config changed event
                                if let Err(e) = self.event_tx.send(event::Event::ConfigChanged).await {
                                    error!("Failed to send config changed event: {}", e);
                                }

                                // Reload template manager with new template
                                if let Err(e) = self.reload_components().await {
                                    error!("Failed to reload components: {}", e);
                                }
                            },
                            config::ConfigChange::Error(e) => {
                                error!("Config error: {}", e);
                            }
                        }
                    }

                    // Process events from player manager
                    Some(event) = self.event_rx.recv() => {
                        debug!("Received event: {:?}", event);
                        if let Err(e) = self.presence_manager.handle_event(event, &self.player_manager).await {
                            error!("Error handling event: {}", e);
                        }
                    }

                    else => {
                        warn!("All event sources have closed, shutting down");
                        break;
                    }
                }
            }

            Ok(())
        }

        async fn reload_components(&mut self) -> Result<(), ServiceRuntimeError> {
            debug!("Reloading service components");
            let detail_template = config::get().template_config().detail;
            let template_manager = template::TemplateManager::new(&detail_template)?;
            self.presence_manager = presence::PresenceManager::new(template_manager);
            Ok(())
        }
    }
}

use crate::{cli::Cli, service::Service};

#[tokio::main]
async fn main() -> Result<(), error::Error> {
    env_logger::init();

    config::initialize()?;

    let cli = Cli::parse();
    if cli.verbose {
        info!("MPRISENCE - Verbose mode enabled");
    } else {
        info!("MPRISENCE");
    }

    match cli.command {
        Some(cmd) => cmd.execute().await?,
        None => {
            let mut service = Service::new()?;
            service.run().await?;
        }
    }

    Ok(())
}
