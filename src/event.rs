use std::fmt::Display;

use crate::player;

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
