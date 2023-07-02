pub mod client;
pub mod config;
pub mod consts;
pub mod context;
pub mod error;
pub mod image;

use client::Client;
use consts::{
    DEFAULT_DETAIL_TEMPLATE, DEFAULT_LARGE_TEXT_NO_ALBUM_IMAGE_TEMPLATE,
    DEFAULT_LARGE_TEXT_TEMPLATE, DEFAULT_SMALL_TEXT_TEMPLATE, DEFAULT_STATE_TEMPLATE,
};
use discord_rich_presence::activity::{Activity, Assets, Timestamps};
use handlebars::Handlebars;

use image::provider::Provider;
use image::ImageURLFinder;
use lazy_static::lazy_static;
use mpris::{PlaybackStatus, Player, PlayerFinder};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::context::Context;
use crate::error::Error;

lazy_static! {
    pub static ref CONFIG: Config = Config::load();
}

pub struct Mprisence {
    image_url_finder: ImageURLFinder,
    client_map: BTreeMap<String, Client>,
}

impl Mprisence {
    pub fn new() -> Self {
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
            image_url_finder: ImageURLFinder::new(provider),
            client_map: BTreeMap::new(),
        }
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

    pub async fn update(&mut self) -> Result<(), Error> {
        let players = get_players();

        self.clean_client_map(&players);

        for player in players {
            let context = Context::from_player(player);
            self.update_by_context(&context).await?;
        }

        Ok(())
    }

    fn clean_client_map(&mut self, players: &Vec<Player>) {
        let client_to_remove = self
            .client_map
            .keys()
            .filter(|unique_name| {
                !players
                    .iter()
                    .any(|p| p.unique_name() == unique_name.as_str())
            })
            .map(|unique_name| unique_name.to_owned())
            .collect::<Vec<String>>();

        for unique_name in client_to_remove {
            let client = self.client_map.get_mut(&unique_name).unwrap();
            match client.clear() {
                Ok(_) => {}
                Err(error) => {
                    println!("{:?}", error);
                }
            }

            match client.close() {
                Ok(_) => {
                    self.client_map.remove(&unique_name);
                }
                Err(error) => {
                    client.reconnect().unwrap_or_default();
                    println!("{:?}", error);
                }
            }
        }
    }

    async fn update_by_context(&mut self, context: &Context) -> Result<(), Error> {
        let player = match context.player() {
            Some(player) => player,
            None => return Err(Error::UpdateError("No player in context".to_owned())),
        };
        let identity = player.identity().to_lowercase().replace(" ", "_");
        let unique_name = player.unique_name().to_owned();

        let playback_status = player
            .get_playback_status()
            .unwrap_or(PlaybackStatus::Stopped);

        let c = Client::new(&identity, &unique_name);
        let client = match self.client_map.get_mut(&unique_name) {
            Some(client) => client,
            None => {
                self.client_map.insert(unique_name.clone(), c);
                self.client_map.get_mut(&unique_name).unwrap()
            }
        };

        client.connect()?;

        let mut activity = Activity::new();

        let data = context.data();
        let reg = Handlebars::new();
        let detail = match reg.render_template(&CONFIG.template.detail, &data) {
            Ok(detail) => detail,
            Err(_) => reg.render_template(DEFAULT_DETAIL_TEMPLATE, &data).unwrap(),
        };
        let state = match reg.render_template(&CONFIG.template.state, &data) {
            Ok(state) => state,
            Err(_) => reg.render_template(DEFAULT_STATE_TEMPLATE, &data).unwrap(),
        };

        if !detail.is_empty() {
            activity = activity.details(&detail);
        }

        if !state.is_empty() {
            activity = activity.state(&state);
        }

        let mut assets = Assets::new();
        let pic_url = match context.metadata() {
            Some(metadata) => self.image_url_finder.from_metadata(metadata).await,
            None => None,
        };

        let large_image: String;
        let small_image: String;
        let large_text: String;
        let small_text: String;

        if pic_url.is_some() {
            large_image = pic_url.unwrap_or_default();
            if !large_image.is_empty() {
                assets = assets.large_image(&large_image);

                large_text = match reg.render_template(&CONFIG.template.large_text, &data) {
                    Ok(large_text) => large_text,
                    Err(_) => reg
                        .render_template(&DEFAULT_LARGE_TEXT_TEMPLATE, &data)
                        .unwrap(),
                };
                assets = assets.large_text(&large_text);

                small_text = match reg.render_template(&CONFIG.template.small_text, &data) {
                    Ok(small_text) => small_text,
                    Err(_) => reg
                        .render_template(&DEFAULT_SMALL_TEXT_TEMPLATE, &data)
                        .unwrap(),
                };

                assets = assets.small_text(&small_text);
            }

            small_image = client.icon().to_string();
            if CONFIG.show_icon && client.has_icon && !small_image.is_empty() {
                assets = assets.small_image(&small_image);
            }
        } else {
            large_image = client.icon().to_string();
            if !large_image.is_empty() {
                assets = assets.large_image(&large_image);

                large_text =
                    match reg.render_template(&CONFIG.template.large_text_no_album_image, &data) {
                        Ok(large_text) => large_text,
                        Err(_) => reg
                            .render_template(&DEFAULT_LARGE_TEXT_NO_ALBUM_IMAGE_TEMPLATE, &data)
                            .unwrap(),
                    };
                assets = assets.large_text(&large_text);
            }
        }

        activity = activity.assets(assets);

        match playback_status {
            PlaybackStatus::Playing => {
                if let Some(timestamps) = get_timestamps(&context) {
                    activity = activity.timestamps(timestamps);
                }
                client.set_activity(activity)?;
            }
            PlaybackStatus::Paused => {
                if !CONFIG.clear_on_pause {
                    client.set_activity(activity)?;
                }
            }
            _ => client.clear()?,
        }

        Ok(())
    }
}

fn get_players() -> Vec<Player> {
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

    players
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
