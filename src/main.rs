use clap::Parser;
use log::{debug, error, info, warn};
use std::{
    collections::{hash_map::Entry, HashMap, VecDeque},
    fmt::Display,
    sync::Arc,
    thread::sleep,
    time::{Duration, SystemTime, UNIX_EPOCH, Instant},
};

use discord_rich_presence::{
    activity::{Activity, ActivityType, Assets, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use handlebars::Handlebars;
use mpris::{Metadata, PlaybackStatus, Player, PlayerFinder};
use smol_str::SmolStr;
use tokio::sync::mpsc;

mod cli;
mod config;
mod error;
mod utils;

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
        pub player_bus_name: SmolStr,
        pub identity: SmolStr,
        pub unique_name: SmolStr,
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
                player_bus_name: SmolStr::new(player.bus_name_player_name_part()),
                identity: SmolStr::new(player.identity()),
                unique_name: SmolStr::new(player.unique_name()),
            }
        }
    }

    #[derive(Debug, Clone)]
    pub struct PlayerState {
        pub playback_status: PlaybackStatus,
        pub metadata: Arc<Metadata>,
        pub position: u32,
        pub volume: u8,
    }

    impl Display for PlayerState {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            // Basic playback info
            write!(
                f,
                "{:?}: {} [{}s, {}%]",
                self.playback_status,
                self.metadata.title().unwrap_or("Unknown"),
                self.position,
                self.volume
            )?;

            // Add track identifiers if available
            if let Some(track_id) = self.metadata.track_id() {
                write!(f, " id:{}", track_id)?;
            }

            if let Some(url) = self.metadata.url() {
                write!(f, " url:{}", url)?;
            }

            Ok(())
        }
    }

    impl TryFrom<&Player> for PlayerState {
        type Error = PlayerError;

        fn try_from(player: &Player) -> Result<Self, Self::Error> {
            Ok(Self {
                playback_status: player.get_playback_status().map_err(PlayerError::DBus)?,
                metadata: Arc::new(player.get_metadata().map_err(PlayerError::DBus)?),
                position: player.get_position().map_err(PlayerError::DBus)?.as_secs() as u32,
                volume: (player.get_volume().map_err(PlayerError::DBus)? * 100.0) as u8,
            })
        }
    }

    impl PlayerState {
        /// Checks if metadata, status or volume has changed
        pub fn has_metadata_changes(&self, previous: &Self) -> bool {
            // Fast path: check Arc pointer equality first
            if Arc::ptr_eq(&self.metadata, &previous.metadata) {
                return false;
            }

            // Check track identity (most important change)
            if self.metadata.track_id() != previous.metadata.track_id()
                || self.metadata.url() != previous.metadata.url()
            {
                debug!("Track identity changed for: {}", self);
                return true;
            }

            // Check playback status and volume
            let status_changed = self.playback_status != previous.playback_status;
            let volume_changed = self.volume != previous.volume;

            if status_changed || volume_changed {
                return true;
            }

            // Check primary metadata fields
            let title_changed = self.metadata.title() != previous.metadata.title();
            let artists_changed = self.metadata.artists() != previous.metadata.artists();

            if title_changed || artists_changed {
                return true;
            }

            // Check secondary metadata fields
            let album_changed = self.metadata.album_name() != previous.metadata.album_name();
            let length_changed = self.metadata.length() != previous.metadata.length();

            if album_changed || length_changed {
                return true;
            }

            false
        }

        /// Checks if there's a significant position change that's not explained by normal playback
        pub fn has_position_jump(&self, previous: &Self, polling_interval: Duration) -> bool {
            // Convert polling interval to seconds for comparison
            let max_expected_change = (polling_interval.as_secs() as u32) * 2; // 2x polling interval as threshold

            // Check for backward jump
            if self.position < previous.position {
                debug!(
                    "Position jumped backward: {}s -> {}s",
                    previous.position, self.position
                );
                return true;
            }

            // Check for forward jump that exceeds expected progression
            let elapsed = self.position.saturating_sub(previous.position);
            if elapsed > max_expected_change {
                debug!(
                    "Position jumped forward: {}s -> {}s",
                    previous.position, self.position
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
                .filter(|id| !current_ids.contains(id))
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

                match PlayerState::try_from(&player) {
                    Ok(player_state) => {
                        if let Err(e) = self
                            .process_player_state(id.clone(), player_state, polling_interval)
                            .await
                        {
                            error!("Failed to process player state: {}", e);
                            // Send clear activity event instead of removal
                            if let Err(e) = self.send_event(event::Event::ClearActivity(id)).await {
                                error!("Failed to send clear activity event: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Failed to get player state for {}: {}", id, e);
                        // Send clear activity event instead of removal
                        if let Err(e) = self.send_event(event::Event::ClearActivity(id)).await {
                            error!("Failed to send clear activity event: {}", e);
                        }
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
            let event = match self.player_states.entry(id) {
                Entry::Occupied(mut entry) => {
                    let has_changes = player_state.requires_presence_update(
                        entry.get(),
                        Duration::from_millis(polling_interval),
                    );

                    let event = if has_changes {
                        let key = entry.key().clone();
                        debug!("Player {} updated: {}", key, player_state);
                        Some(event::Event::PlayerUpdate(key, player_state.clone()))
                    } else {
                        None
                    };
                    entry.insert(player_state);
                    event
                }
                Entry::Vacant(entry) => {
                    let key = entry.key().clone();
                    info!("New player detected: {} playing {}", key, player_state);
                    entry.insert(player_state.clone());
                    Some(event::Event::PlayerUpdate(key, player_state))
                }
            };

            // Send event if any
            if let Some(event) = event {
                self.send_event(event).await?;
            }

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
        cleared_activities: HashMap<player::PlayerId, bool>, // true if activity is cleared
    }

    impl PresenceManager {
        pub fn new(template_manager: template::TemplateManager) -> Self {
            info!("Initializing PresenceManager");
            Self {
                discord_clients: HashMap::new(),
                template_manager,
                cleared_activities: HashMap::new(),
            }
        }

        pub async fn handle_event(&mut self, event: event::Event) -> Result<(), PresenceError> {
            match event {
                event::Event::PlayerUpdate(id, state) => {
                    // If player is paused and clear_on_pause is enabled, clear the activity
                    if state.playback_status == PlaybackStatus::Paused && config::get().clear_on_pause() {
                        if !self.cleared_activities.get(&id).copied().unwrap_or(false) {
                            if let Some(client) = self.discord_clients.get_mut(&id) {
                                if let Err(e) = Self::clear_activity(client) {
                                    warn!("Failed to clear activity for {}: {}", id, e);
                                } else {
                                    debug!("Cleared activity for paused player {}", id);
                                    self.cleared_activities.insert(id, true);
                                }
                            }
                        }
                    } else {
                        // Player is playing or clear_on_pause is disabled, update presence
                        self.cleared_activities.insert(id.clone(), false);
                        self.update_presence(&id, &state).await?;
                    }
                }
                event::Event::PlayerRemove(id) => {
                    self.cleared_activities.remove(&id);
                    self.remove_presence(&id)?;
                }
                event::Event::ClearActivity(id) => {
                    // Only clear if not already cleared
                    if !self.cleared_activities.get(&id).copied().unwrap_or(false) {
                        if let Some(client) = self.discord_clients.get_mut(&id) {
                            if let Err(e) = Self::clear_activity(client) {
                                warn!("Failed to clear activity for {}: {}", id, e);
                            } else {
                                debug!("Cleared activity for {}", id);
                                self.cleared_activities.insert(id, true);
                            }
                        }
                    } else {
                        trace!("Skipping clear activity for {}: already cleared", id);
                    }
                }
                event::Event::ConfigChanged => {
                    debug!("Received config changed event in presence manager");
                    let config = config::get();
                    if let Err(e) = self.template_manager.reload(&config) {
                        error!("Failed to reload templates: {}", e);
                    }
                }
            }

            Ok(())
        }

        async fn update_presence(
            &mut self,
            player_id: &player::PlayerId,
            state: &player::PlayerState,
        ) -> Result<(), PresenceError> {
            // Don't show activity if player is stopped
            if state.playback_status == PlaybackStatus::Stopped {
                // Clear activity if it's not already cleared
                if !self.cleared_activities.get(player_id).copied().unwrap_or(false) {
                    if let Some(client) = self.discord_clients.get_mut(player_id) {
                        if let Err(e) = Self::clear_activity(client) {
                            warn!("Failed to clear activity for {}: {}", player_id, e);
                        } else {
                            debug!("Cleared activity for stopped player {}", player_id);
                            self.cleared_activities.insert(player_id.clone(), true);
                        }
                    }
                }
                return Ok(());
            }

            let ctx = config::get();
            let player_config = ctx.player_config(player_id.identity.as_str());
            let as_elapsed = ctx.time_config().as_elapsed;

            // Create template data
            let template_data = template::TemplateManager::create_data(player_id, state);

            // Render templates
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

            // Get content type from metadata if available
            let content_type = utils::get_content_type_from_metadata(&state.metadata);
            
            // Determine activity type based on content type or player configuration
            let activity_type = ctx.player_config(player_id.identity.as_str())
                .activity_type(content_type.as_deref());

            trace!("Preparing Discord activity update: {}", details);

            // Build activity using rendered templates
            let activity = Self::build_activity(
                details,
                state_text,
                large_text,
                small_text,
                activity_type,
                state.playback_status,
                Duration::from_secs(state.position as u64),
                state.metadata.length().unwrap_or_default(),
                as_elapsed,
            );

            self.update_activity(player_id, activity, &player_config.app_id)
        }

        fn build_activity(
            details: String,
            state: String,
            large_text: String,
            small_text: String,
            activity_type: config::ActivityType,
            playback_status: PlaybackStatus,
            position: Duration,
            length: Duration,
            show_elapsed: bool,
        ) -> Activity<'static> {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards");

            let start_dur = now.checked_sub(position).unwrap_or_default();
            let end = start_dur.checked_add(length).unwrap_or_default();

            let start_s = start_dur.as_secs();
            let end_s = end.as_secs();

            let mut timestamps = Timestamps::new();

            if !show_elapsed {
                timestamps = timestamps.end(end_s as i64);
            }

            if playback_status == PlaybackStatus::Playing {
                timestamps = timestamps.start(start_s as i64);
            }

            // SAFETY: We intentionally leak these strings to get 'static references.
            // The discord-rich-presence library requires 'static str references, and these
            // strings are used for the lifetime of the program. Memory usage is bounded
            // by the number of unique status messages.
            let details_static = if !details.is_empty() {
                Some(Box::leak(details.into_boxed_str()))
            } else {
                None
            };

            let state_static = if !state.is_empty() {
                Some(Box::leak(state.into_boxed_str()))
            } else {
                None
            };

            let large_text_static = if !large_text.is_empty() {
                Some(Box::leak(large_text.into_boxed_str()))
            } else {
                None
            };

            let small_text_static = if !small_text.is_empty() {
                Some(Box::leak(small_text.into_boxed_str()))
            } else {
                None
            };

            let mut assets = Assets::new();
            if let Some(large_text) = large_text_static {
                assets = assets.large_text(large_text);
            }
            if let Some(small_text) = small_text_static {
                assets = assets.small_text(small_text);
            }

            trace!("activity_type: {:?}", activity_type);
            trace!("timestamp start: {}", start_s);
            trace!("timestamp end: {}", end_s);

            let mut activity = Activity::new().activity_type(activity_type.into());

            if let Some(details) = details_static {
                activity = activity.details(details);
            }
            if let Some(state) = state_static {
                activity = activity.state(state);
            }

            activity.timestamps(timestamps).assets(assets)
        }

        fn update_activity(
            &mut self,
            player_id: &player::PlayerId,
            activity: Activity,
            app_id: &str,
        ) -> Result<(), PresenceError> {
            debug!("Updating activity for player: {}", player_id);

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

        fn clear_activity(
            client: &mut DiscordIpcClient,
        ) -> Result<(), PresenceError> {
            debug!("Clearing Discord activity");
            client
                .clear_activity()
                .map_err(|e| PresenceError::Update(format!("Clear error: {}", e)))?;
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
            self.cleared_activities.remove(player_id);
            if let Some(mut client) = self.discord_clients.remove(player_id) {
                // Try to clear activity before closing
                if let Err(e) = Self::clear_activity(&mut client) {
                    warn!("Failed to clear activity: {}", e);
                }
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
    use crate::utils::format_duration;

    use super::*;
    use std::collections::BTreeMap;

    pub struct TemplateManager {
        handlebars: Handlebars<'static>,
    }

    impl TemplateManager {
        pub fn new(config: &Arc<config::ConfigManager>) -> Result<Self, TemplateError> {
            info!("Initializing TemplateManager");
            let mut handlebars = Handlebars::new();
            let template_config = config.template_config();

            // Register all templates
            handlebars.register_template_string("detail", &template_config.detail)?;
            handlebars.register_template_string("state", &template_config.state)?;
            handlebars.register_template_string("large_text", &template_config.large_text)?;
            handlebars.register_template_string("small_text", &template_config.small_text)?;

            info!("Template registration successful");
            Ok(Self { handlebars })
        }

        pub fn reload(&mut self, config: &Arc<config::ConfigManager>) -> Result<(), TemplateError> {
            debug!("Reloading templates");
            let template_config = config.template_config();

            // Reregister all templates without recreating Handlebars instance
            self.handlebars
                .register_template_string("detail", &template_config.detail)?;
            self.handlebars
                .register_template_string("state", &template_config.state)?;
            self.handlebars
                .register_template_string("large_text", &template_config.large_text)?;
            self.handlebars
                .register_template_string("small_text", &template_config.small_text)?;

            Ok(())
        }

        pub fn render(
            &self,
            template_name: &str,
            data: &BTreeMap<String, String>,
        ) -> Result<String, TemplateError> {
            Ok(self.handlebars.render(template_name, data)?)
        }

        /// Helper to create template data from player state
        pub fn create_data(
            player_id: &player::PlayerId,
            state: &player::PlayerState,
        ) -> BTreeMap<String, String> {
            let mut data = BTreeMap::new();
            let config = config::get();
            let template_config = config.template_config();

            // Helper function to handle unknown values
            let handle_unknown = |value: Option<String>| -> String {
                value.unwrap_or_else(|| template_config.unknown_text.to_string())
            };

            // Player information
            data.insert("player".to_string(), player_id.identity.to_string());
            data.insert(
                "player_bus_name".to_string(),
                player_id.player_bus_name.to_string(),
            );

            // Playback information - use static strings where possible
            data.insert("status".to_string(), format!("{:?}", state.playback_status));

            let status_icon = match state.playback_status {
                PlaybackStatus::Playing => "▶",
                PlaybackStatus::Paused => "⏸️",
                PlaybackStatus::Stopped => "⏹️",
            };
            data.insert("status_icon".to_string(), status_icon.to_string());
            data.insert("volume".to_string(), state.volume.to_string());

            data.insert(
                "position".to_string(),
                format_duration(state.position as u64),
            );

            // Basic track metadata with unknown handling
            data.insert(
                "title".to_string(),
                handle_unknown(state.metadata.title().map(|s| s.to_string())),
            );

            data.insert(
                "artists".to_string(),
                handle_unknown(state.metadata.artists().map(|a| a.join(", "))),
            );

            data.insert(
                "album_name".to_string(),
                handle_unknown(state.metadata.album_name().map(|s| s.to_string())),
            );

            data.insert(
                "album_artists".to_string(),
                handle_unknown(state.metadata.album_artists().map(|a| a.join(", "))),
            );

            // Track timing
            if let Some(length) = state.metadata.length() {
                data.insert("length".to_string(), format_duration(length.as_secs()));
            }

            // Track numbering
            if let Some(track_number) = state.metadata.track_number() {
                data.insert("track_number".to_string(), track_number.to_string());
            }

            if let Some(disc_number) = state.metadata.disc_number() {
                data.insert("disc_number".to_string(), disc_number.to_string());
            }

            // Additional metadata fields
            if let Some(auto_rating) = state.metadata.auto_rating() {
                data.insert("auto_rating".to_string(), auto_rating.to_string());
            }

            // URL
            if let Some(url) = state.metadata.url() {
                data.insert("url".to_string(), url.to_string());
            }

            data
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
        ClearActivity(player::PlayerId),
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
            let config = config::get();
            let template_manager = template::TemplateManager::new(&config)?;

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
                        debug!("Received event: {:#?}", event);
                        if let Err(e) = self.presence_manager.handle_event(event).await {
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
            let config = config::get();
            let template_manager = template::TemplateManager::new(&config)?;
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
