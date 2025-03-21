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
use bumpalo;
use discord_rich_presence::{
    activity::{Activity, ActivityType, Assets, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use handlebars::Handlebars;
use mpris::{Metadata, PlaybackStatus, Player, PlayerFinder};
use smol_str::SmolStr;
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
        /// Get title with a default value if not present
        pub fn title(&self) -> &str {
            self.metadata.title().unwrap_or_default()
        }

        /// Checks if metadata, status or volume has changed
        pub fn has_metadata_changes(&self, previous: &Self) -> bool {
            let metadata_changed = !Arc::ptr_eq(&self.metadata, &previous.metadata)
                && self.metadata.as_hashmap() != previous.metadata.as_hashmap();
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
            // Convert polling_interval to seconds for comparison with u32
            let interval_secs = polling_interval.as_secs() as u32;

            // If position decreased, it's a jump backward
            if self.position < previous.position {
                debug!(
                    "Position jumped backward: {}s -> {}s",
                    previous.position, self.position
                );
                return true;
            }

            // If position increased more than expected, it's a forward jump
            let elapsed = self.position.saturating_sub(previous.position);
            let expected_max = interval_secs.saturating_mul(2); // Double polling interval as threshold

            if elapsed > expected_max {
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

            let current = self.player_finder.find_all().map_err(PlayerError::Finding)?;
            let current_ids: Vec<_> = current.iter().map(PlayerId::from).collect();

            // Find removed players
            let removed_ids: Vec<_> = self.player_states
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
                    // We could reload templates here if needed
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

            trace!("Preparing Discord activity update: {}", details);

            // Build activity using rendered templates
            let activity = Self::build_activity(
                details,
                state_text,
                large_text,
                small_text,
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
            let details_static = Box::leak(details.into_boxed_str());
            let state_static = Box::leak(state.into_boxed_str());
            let large_text_static = Box::leak(large_text.into_boxed_str());
            let small_text_static = Box::leak(small_text.into_boxed_str());

            let assets = Assets::new()
                .large_text(large_text_static)
                .small_text(small_text_static);

            Activity::new()
                .activity_type(ActivityType::Listening)
                .details(details_static)
                .state(state_static)
                .timestamps(timestamps)
                .assets(assets)
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

            // Player information
            data.insert("player".to_string(), player_id.identity.to_string());

            // Playback information - use static strings where possible
            data.insert("status".to_string(), format!("{:?}", state.playback_status));

            let status_icon = match state.playback_status {
                PlaybackStatus::Playing => "▶️",
                PlaybackStatus::Paused => "⏸️",
                PlaybackStatus::Stopped => "⏹️",
            };
            data.insert("status_icon".to_string(), status_icon.to_string());
            data.insert("volume".to_string(), state.volume.to_string());

            let position = format!(
                "{:02}:{:02}",
                state.position as u64 / 60,
                state.position as u64 % 60
            );
            data.insert("position".to_string(), position);

            // Basic track metadata
            if let Some(title) = state.metadata.title() {
                data.insert("title".to_string(), title.to_string());
            }

            if let Some(artists) = state.metadata.artists() {
                data.insert("artists".to_string(), artists.join(", "));
            }

            if let Some(album_name) = state.metadata.album_name() {
                data.insert("album_name".to_string(), album_name.to_string());
            }

            if let Some(album_artists) = state.metadata.album_artists() {
                data.insert("album_artists".to_string(), album_artists.join(", "));
            }

            // Track timing
            if let Some(length) = state.metadata.length() {
                let length = format!("{:02}:{:02}", length.as_secs() / 60, length.as_secs() % 60);
                data.insert("length".to_string(), length);
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
