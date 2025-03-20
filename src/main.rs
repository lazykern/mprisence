use mpris::{Metadata, PlayerFinder};
use std::{collections::HashMap, thread::sleep, time::Duration};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let players = PlayerFinder::new()?;
    let mut player_states: HashMap<String, Metadata> = HashMap::new();
    let poll_interval = Duration::from_millis(100);

    loop {
        sleep(poll_interval);

        let current_players = players.find_all().unwrap_or_else(|e| {
            eprintln!("Error finding players: {}", e);
            vec![]
        });

        // Remove players that no longer exist
        player_states.retain(|id, _| current_players.iter().any(|p| p.identity() == id));

        // Update or add new players
        for player in current_players {
            let id = player.identity().to_string();

            if let Ok(new_metadata) = player.get_metadata() {
                match player_states.get(&id) {
                    Some(old_metadata)
                        if old_metadata.as_hashmap() != new_metadata.as_hashmap() =>
                    {
                        println!("Player {} updated", id);
                        player_states.insert(id, new_metadata);
                    }
                    None => {
                        println!("Player {} added", id);
                        player_states.insert(id, new_metadata);
                    }
                    _ => {} // No change
                }
            }
        }
    }
}
