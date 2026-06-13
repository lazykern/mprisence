use crate::error::Error;
use crate::player::{is_mprisence_web_bridge_bus, is_playerctld_no_active_error};
use crate::utils::normalize_player_identity;
use mpris::PlayerFinder;

/// Live MPRIS players as `(display identity, normalized pattern key)`.
pub fn collect_live_player_identities() -> Result<Vec<(String, String)>, Error> {
    let mut finder = PlayerFinder::new()?;
    finder.set_player_timeout_ms(3000);
    let iter = finder.iter_players()?;

    let mut out = Vec::new();
    for player in iter {
        match player {
            Ok(player) => {
                if is_mprisence_web_bridge_bus(player.bus_name()) {
                    continue;
                }
                let identity = player.identity().to_string();
                let key = normalize_player_identity(&identity);
                out.push((identity, key));
            }
            Err(err) if is_playerctld_no_active_error(&err) => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(out)
}
