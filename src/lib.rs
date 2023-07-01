pub mod client;
pub mod config;
pub mod consts;
pub mod context;
pub mod error;
pub mod picture;

use client::Client;
use discord_rich_presence::activity::{Assets, Timestamps};
use handlebars::Handlebars;

use discord_rich_presence::activity;
use lazy_static::lazy_static;
use mpris::{PlaybackStatus, Player, PlayerFinder};
use picture::provider::Provider;
use picture::PictureURLFinder;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::context::Context;
use crate::error::Error;

lazy_static! {
    pub static ref CONFIG: Config = Config::load();
}

pub struct Mprisence {
    current_player: String,
    clients: HashMap<String, Client>,
    picture_url_finder: PictureURLFinder,
}

impl Mprisence {
    pub fn new() -> Self {
        let mut clients: HashMap<String, Client> = HashMap::new();

        for (player, player_config) in CONFIG.player.iter() {
            let app_id = player_config.app_id.clone();
            clients.insert(player.to_string().replace(" ", "_"), Client::new(app_id));
        }

        let fallback_app_id = consts::DEFAULT_APP_ID;
        let fallback_client = Client::new(fallback_app_id);

        clients.insert("fallback".to_string(), fallback_client);

        let provider = match CONFIG.image.provider.provider.to_lowercase().as_str() {
            "imgbb" => Some(Provider::new_imgbb(
                &CONFIG
                    .image
                    .provider
                    .imgbb
                    .api_key
                    .clone()
                    .unwrap_or_default(),
            )),
            _ => None,
        };

        Mprisence {
            current_player: "fallback".to_string(),
            clients,
            picture_url_finder: PictureURLFinder::new(provider),
        }
    }

    fn close_all_clients(&mut self) -> Result<(), Error> {
        for (_, client) in self.clients.iter_mut() {
            client.close()?;
        }
        Ok(())
    }

    pub async fn start(&mut self) {
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

    pub fn clear_all_activities(&mut self) -> Result<(), Error> {
        for (_, client) in self.clients.iter_mut() {
            client.clear_activity()?;
        }
        Ok(())
    }

    pub async fn update(&mut self) -> Result<(), Error> {
        let player = match get_player() {
            Some(player) => player,
            None => {
                self.close_all_clients()?;
                return Err(Error::UpdateError("No player found".to_string()));
            }
        };

        let player_identity = player.identity().to_lowercase().replace(" ", "_");

        if player_identity != self.current_player {
            self.close_all_clients()?;
            self.current_player = player_identity.clone();
        }

        let playback_status = player.get_playback_status()?;

        let context = Context::from_player(player);

        let data = context.data();

        let handlebars = Handlebars::new();

        let mut client = self
            .clients
            .get_mut(player_identity.to_lowercase().replace(" ", "_").as_str());

        if client.is_none() {
            client = self.clients.get_mut("default");
        }

        let mut _default = Default::default();
        if client.is_none() {
            client = Some(&mut _default);
        }

        let client = client.unwrap();

        client.connect()?;

        let mut activity = activity::Activity::new();
        let detail = handlebars.render_template(&CONFIG.template.detail, &data)?;
        let state = handlebars.render_template(&CONFIG.template.state, &data)?;

        if !detail.is_empty() {
            activity = activity.details(detail.as_str());
        }

        if !state.is_empty() {
            activity = activity.state(state.as_str());
        }

        let pic_url = match context.metadata() {
            Some(metadata) => self.picture_url_finder.from_metadata(metadata).await,
            None => None,
        };

        let pic_url = pic_url.unwrap_or_default();
        if !pic_url.is_empty() {
            let assets = Assets::new().large_image(pic_url.as_str());
            activity = activity.assets(assets);
        }

        match playback_status {
            PlaybackStatus::Playing => {
                if CONFIG.time.show {
                    if let Some(timestamps) = get_timestamps(&context) {
                        activity = activity.timestamps(timestamps);
                    }
                }
                client.set_activity(activity)?
            }
            PlaybackStatus::Paused => match CONFIG.clear_on_pause {
                true => client.clear_activity()?,
                false => client.set_activity(activity)?,
            },
            _ => client.clear_activity()?,
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
            match &CONFIG.player.get(&name) {
                Some(player_config) => !player_config.ignore,
                None => true,
            }
        })
        .collect();

    players.sort_by(|a, b| {
        let a_identity = a.identity().to_lowercase().replace(" ", "_");
        let b_identity = b.identity().to_lowercase().replace(" ", "_");

        let a_state = a.get_playback_status().unwrap_or(PlaybackStatus::Stopped);
        let b_state = b.get_playback_status().unwrap_or(PlaybackStatus::Stopped);

        let _default = Default::default();
        let a_i = &CONFIG.player.get(&a_identity).unwrap_or(&_default);
        let b_i = &CONFIG.player.get(&b_identity).unwrap_or(&_default);

        match &CONFIG.playing_first {
            true => {
                if a_state == PlaybackStatus::Playing && b_state != PlaybackStatus::Playing {
                    Ordering::Less
                } else if a_state != PlaybackStatus::Playing && b_state == PlaybackStatus::Playing {
                    Ordering::Greater
                } else {
                    a_i.cmp(&b_i)
                }
            }
            false => a_i.cmp(&b_i),
        }
    });

    players.into_iter().nth(0)
}

fn get_timestamps(context: &Context) -> Option<Timestamps> {
    let mut timestamps = Timestamps::new();
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(t) => {
            let p = context.player().unwrap();
            let position = p.get_position();
            let position_dur = position.unwrap_or(Duration::from_secs(0));
            let start_dur = match t > position_dur {
                true => t - position_dur,
                false => t,
            };

            timestamps = timestamps.start(start_dur.as_secs() as i64);

            if !CONFIG.time.as_elapsed {
                let m = match context.metadata() {
                    Some(m) => m,
                    None => return None,
                };
                let length_dur = match m.length() {
                    Some(l) => l,
                    None => return None,
                };

                let end_dur = start_dur + length_dur;

                timestamps = timestamps.end(end_dur.as_secs() as i64);
            }
        }
        _ => return None,
    }

    Some(timestamps)
}
