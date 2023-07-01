pub mod config;
pub mod consts;
pub mod context;
pub mod error;

use handlebars::Handlebars;

use config::PlayerConfig;
use discord_rich_presence::{activity, DiscordIpc};
use lazy_static::lazy_static;
use mpris::{PlaybackStatus, Player, PlayerFinder};
use std::collections::BTreeMap;
use std::time::Duration;

use crate::config::Config;
use crate::context::Context;
use crate::error::Error;

lazy_static! {
    pub static ref CONFIG: Config = Config::load();
}

pub struct Mprisence {
    app_id: String,
    client: discord_rich_presence::DiscordIpcClient,
}

impl Mprisence {
    pub fn new() -> Self {
        let app_id = PlayerConfig::default().app_id;
        let client = discord_rich_presence::DiscordIpcClient::new(app_id.as_str()).unwrap();

        Mprisence { app_id, client }
    }

    pub async fn start(&mut self) {
        self.client.connect().unwrap();
        loop {
            match self.update().await {
                Ok(_) => {}
                Err(error) => {
                    println!("{:?}", error);
                }
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    pub async fn update(&mut self) -> Result<(), Error> {
        let player = match get_player() {
            Some(player) => player,
            None => return Err(Error::UpdateError("No player found".to_string())),
        };

        let player_identity = player.identity().to_lowercase().replace(" ", "_");
        let playback_status = player.get_playback_status()?;

        let context = Context::from_player(player);

        let data: BTreeMap<String, String> = context.into();

        let handlebars = Handlebars::new();

        let _default = Default::default();
        let player_config = CONFIG
            .player
            .get(player_identity.to_lowercase().replace(" ", "_").as_str())
            .unwrap_or(&_default);

        println!("{:?}", player_config);

        match playback_status {
            PlaybackStatus::Playing => {
                let mut activity = activity::Activity::new();
                let detail = handlebars.render_template(&CONFIG.template.detail, &data)?;
                let state = handlebars.render_template(&CONFIG.template.state, &data)?;

                if !detail.is_empty() {
                    activity = activity.details(detail.as_str());
                }

                if !state.is_empty() {
                    activity = activity.state(state.as_str());
                }

                match self.client.set_activity(activity) {
                    Ok(_) => {
                        println!("Set activity");
                    }
                    Err(_) => {
                        return Err(Error::DiscordError("Could not set activity".to_string()))
                    }
                }
            }
            PlaybackStatus::Paused => {}
            _ => {}
        }
        Ok(())
    }
}

fn get_player() -> Option<Player> {
    let player_finder = PlayerFinder::new().expect("Could not connect to D-Bus");

    let mut players = match player_finder.find_all() {
        Ok(players) => players,
        Err(_) => vec![],
    };

    players = players
        .into_iter()
        .filter(|player| {
            let name = player.identity().to_lowercase().replace(" ", "_");

            if !CONFIG.allow_streaming {
                if let Ok(metadata) = player.get_metadata() {
                    if let Some(_) = metadata.url().filter(|url| url.starts_with("http")) {
                        return false;
                    }
                }
            }
            match CONFIG.player.get(&name) {
                Some(player_config) => !player_config.ignore,
                None => true,
            }
        })
        .collect();

    players.sort_by(|a, b| {
        let a = a.identity().to_lowercase().replace(" ", "_");
        let b = b.identity().to_lowercase().replace(" ", "_");

        let _default = Default::default();
        let a = CONFIG.player.get(&a).unwrap_or(&_default);
        let b = CONFIG.player.get(&b).unwrap_or(&_default);

        a.cmp(&b)
    });

    players.into_iter().nth(0)
}
