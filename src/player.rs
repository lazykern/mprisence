use std::{fmt::Display, time::Duration};

use log::{debug, info};
use mpris::{PlaybackStatus, Player};
use smol_str::SmolStr;

use crate::error::PlayerError;

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
    pub playback_status: Option<PlaybackStatus>,
    pub track_id: Option<Box<str>>,
    pub url: Option<Box<str>>,
    pub title: Option<Box<str>>,
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

impl From<&Player> for PlayerState {
    fn from(player: &Player) -> Self {
        let metadata = player.get_metadata().ok();

        Self {
            playback_status: player.get_playback_status().ok(),
            track_id: metadata
                .as_ref()
                .and_then(|m| m.track_id().map(|s| s.to_string().into_boxed_str())),
            url: metadata
                .as_ref()
                .and_then(|m| m.url().map(|s| s.to_string().into_boxed_str())),
            title: metadata
                .as_ref()
                .and_then(|m| m.title().map(|s| s.to_string().into_boxed_str())),
            position: player
                .get_position()
                .map_err(PlayerError::DBus)
                .map(|d| d.as_secs() as u32)
                .ok(),
            volume: player
                .get_volume()
                .map_err(PlayerError::DBus)
                .map(|v| (v * 100.0) as u8)
                .ok(),
        }
    }
}

impl PlayerState {
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
                previous.position.unwrap_or(0),
                self.position.unwrap_or(0)
            );
            return true;
        }

        // Check for forward jump that exceeds expected progression
        let elapsed = self
            .position
            .unwrap_or(0)
            .saturating_sub(previous.position.unwrap_or(0));
        if elapsed > max_expected_change {
            debug!(
                "Position jumped forward: {}s -> {}s",
                previous.position.unwrap_or(0),
                self.position.unwrap_or(0)
            );
            return true;
        }

        false
    }
}
