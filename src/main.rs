use clap::Parser;
use log::{debug, info, warn};
use std::{thread::sleep, time::Duration};

mod cli;
mod config;
pub mod error;

use crate::error::{Error, PlayerError, PresenceError, TemplateError};

use handlebars::Handlebars;
use mpris::PlayerFinder;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use tokio::sync::RwLock;

// Internal modules - to be extracted to separate files in the future
mod player {
    use crate::{
        config,
        error::{Error, PlayerError},
    };
    use log::{debug, error, info, warn};
    use mpris::{Metadata, PlaybackStatus, Player, PlayerFinder};
    use std::{collections::HashMap, fmt::Display, time::Duration};

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

        fn try_from(player: &Player) -> Result<Self, PlayerError> {
            Ok(Self {
                playback_status: player.get_playback_status()?,
                metadata: player.get_metadata()?,
                position: player.get_position()?,
                volume: (player.get_volume()? * 100.0) as u8,
            })
        }
    }

    impl PlayerState {
        /// Checks if metadata, status or volume has changed
        pub fn has_metadata_changes(&self, previous: &Self) -> bool {
            let metadata_changed = self.metadata.as_hashmap() != previous.metadata.as_hashmap();
            let status_changed = self.playback_status != previous.playback_status;
            let volume_changed = self.volume != previous.volume;

            // Log the changes for debugging
            if metadata_changed {
                debug!("Track metadata changed");
                if self.metadata.title() != previous.metadata.title() {
                    debug!(
                        "  Title: '{}' -> '{}'",
                        previous.metadata.title().unwrap_or_default(),
                        self.metadata.title().unwrap_or_default()
                    );
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

            metadata_changed || status_changed || volume_changed
        }

        /// Checks if there's a significant position change that's not explained by normal playback
        pub fn has_position_jump(&self, previous: &Self, polling_interval: Duration) -> bool {
            const BASE_TOLERANCE: Duration = Duration::from_secs(1);
            let position_tolerance = BASE_TOLERANCE + polling_interval;

            // Different logic based on playback state
            let position_changed = if previous.playback_status == PlaybackStatus::Playing
                && self.playback_status == PlaybackStatus::Playing
            {
                // If both states are playing, check if position change is reasonable
                if self.position < previous.position {
                    // Position decreased - user likely sought backward
                    debug!(
                        "Position jumped backward: {}s -> {}s (seek: -{}s)",
                        previous.position.as_secs(),
                        self.position.as_secs(),
                        (previous.position - self.position).as_secs()
                    );
                    return true;
                }

                // Position increased - check if it's close to the expected increase
                let elapsed = self.position - previous.position;
                let expected_range_min = polling_interval.saturating_sub(position_tolerance);
                let expected_range_max = polling_interval.saturating_add(position_tolerance);

                let is_jump = elapsed < expected_range_min || elapsed > expected_range_max;

                if is_jump {
                    debug!(
                        "Position jumped: {}s -> {}s (diff: +{}s, expected: +{}s±{}s)",
                        previous.position.as_secs(),
                        self.position.as_secs(),
                        elapsed.as_secs(),
                        polling_interval.as_secs(),
                        position_tolerance.as_secs()
                    );
                }

                is_jump
            } else {
                // For non-playing states or different states, any significant position difference matters
                let diff = self.position.abs_diff(previous.position);
                let is_significant = diff > position_tolerance;

                if is_significant {
                    debug!(
                          "Significant position change during non-playing state: {}s -> {}s (diff: {}s)",
                          previous.position.as_secs(),
                          self.position.as_secs(),
                          diff.as_secs()
                      );
                }

                is_significant
            };

            position_changed
        }

        /// Combines both checks to determine if a presence update is needed
        pub fn requires_presence_update(
            &self,
            previous: &Self,
            polling_interval: Duration,
        ) -> bool {
            self.has_metadata_changes(previous)
                || self.has_position_jump(previous, polling_interval)
        }
    }

    // PlayerUpdate represents possible player state changes
    #[derive(Debug)]
    pub enum PlayerUpdate {
        New(PlayerId, PlayerState),
        Updated(PlayerId, PlayerState),
        Paused(PlayerId, PlayerState),
        Removed(PlayerId),
    }

    pub struct PlayerManager {
        player_finder: PlayerFinder,
        player_states: HashMap<PlayerId, PlayerState>,
    }

    impl PlayerManager {
        pub fn new() -> Result<Self, PlayerError> {
            info!("Initializing PlayerManager");
            let finder = PlayerFinder::new()?;
            Ok(Self {
                player_finder: finder,
                player_states: HashMap::new(),
            })
        }

        pub fn get_state(&self, player_id: &PlayerId) -> Option<&PlayerState> {
            self.player_states.get(player_id)
        }

        pub fn update_players(&mut self) -> Result<Vec<PlayerUpdate>, PlayerError> {
            let ctx = config::get().context();
            let polling_interval = ctx.interval();
            let current = self.player_finder.find_all()?;
            let current_ids: Vec<_> = current.iter().map(PlayerId::from).collect();
            let mut updates = Vec::new();

            let config_ctx = config::get().context();

            // Find removed players
            let removed_ids: Vec<_> = self
                .player_states
                .keys()
                .filter(|id| !current_ids.iter().any(|curr_id| curr_id == *id))
                .cloned()
                .collect();

            // Process removals and collect updates
            for id in removed_ids {
                info!("Player removed: {}", id);
                self.player_states.remove(&id);
                updates.push(PlayerUpdate::Removed(id));
            }

            // Handle new or updated players
            for player in current {
                let id = PlayerId::from(&player);

                // Service-level config access
                let player_config = config_ctx.player_config(id.identity.as_str());

                if player_config.ignore {
                    debug!("Ignoring player {} (configured to ignore)", id);
                    continue;
                }

                // Process player state
                match PlayerState::try_from(&player) {
                    Ok(player_state) => {
                        match self.player_states.get(&id) {
                            Some(old_state) => {
                                if player_state.has_metadata_changes(old_state)
                                    || player_state.has_position_jump(
                                        old_state,
                                        Duration::from_millis(polling_interval),
                                    )
                                {
                                    // Check if transitioning from playing to paused
                                    let clear_on_pause = config_ctx.clear_on_pause();

                                    if clear_on_pause
                                        && player_state.playback_status == PlaybackStatus::Paused
                                        && old_state.playback_status == PlaybackStatus::Playing
                                    {
                                        info!("Player paused (clearing presence): {}", id);
                                        updates.push(PlayerUpdate::Paused(
                                            id.clone(),
                                            player_state.clone(),
                                        ));
                                    } else {
                                        updates.push(PlayerUpdate::Updated(
                                            id.clone(),
                                            player_state.clone(),
                                        ));
                                    }
                                }
                                self.player_states.insert(id, player_state);
                            }
                            None => {
                                info!("New player detected: {}", id);
                                updates.push(PlayerUpdate::New(id.clone(), player_state.clone()));
                                self.player_states.insert(id, player_state);
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to get player state for {}: {}", id, e);
                    }
                }
            }

            if !updates.is_empty() {
                debug!("Generated {} player updates", updates.len());
            }

            Ok(updates)
        }
    }
}

mod presence {
    use crate::{
        config,
        error::{Error, PresenceError},
        player::{PlayerId, PlayerManager, PlayerState, PlayerUpdate},
        template::TemplateManager,
    };
    use discord_rich_presence::{
        activity::{Activity, ActivityType, Timestamps},
        DiscordIpc, DiscordIpcClient,
    };
    use log::{debug, error, info, warn};
    use std::{
        collections::{HashMap, VecDeque},
        time::Duration,
    };
    use tokio::task;

    // action types for presence changes
    #[derive(Debug)]
    pub enum PresenceAction {
        Update(PlayerId),
        Remove(PlayerId),
    }

    pub struct PresenceManager {
        queue: VecDeque<PresenceAction>,
        discord_clients: HashMap<PlayerId, DiscordIpcClient>,
    }

    impl PresenceManager {
        pub fn new() -> Self {
            info!("Initializing PresenceManager");
            Self {
                queue: VecDeque::new(),
                discord_clients: HashMap::new(),
            }
        }

        pub fn add_action(&mut self, action: PresenceAction) {
            debug!("Adding presence action: {:?}", action);
            self.queue.push_back(action);
        }

        pub fn process_player_updates(&mut self, updates: Vec<PlayerUpdate>) {
            if updates.is_empty() {
                return;
            }

            for update in updates {
                match update {
                    PlayerUpdate::New(id, _) => {
                        info!("New player presence requested: {}", id);
                        self.queue.push_back(PresenceAction::Update(id));
                    }
                    PlayerUpdate::Updated(id, _) => {
                        self.queue.push_back(PresenceAction::Update(id));
                    }
                    PlayerUpdate::Paused(id, _) | PlayerUpdate::Removed(id) => {
                        info!("Player presence removal requested: {}", id);
                        self.queue.push_back(PresenceAction::Remove(id));
                    }
                }
            }
        }

        pub async fn process_queue(
            &mut self,
            player_manager: &PlayerManager,
            template_manager: &TemplateManager,
        ) -> Result<(), PresenceError> {
            while let Some(action) = self.queue.pop_front() {
                match action {
                    PresenceAction::Update(player_id) => {
                        let ctx = config::get().context();
                        let player_config = ctx.player_config(player_id.identity.as_str());
                        let as_elapsed = ctx.time_config().as_elapsed;

                        if let Some(state) = player_manager.get_state(&player_id) {
                            if let Err(e) = self
                                .update_discord_rich_presence(
                                    &player_id,
                                    state,
                                    template_manager,
                                    as_elapsed,
                                    &player_config.app_id,
                                )
                                .await
                            {
                                error!(
                                    "Failed to update Discord presence for {}: {}",
                                    player_id, e
                                );
                            }
                        } else {
                            warn!(
                                "Player state not found for {}, skipping presence update",
                                player_id
                            );
                        }
                    }
                    PresenceAction::Remove(player_id) => {
                        if let Err(e) = self.remove_client(&player_id) {
                            error!("Failed to remove Discord client for {}: {}", player_id, e);
                        }
                    }
                }
            }

            Ok(())
        }

        fn create_client(app_id: &str) -> Result<DiscordIpcClient, PresenceError> {
            debug!("Creating new Discord client with app_id: {}", app_id);
            let mut client = DiscordIpcClient::new(app_id)
                .map_err(|e| PresenceError::Connection(e.to_string()))?;

            info!("Connecting to Discord...");
            client
                .connect()
                .map_err(|e| PresenceError::Connection(e.to_string()))?;
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
                .map_err(|e| PresenceError::Update(e.to_string()))?;
            Ok(())
        }

        fn close_client(client: &mut DiscordIpcClient) -> Result<(), PresenceError> {
            debug!("Closing Discord client connection");
            client
                .close()
                .map_err(|e| PresenceError::Connection(e.to_string()))?;
            Ok(())
        }

        async fn update_discord_rich_presence(
            &mut self,
            player_id: &PlayerId,
            state: &PlayerState,
            _template_manager: &TemplateManager,
            show_elapsed: bool,
            app_id: &str,
        ) -> Result<(), PresenceError> {
            let title = state.metadata.title().unwrap_or_default();
            let length = state.metadata.length().unwrap_or_default();

            let activity = Self::build_activity(
                title,
                state.playback_status,
                state.position,
                length,
                show_elapsed,
            );

            self.update_activity(player_id, activity, app_id)
        }

        fn build_activity(
            title: &str,
            playback_status: mpris::PlaybackStatus,
            position: Duration,
            length: Duration,
            show_elapsed: bool,
        ) -> Activity {
            use std::time::{SystemTime, UNIX_EPOCH};

            let mut activity = Activity::new().activity_type(ActivityType::Listening);

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

            if playback_status == mpris::PlaybackStatus::Playing {
                timestamps = timestamps.start(start_s as i64);
            }

            // Build activity with owned strings
            activity.details(title).timestamps(timestamps)
        }

        pub fn update_activity(
            &mut self,
            player_id: &PlayerId,
            activity: Activity,
            app_id: &str,
        ) -> Result<(), PresenceError> {
            debug!("Updating activity for player: {}", player_id);
            let client = self
                .discord_clients
                .entry(player_id.clone())
                .or_insert_with(|| {
                    // Get or create client
                    Self::create_client(app_id).expect("Failed to create Discord IPC client")
                });

            Self::set_activity(client, activity)
        }

        pub fn remove_client(&mut self, player_id: &PlayerId) -> Result<(), PresenceError> {
            debug!("Removing Discord client for player: {}", player_id);
            if let Some(mut client) = self.discord_clients.remove(player_id) {
                Self::close_client(&mut client)?;
            }
            Ok(())
        }
    }
}

mod template {
    use crate::error::TemplateError;
    use handlebars::Handlebars;
    use log::{debug, info};

    pub struct TemplateManager {
        handlebars: Handlebars<'static>,
    }

    impl TemplateManager {
        pub fn new(detail_template: &str) -> Result<Self, TemplateError> {
            info!("Initializing TemplateManager");
            debug!("Registering detail template: {}", detail_template);

            let mut handlebars = Handlebars::new();

            handlebars
                .register_template_string("detail", detail_template)
                .map_err(|e| TemplateError::Init(format!("Failed to register template: {}", e)))?;

            info!("Template registration successful");
            Ok(Self { handlebars })
        }
    }
}

mod service {
    use crate::{
        config::{self, ConfigChange, ConfigChangeReceiver},
        error::{ServiceInitError, ServiceRuntimeError},
        player::PlayerManager,
        presence::PresenceManager,
        template::TemplateManager,
    };
    use log::{debug, error, info, warn};
    use std::{sync::Arc, thread::sleep, time::Duration};

    pub struct Service {
        player_manager: PlayerManager,
        presence_manager: PresenceManager,
        template_manager: TemplateManager,
        config_rx: ConfigChangeReceiver,
    }

    impl Service {
        pub fn new() -> Result<Self, ServiceInitError> {
            info!("Initializing service components");
            let cfg = config::get();

            debug!("Creating player manager");
            let player_manager = PlayerManager::new()?;

            debug!("Creating presence manager");
            let presence_manager = PresenceManager::new();

            debug!("Creating template manager");
            let template_manager = TemplateManager::new("")?;

            info!("Service initialization complete");
            Ok(Self {
                player_manager,
                presence_manager,
                template_manager,
                config_rx: cfg.subscribe(),
            })
        }

        pub async fn run(&mut self) -> Result<(), ServiceRuntimeError> {
            info!("Starting service main loop");
            loop {
                tokio::select! {
                    // Handle config changes
                    Ok(change) = self.config_rx.recv() => {
                        match change {
                            ConfigChange::Updated | ConfigChange::Reloaded => {
                                info!("Config change detected, reloading components");
                                if let Err(e) = self.reload_components() {
                                    error!("Failed to reload components: {}", e);
                                } else {
                                    info!("Components reloaded successfully");
                                }
                            },
                            ConfigChange::Error(e) => {
                                error!("Config error: {}", e);
                            }
                        }
                    }

                    // Normal service loop with moved self reference
                    _ = async {
                      let interval = config::get().context().interval();
                        sleep(Duration::from_millis(interval));
                        Ok::<_, ServiceRuntimeError>(())
                    } => {
                        // After the sleep, update
                        if let Err(e) = self.update_loop().await {
                            error!("Error in update loop: {}", e);
                        }
                    }
                }
            }
        }

        fn reload_components(&mut self) -> Result<(), ServiceRuntimeError> {
            debug!("Reloading service components");
            let ctx = config::get().context();
            let detail_template = ctx.template_config().detail;

            debug!("Recreating template manager with new template");
            self.template_manager = TemplateManager::new(&detail_template)?;

            debug!("Components reload complete");
            Ok(())
        }

        async fn update_loop(&mut self) -> Result<(), ServiceRuntimeError> {
            let player_updates = self.player_manager.update_players()?;

            if !player_updates.is_empty() {
                debug!("Processing {} player updates", player_updates.len());
                // Process updates to generate presence actions
                self.presence_manager.process_player_updates(player_updates);

                // Process presence queue
                self.presence_manager
                    .process_queue(&self.player_manager, &self.template_manager)
                    .await?;
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
