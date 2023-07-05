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

    let players = match player_finder.find_all() {
        Ok(players) => players,
        Err(e) => {
            log::error!("Error finding players: {:?}", e);
            return vec![];
        }
    };

    players
}
