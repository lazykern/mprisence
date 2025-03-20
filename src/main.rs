use mpris::{Metadata, PlaybackStatus, Player, PlayerFinder};
use std::{collections::HashMap, fmt::Display, thread::sleep, time::Duration};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PlayerId {
    player_bus_name: String,
    identity: String,
}

impl Display for PlayerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.identity, self.player_bus_name)
    }
}

#[derive(Debug)]
struct PlayerState {
    playback_status: PlaybackStatus,
    metadata: Metadata,
}

#[derive(Error, Debug)]
pub enum MprisError {
    #[error("DBus error: {0}")]
    DBus(#[from] mpris::DBusError),
}

impl From<&Player> for PlayerId {
    fn from(player: &Player) -> Self {
        Self {
            player_bus_name: player.bus_name_player_name_part().to_string(),
            identity: player.identity().to_string(),
        }
    }
}

impl TryFrom<&Player> for PlayerState {
    type Error = MprisError;

    fn try_from(player: &Player) -> Result<Self, Self::Error> {
        Ok(Self {
            playback_status: player.get_playback_status()?,
            metadata: player.get_metadata()?,
        })
    }
}

impl PartialEq for PlayerState {
    fn eq(&self, other: &Self) -> bool {
        self.playback_status == other.playback_status
            && self.metadata.as_hashmap() == other.metadata.as_hashmap()
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let finder = PlayerFinder::new()?;
    let mut player_states: HashMap<PlayerId, PlayerState> = HashMap::new();

    loop {
        sleep(Duration::from_secs(1));

        let current = finder.find_all().unwrap_or_default();
        let current_ids: Vec<_> = current.iter().map(PlayerId::from).collect();

        // Handle removed players
        player_states.retain(|id, _| {
            let exists = current_ids.iter().any(|current_id| current_id == id);
            if !exists {
                println!("Player {} removed", id);
            }
            exists
        });

        // Handle new or updated players
        for player in current {
            let id = PlayerId::from(&player);
            if let Ok(player_state) = PlayerState::try_from(&player) {
                match player_states.get(&id) {
                    Some(old_player_state) if old_player_state != &player_state => {
                        println!("Player {}:{} updated", id.player_bus_name, id.identity);

                        player_states.insert(id, player_state);
                    }
                    None => {
                        println!("Player {}:{} added", id.player_bus_name, id.identity);
                        player_states.insert(id, player_state);
                    }
                    _ => {}
                }
            }
        }
    }
}
