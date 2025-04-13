use std::{fmt::Display, time::Duration};

use log::{debug, info};
use mpris::{PlaybackStatus, Player};
use smol_str::SmolStr;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerIdentifier {
    pub player_bus_name: SmolStr,
    pub identity: SmolStr,
    pub unique_name: SmolStr,
}

impl Display for PlayerIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.identity, self.player_bus_name, self.unique_name
        )
    }
}

impl From<&Player> for PlayerIdentifier {
    fn from(player: &Player) -> Self {
        Self {
            player_bus_name: SmolStr::new(player.bus_name_player_name_part()),
            identity: SmolStr::new(player.identity()),
            unique_name: SmolStr::new(player.unique_name()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlaybackState {
    pub playback_status: Option<PlaybackStatus>,
    pub track_identifier: Option<Box<str>>,
    pub title: Option<Box<str>>,
    pub position: Option<u32>,
    pub volume: Option<u8>,
}

impl Display for PlaybackState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?}: {} [{}s, {}%]",
            self.playback_status,
            self.title.as_deref().unwrap_or("Unknown"),
            self.position.unwrap_or(0),
            self.volume.unwrap_or(0)
        )?;

        if let Some(id) = &self.track_identifier {
            write!(f, " id:{}", id)?;
        }

        Ok(())
    }
}

impl From<&Player> for PlaybackState {
    fn from(player: &Player) -> Self {
        let metadata = player.get_metadata().ok();

        let track_identifier = metadata
            .as_ref()
            .and_then(|m| {
                m.track_id()
                    .map(|s| s.to_string())
                    .or_else(|| m.url().map(|s| s.to_string()))
            })
            .map(|s| s.into_boxed_str());

        Self {
            playback_status: player.get_playback_status().ok(),
            track_identifier,
            title: metadata
                .as_ref()
                .and_then(|m| m.title().map(|s| s.to_string().into_boxed_str())),
            position: player
                .get_position()
                .map(|d| d.as_secs() as u32)
                .ok(),
            volume: player
                .get_volume()
                .map(|v| (v * 100.0) as u8)
                .ok(),
        }
    }
}

impl PlaybackState {
    pub fn has_significant_changes(&self, previous: &Self) -> bool {
        if self.track_identifier != previous.track_identifier {
            debug!("Track identity changed");
            return true;
        }

        if self.playback_status != previous.playback_status || self.volume != previous.volume {
            info!(
                "Player changed status: {:?} -> {:?}",
                previous.playback_status, self.playback_status,
            );
            return true;
        }

        false
    }

    pub fn has_position_jump(
        &self,
        previous: &Self,
        polling_interval: Duration,
        dbus_delay: Duration,
    ) -> bool {
        // Add a buffer to account for variations
        const BUFFER_DURATION: Duration = Duration::from_secs(2);

        let max_expected_change_duration = polling_interval + dbus_delay + BUFFER_DURATION;
        let max_expected_change = max_expected_change_duration.as_secs() as u32;

        if self.position < previous.position {
            debug!(
                "Position jumped backward: {}s -> {}s",
                previous.position.unwrap_or(0),
                self.position.unwrap_or(0)
            );
            return true;
        }

        let elapsed = self
            .position
            .unwrap_or(0)
            .saturating_sub(previous.position.unwrap_or(0));
        if elapsed > max_expected_change {
            debug!(
                "Position jumped forward: {}s -> {}s (expected max change: {}s)",
                previous.position.unwrap_or(0),
                self.position.unwrap_or(0),
                max_expected_change
            );
            return true;
        }

        false
    }
}
