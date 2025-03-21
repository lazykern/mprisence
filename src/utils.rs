use crate::player::{PlayerId, PlayerState};
use mpris::PlaybackStatus;
use std::collections::BTreeMap;

pub fn to_snake_case(input: &str) -> String {
    input
        .split_whitespace()
        .map(|word| word.to_lowercase())
        .collect::<Vec<_>>()
        .join("_")
}

pub fn format_duration(seconds: u64) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}
