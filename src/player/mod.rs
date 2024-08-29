pub use mpris::{Player, PlayerFinder};

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

    let mut players = vec![];

    match player_finder.iter_players() {
        Ok(found_players) => {
            for player in found_players {
                match player {
                    Ok(player) => {
                        log::info!("Found player: {:?}", player);
                        players.push(player);
                    }
                    Err(e) => {
                        log::error!("Error iterating players: {:?}", e);
                    }
                }
            }
        }
        Err(e) => {
            log::error!("Error finding players: {:?}", e);
        }
    }

    players
}
