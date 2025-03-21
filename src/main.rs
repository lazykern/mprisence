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
    use log::warn;
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

    impl PartialEq for PlayerState {
        fn eq(&self, other: &Self) -> bool {
            const BASE_TOLERANCE: Duration = Duration::from_secs(1);

            let ctx = config::get().context();
            let polling_interval = Duration::from_millis(ctx.interval());

            let position_tolerance = BASE_TOLERANCE + polling_interval;

            let res = self.playback_status == other.playback_status
                && self.metadata.as_hashmap() == other.metadata.as_hashmap()
                && self.volume == other.volume
                && self.position.abs_diff(other.position) <= position_tolerance;

            res
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
            let finder = PlayerFinder::new()?;

            Ok(Self {
                player_finder: finder,
                player_states: HashMap::new(),
            })
        }

        // Get a player state if it exists
        pub fn get_state(&self, player_id: &PlayerId) -> Option<&PlayerState> {
            self.player_states.get(player_id)
        }

        // Update players and return a list of changes
        pub fn update_players(&mut self) -> Result<Vec<PlayerUpdate>, PlayerError> {
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
                self.player_states.remove(&id);
                updates.push(PlayerUpdate::Removed(id));
            }

            // Handle new or updated players
            for player in current {
                let id = PlayerId::from(&player);

                // Service-level config access
                let player_config = config_ctx.player_config(id.identity.as_str());

                if player_config.ignore {
                    continue;
                }

                // Process player state
                match PlayerState::try_from(&player) {
                    Ok(player_state) => {
                        match self.player_states.get(&id) {
                            Some(old_state) => {
                                if old_state != &player_state {
                                    // Check if transitioning from playing to paused
                                    let clear_on_pause = config_ctx.clear_on_pause();

                                    if clear_on_pause
                                        && player_state.playback_status == PlaybackStatus::Paused
                                        && old_state.playback_status == PlaybackStatus::Playing
                                    {
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

                                    self.player_states.insert(id, player_state);
                                }
                            }
                            None => {
                                updates.push(PlayerUpdate::New(id.clone(), player_state.clone()));
                                self.player_states.insert(id, player_state);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Error getting player state: {}", e);
                    }
                }
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
    use log::debug;
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
            Self {
                queue: VecDeque::new(),
                discord_clients: HashMap::new(),
            }
        }

        // Add a new action to the queue
        pub fn add_action(&mut self, action: PresenceAction) {
            self.queue.push_back(action);
        }

        // Process player updates to generate presence actions
        pub fn process_player_updates(&mut self, updates: Vec<PlayerUpdate>) {
            for update in updates {
                match update {
                    PlayerUpdate::New(id, _) | PlayerUpdate::Updated(id, _) => {
                        self.queue.push_back(PresenceAction::Update(id));
                    }
                    PlayerUpdate::Paused(id, _) | PlayerUpdate::Removed(id) => {
                        self.queue.push_back(PresenceAction::Remove(id));
                    }
                }
            }
        }

        // Process all queued actions
        pub async fn process_queue(
            &mut self,
            player_manager: &PlayerManager,
            template_manager: &TemplateManager,
        ) -> Result<(), PresenceError> {
            while let Some(action) = self.queue.pop_front() {
                debug!("Processing presence action: {:?}", action);

                match action {
                    PresenceAction::Update(player_id) => {
                        let ctx = config::get().context();
                        let player_config = ctx.player_config(player_id.identity.as_str());
                        let as_elapsed = ctx.time_config().as_elapsed;

                        // Get player state and build activity
                        if let Some(state) = player_manager.get_state(&player_id) {
                            self.update_discord_rich_presence(
                                &player_id,
                                state,
                                template_manager,
                                as_elapsed,
                                &player_config.app_id,
                            )
                            .await?;
                        }
                    }
                    PresenceAction::Remove(player_id) => {
                        self.remove_client(&player_id)?;
                    }
                }
            }

            Ok(())
        }

        // Update a player's presence
        async fn update_discord_rich_presence(
            &mut self,
            player_id: &PlayerId,
            state: &PlayerState,
            _template_manager: &TemplateManager,
            show_elapsed: bool,
            app_id: &str,
        ) -> Result<(), PresenceError> {
            // Process player data
            let title = state.metadata.title().unwrap_or_default();
            let length = state.metadata.length().unwrap_or_default();

            // Build activity
            let activity = Self::build_activity(
                title,
                state.playback_status,
                state.position,
                length,
                show_elapsed,
            );

            // Update Discord activity
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
            if let Some(mut client) = self.discord_clients.remove(player_id) {
                Self::close_client(&mut client)?;
            }
            Ok(())
        }

        fn create_client(app_id: &str) -> Result<DiscordIpcClient, PresenceError> {
            let mut client = DiscordIpcClient::new(app_id)
                .map_err(|e| PresenceError::Connection(e.to_string()))?;

            client
                .connect()
                .map_err(|e| PresenceError::Connection(e.to_string()))?;

            Ok(client)
        }

        fn set_activity(
            client: &mut DiscordIpcClient,
            activity: Activity,
        ) -> Result<(), PresenceError> {
            client
                .set_activity(activity)
                .map_err(|e| PresenceError::Update(e.to_string()))?;

            Ok(())
        }

        fn close_client(client: &mut DiscordIpcClient) -> Result<(), PresenceError> {
            client
                .close()
                .map_err(|e| PresenceError::Connection(e.to_string()))?;

            Ok(())
        }
    }
}

mod template {
    use crate::error::TemplateError;
    use handlebars::Handlebars;

    pub struct TemplateManager {
        handlebars: Handlebars<'static>,
    }

    impl TemplateManager {
        pub fn new(detail_template: &str) -> Result<Self, TemplateError> {
            let mut handlebars = Handlebars::new();

            handlebars
                .register_template_string("detail", detail_template)
                .map_err(|e| TemplateError::Init(format!("Failed to register template: {}", e)))?;

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
    use log::{debug, warn};
    use std::{sync::Arc, thread::sleep, time::Duration};

    pub struct Service {
        player_manager: PlayerManager,
        presence_manager: PresenceManager,
        template_manager: TemplateManager,
        config_rx: ConfigChangeReceiver,
    }

    impl Service {
        pub fn new() -> Result<Self, ServiceInitError> {
            let cfg = config::get();
            let player_manager = PlayerManager::new()?;
            let presence_manager = PresenceManager::new();

            // Template setup
            let template_manager = TemplateManager::new("")?;

            Ok(Self {
                player_manager,
                presence_manager,
                template_manager,
                config_rx: cfg.subscribe(),
            })
        }

        pub async fn run(&mut self) -> Result<(), ServiceRuntimeError> {
            loop {
                tokio::select! {
                    // Handle config changes
                    Ok(change) = self.config_rx.recv() => {
                        match change {
                            ConfigChange::Updated | ConfigChange::Reloaded => {
                                self.reload_components()?;
                            },
                            ConfigChange::Error(e) => {
                                warn!("Config error: {}", e);
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
                        self.update_loop().await?;
                    }
                }
            }
        }

        fn reload_components(&mut self) -> Result<(), ServiceRuntimeError> {
            let ctx = config::get().context();
            let detail_template = ctx.template_config().detail;
            self.template_manager = TemplateManager::new(&detail_template)?;
            Ok(())
        }

        async fn update_loop(&mut self) -> Result<(), ServiceRuntimeError> {
            debug!("tick");

            // Update players and get changes
            let player_updates = self.player_manager.update_players()?;

            // Process updates to generate presence actions
            self.presence_manager.process_player_updates(player_updates);

            // Process presence queue
            self.presence_manager
                .process_queue(&self.player_manager, &self.template_manager)
                .await?;

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
