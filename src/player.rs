use std::{
    collections::{hash_map::Entry, HashMap},
    fmt::Display,
    time::Duration,
};

use log::{debug, info, warn};
use mpris::{Metadata, PlaybackStatus, Player, PlayerFinder};
use smol_str::SmolStr;

use crate::{config::get_config, error::PlayerError};

#[derive(Debug, Clone)]
pub enum PlayerStateChange {
    Updated(PlayerId, PlayerState),
    Removed(PlayerId),
    Cleared(PlayerId),
}

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
    pub position: Option<u32>,
    pub volume: Option<u8>,
}

impl Display for PlayerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Basic playback info
        write!(
            f,
            "{:?}: {} [{}s, {}%]",
            self.playback_status,
            self.title.as_deref().unwrap_or("Unknown"),
            self.position.unwrap_or(0),
            self.volume.unwrap_or(0)
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
            position: player.get_position().map_err(PlayerError::DBus).map(|d| d.as_secs() as u32).ok(),
            volume: player.get_volume().map_err(PlayerError::DBus).map(|v| (v * 100.0) as u8).ok(),
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
            info!(
                "Player changed status: {:?} -> {:?}",
                previous.playback_status, self.playback_status,
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
                previous.position.unwrap_or(0), self.position.unwrap_or(0)
            );
            return true;
        }

        // Check for forward jump that exceeds expected progression
        let elapsed = self.position.unwrap_or(0).saturating_sub(previous.position.unwrap_or(0));
        if elapsed > max_expected_change {
            debug!(
                "Position jumped forward: {}s -> {}s",
                previous.position.unwrap_or(0), self.position.unwrap_or(0)
            );
            return true;
        }

        false
    }

    /// Determines if a presence update is needed
    pub fn requires_presence_update(&self, previous: &Self, polling_interval: Duration) -> bool {
        self.has_metadata_changes(previous) || self.has_position_jump(previous, polling_interval)
    }
}

pub struct PlayerManager {
    player_finder: PlayerFinder,
    players: HashMap<PlayerId, Player>, // Store actual players for metadata access
    player_states: HashMap<PlayerId, PlayerState>,
}

impl PlayerManager {
    pub fn new() -> Result<Self, PlayerError> {
        info!("Initializing PlayerManager");
        let finder = PlayerFinder::new().map_err(PlayerError::DBus)?;

        Ok(Self {
            player_finder: finder,
            players: HashMap::new(),
            player_states: HashMap::new(),
        })
    }

    pub async fn get_changes(&mut self) -> Result<Vec<PlayerStateChange>, PlayerError> {
        let config = get_config();
        let polling_interval = config.interval();

        let current = self
            .player_finder
            .find_all()
            .map_err(PlayerError::Finding)?;

        let current_ids: Vec<_> = current.iter().map(PlayerId::from).collect();
        let mut state_changes = Vec::new();

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
            state_changes.push(PlayerStateChange::Removed(id));
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

                    match self.player_states.entry(id.clone()) {
                        Entry::Occupied(mut entry) => {
                            let has_changes = player_state.requires_presence_update(
                                entry.get(),
                                Duration::from_millis(polling_interval),
                            );

                            if has_changes {
                                let key = entry.key().clone();
                                debug!("Player {} updated: {}", key, player_state);
                                state_changes.push(PlayerStateChange::Updated(key, player_state.clone()));
                            }
                            entry.insert(player_state);
                        }
                        Entry::Vacant(entry) => {
                            let key = entry.key().clone();
                            info!("New player detected: {} playing {}", key, player_state);
                            entry.insert(player_state.clone());
                            state_changes.push(PlayerStateChange::Updated(key, player_state));
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to get player state for {}: {}", id, e);
                    state_changes.push(PlayerStateChange::Cleared(id));
                }
            }
        }

        Ok(state_changes)
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
