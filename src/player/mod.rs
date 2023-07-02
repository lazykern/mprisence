use mpris::{Player, PlayerFinder};

use crate::CONFIG;

pub mod cmus;

pub fn get_players() -> Vec<Player> {
    log::info!("Searching for players");

    let player_finder = match PlayerFinder::new() {
        Ok(player_finder) => player_finder,
        Err(e) => {
            log::error!("Error creating player finder: {:?}", e);
            return vec![];
        }
    };

    let mut players = match player_finder.find_all() {
        Ok(players) => players,
        Err(e) => {
            log::error!("Error finding players: {:?}", e);
            return vec![];
        }
    };

    // Filter players
    players = players
        .into_iter()
        .filter(|player| {
            let name = player.identity().to_lowercase().replace(" ", "_");

            // Ignore streaming URLs if it's not enabled.
            if !CONFIG.allow_streaming {
                if let Ok(metadata) = player.get_metadata() {
                    if let Some(_) = metadata.url().filter(|url| url.starts_with("http")) {
                        return false;
                    }
                }
            }

            // Ignore players that are ignored.
            match &CONFIG.player.get(&name) {
                Some(player_config) => !player_config.ignore,
                None => true,
            }
        })
        .collect();

    players
}
