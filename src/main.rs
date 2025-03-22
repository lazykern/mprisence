use clap::Parser;
use log::{debug, error, info, trace, warn};
use parking_lot::Mutex as ParkingLotMutex;
use smallvec::SmallVec;
use std::{
    collections::{hash_map::Entry, BTreeMap, HashMap, VecDeque},
    fmt::Display,
    sync::Arc,
    thread::sleep,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use discord_presence::Client as DiscordClient;
use handlebars::Handlebars;
use mpris::{Metadata, PlaybackStatus, Player, PlayerFinder};
use smol_str::SmolStr;
use tokio::sync::{mpsc, Mutex as TokioMutex};

mod cli;
mod config;
mod cover;
mod error;
mod utils;

use std::alloc::System;

#[global_allocator]
static GLOBAL: System = System;

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
        // Just store minimal data for comparison
        pub track_id: Option<Box<str>>,
        pub url: Option<Box<str>>,
        pub title: Option<Box<str>>,
        pub artists: Option<Box<str>>,
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
                self.title.as_deref().unwrap_or("Unknown"),
                self.position,
                self.volume
            )?;

            // Add track identifiers if available
            if let Some(track_id) = &self.track_id {
                write!(f, " id:{}", track_id)?;
            }

            if let Some(url) = &self.url {
                write!(f, " url:{}", url)?;
            }

            Ok(())
        }
    }

    impl TryFrom<&Player> for PlayerState {
        type Error = PlayerError;

        fn try_from(player: &Player) -> Result<Self, Self::Error> {
            let metadata = player.get_metadata().map_err(PlayerError::DBus)?;

            Ok(Self {
                playback_status: player.get_playback_status().map_err(PlayerError::DBus)?,
                track_id: metadata.track_id().map(|s| s.to_string().into_boxed_str()),
                url: metadata.url().map(|s| s.to_string().into_boxed_str()),
                title: metadata.title().map(|s| s.to_string().into_boxed_str()),
                artists: metadata.artists().map(|a| a.join(", ").into_boxed_str()),
                position: player.get_position().map_err(PlayerError::DBus)?.as_secs() as u32,
                volume: (player.get_volume().map_err(PlayerError::DBus)? * 100.0) as u8,
            })
        }
    }

    impl PlayerState {
        /// Checks if metadata, status or volume has changed
        pub fn has_metadata_changes(&self, previous: &Self) -> bool {
            // Check track identity (most important change)
            if self.track_id != previous.track_id || self.url != previous.url {
                debug!("Track identity changed");
                return true;
            }

            // Check playback status and volume
            if self.playback_status != previous.playback_status || self.volume != previous.volume {
                log::info!(
                    "Player changed status: {:?} -> {:?}",
                    previous.playback_status,
                    self.playback_status,
                );
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
        players: HashMap<PlayerId, Player>, // Store actual players for metadata access
        player_states: HashMap<PlayerId, PlayerState>,
        event_tx: mpsc::Sender<event::Event>,
    }

    impl PlayerManager {
        pub fn new(event_tx: mpsc::Sender<event::Event>) -> Result<Self, PlayerError> {
            info!("Initializing PlayerManager");
            let finder = PlayerFinder::new().map_err(PlayerError::DBus)?;

            Ok(Self {
                player_finder: finder,
                players: HashMap::new(),
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
                self.players.remove(&id);
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
                        // Store the player for later metadata access
                        self.players.insert(id.clone(), player);

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

                        // Handle clear on pause here
                        if player_state.playback_status == PlaybackStatus::Paused
                            && config::get().clear_on_pause()
                        {
                            Some(event::Event::ClearActivity(key))
                        } else {
                            Some(event::Event::PlayerUpdate(key, player_state.clone()))
                        }
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

                    // Handle clear on pause for new players too
                    if player_state.playback_status == PlaybackStatus::Paused
                        && config::get().clear_on_pause()
                    {
                        Some(event::Event::ClearActivity(key))
                    } else {
                        Some(event::Event::PlayerUpdate(key, player_state))
                    }
                }
            };

            // Send event if any
            if let Some(event) = event {
                self.send_event(event).await?;
            }

            Ok(())
        }

        // New method to get current metadata for a player
        pub fn get_metadata(&self, player_id: &PlayerId) -> Result<Metadata, PlayerError> {
            if let Some(player) = self.players.get(player_id) {
                player.get_metadata().map_err(PlayerError::DBus)
            } else {
                Err(PlayerError::General(format!(
                    "Player not found: {}",
                    player_id
                )))
            }
        }
    }
}

// ============================================================================
// PRESENCE MODULE - Discord integration
// ============================================================================
mod presence {
    use log::trace;

    use super::utils::format_duration;
    use super::*;

    pub struct PresenceManager {
        discord_clients: HashMap<player::PlayerId, DiscordClient>,
        template_manager: template::TemplateManager,
        has_activity: HashMap<player::PlayerId, bool>,
        cover_art_manager: cover::CoverArtManager,
        player_states: HashMap<player::PlayerId, player::PlayerState>,
        player_manager: Arc<TokioMutex<player::PlayerManager>>,
    }

    impl PresenceManager {
        pub fn new(
            template_manager: template::TemplateManager,
            player_manager: Arc<TokioMutex<player::PlayerManager>>,
        ) -> Result<Self, PresenceError> {
            info!("Initializing PresenceManager");
            let config = config::get();

            let cover_art_manager = cover::CoverArtManager::new(&config).map_err(|e| {
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

        pub async fn handle_event(&mut self, event: event::Event) -> Result<(), PresenceError> {
            match event {
                event::Event::PlayerUpdate(id, state) => {
                    self.has_activity.insert(id.clone(), true);
                    self.update_presence(&id, &state).await?;
                }
                event::Event::PlayerRemove(id) => {
                    self.has_activity.remove(&id);
                    self.remove_presence(&id)?;
                }
                event::Event::ClearActivity(id) => {
                    // Only clear if activity is active
                    if self.has_activity.get(&id).copied().unwrap_or(false) {
                        if let Some(client) = self.discord_clients.get_mut(&id) {
                            if let Err(e) = client.clear_activity() {
                                warn!("Failed to clear activity for {}: {}", id, e);
                            } else {
                                debug!("Cleared activity for {}", id);
                                self.has_activity.insert(id, false);
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

                    // Update all active players with new template/config
                    let players_to_update: Vec<_> = self
                        .player_states
                        .iter()
                        .filter(|(id, _)| self.has_activity.get(id).copied().unwrap_or(false))
                        .map(|(id, state)| (id.clone(), state.clone()))
                        .collect();

                    for (id, state) in players_to_update {
                        if let Err(e) = self.update_presence(&id, &state).await {
                            error!(
                                "Failed to update presence for {} after config change: {}",
                                id, e
                            );
                        }
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
                if self.has_activity.get(player_id).copied().unwrap_or(false) {
                    if let Some(client) = self.discord_clients.get_mut(player_id) {
                        if let Err(e) = client.clear_activity() {
                            warn!("Failed to clear activity for {}: {}", player_id, e);
                        } else {
                            debug!("Cleared activity for stopped player {}", player_id);
                            self.has_activity.insert(player_id.clone(), false);
                        }
                    }
                }
                return Ok(());
            }

            let config = config::get();
            let player_config = config.player_config(player_id.identity.as_str());
            let as_elapsed = config.time_config().as_elapsed;

            // Save player state for later reference
            self.player_states.insert(player_id.clone(), state.clone());

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

        fn remove_presence(&mut self, player_id: &player::PlayerId) -> Result<(), PresenceError> {
            debug!("Removing Discord client for player: {}", player_id);
            self.has_activity.remove(player_id);

            if let Some(client) = self.discord_clients.remove(player_id) {
                // The client will be dropped, which should automatically clean up
                debug!("Removed Discord client for player: {}", player_id);
            }

            Ok(())
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
}

// ============================================================================
// TEMPLATE MODULE - Simple templating functionality
// ============================================================================
mod template {
    use super::*;
    use crate::utils::format_duration;
    use std::collections::{BTreeMap, HashMap};

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
            // Directly render without caching
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
                handle_unknown(state.title.as_ref().map(|s| s.to_string())),
            );

            data.insert(
                "artists".to_string(),
                handle_unknown(state.artists.as_ref().map(|s| s.to_string())),
            );

            // URL
            if let Some(url) = state.url.as_ref().map(|s| s.to_string()) {
                data.insert("url".to_string(), url);
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

    impl Display for Event {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let event = match self {
                Event::PlayerUpdate(id, state) => format!("PlayerUpdate({}, {})", id, state),
                Event::PlayerRemove(id) => format!("PlayerRemove({})", id),
                Event::ClearActivity(id) => format!("ClearActivity({})", id),
                Event::ConfigChanged => "ConfigChanged".to_string(),
            };
            write!(f, "{}", event)
        }
    }
}

// ============================================================================
// SERVICE MODULE - Main application service
// ============================================================================
mod service {
    use log::trace;

    use super::*;

    pub struct Service {
        player_manager: Arc<TokioMutex<player::PlayerManager>>,
        presence_manager: presence::PresenceManager,
        event_rx: mpsc::Receiver<event::Event>,
        event_tx: mpsc::Sender<event::Event>,
        config_rx: config::ConfigChangeReceiver,
        pending_events: SmallVec<[event::Event; 16]>,
    }

    impl Service {
        pub fn new() -> Result<Self, ServiceInitError> {
            info!("Initializing service components");

            let (event_tx, event_rx) = mpsc::channel(128);

            debug!("Creating template manager");
            let config = config::get();
            let template_manager = template::TemplateManager::new(&config)?;

            debug!("Creating player manager");
            let player_manager = Arc::new(TokioMutex::new(player::PlayerManager::new(
                event_tx.clone(),
            )?));

            debug!("Creating presence manager");
            let presence_manager =
                presence::PresenceManager::new(template_manager, player_manager.clone())?;

            info!("Service initialization complete");
            Ok(Self {
                player_manager,
                presence_manager,
                event_rx,
                event_tx,
                config_rx: config::get().subscribe(),
                pending_events: SmallVec::new(),
            })
        }

        pub async fn run(&mut self) -> Result<(), ServiceRuntimeError> {
            info!("Starting service main loop");

            let mut interval =
                tokio::time::interval(Duration::from_millis(config::get().interval()));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        trace!("Checking players");
                        let mut player_manager = self.player_manager.lock().await;
                        if let Err(e) = player_manager.check_players().await {
                            error!("Error checking players: {}", e);
                        }
                    },

                    Ok(change) = self.config_rx.recv() => {
                        match change {
                            config::ConfigChange::Updated | config::ConfigChange::Reloaded => {
                                info!("Config change detected");
                                interval = tokio::time::interval(Duration::from_millis(config::get().interval()));

                                if let Err(e) = self.event_tx.send(event::Event::ConfigChanged).await {
                                    error!("Failed to send config changed event: {}", e);
                                }

                                if let Err(e) = self.reload_components().await {
                                    error!("Failed to reload components: {}", e);
                                }
                            },
                            config::ConfigChange::Error(e) => {
                                error!("Config error: {}", e);
                            }
                        }
                    },

                    Some(event) = self.event_rx.recv() => {
                        debug!("Received event: {}", event);

                        // Add first event to SmallVec
                        self.pending_events.push(event);

                        // Try to collect more events
                        for _ in 0..9 {
                            match self.event_rx.try_recv() {
                                Ok(event) => {
                                    debug!("Batched event: {}", event);
                                    self.pending_events.push(event);
                                },
                                Err(_) => break,
                            }
                        }

                        // Process all collected events
                        for event in self.pending_events.drain(..) {
                            trace!("Handling event: {:?}", event);
                            if let Err(e) = self.presence_manager.handle_event(event).await {
                                error!("Error handling event: {}", e);
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

        async fn reload_components(&mut self) -> Result<(), ServiceRuntimeError> {
            debug!("Reloading service components based on configuration changes");
            let config = config::get();

            // Only create a new template manager and pass it to presence manager
            let template_manager = template::TemplateManager::new(&config)?;

            // Update presence manager with new templates instead of recreating it
            if let Err(e) = self.presence_manager.update_templates(template_manager) {
                error!("Failed to update templates: {}", e);
            }

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
