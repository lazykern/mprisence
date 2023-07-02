pub mod activity;
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
use handlebars::Handlebars;

use image::provider::Provider;
use image::ImageURLFinder;
use lazy_static::lazy_static;
use mpris::{PlaybackStatus, Player, PlayerFinder};
use std::collections::BTreeMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::activity::Activity;
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
        log::info!("Creating mprisence instance");

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
        log::info!("Starting mprisence discord rich presence");
        loop {
            match self.update().await {
                Ok(_) => {
                    log::info!("Updated rich presence")
                }
                Err(error) => {
                    log::error!("Error updating rich presence: {:?}", error);
                }
            }
            log::info!("Waiting 2 second");
            std::thread::sleep(Duration::from_secs(2));
        }
    }

    pub async fn update(&mut self) -> Result<(), Error> {
        log::info!("Updating rich presence");
        let players = get_players();

        self.clean_client_map(&players);

        for player in players {
            let context = Context::from_player(player);
            match self.update_by_context(&context).await {
                Ok(_) => {}
                Err(error) => {
                    log::error!("Error updating rich presence: {:?}", error);
                }
            }
        }

        Ok(())
    }

    fn clean_client_map(&mut self, players: &Vec<Player>) {
        log::info!("Cleaning client map");

        // Find the clients that have disconnected
        let client_to_remove = self
            .client_map
            .keys()
            .filter(|unique_name| {
                // A player has disconnected if they are not in the list of players
                // given to us by the server.
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
                    log::error!("Error clearing client {}: {:?}", client.app_id(), error);
                }
            }

            match client.close() {
                Ok(_) => {
                    self.client_map.remove(&unique_name);
                }
                Err(error) => {
                    client.reconnect().unwrap_or_default();
                    log::error!("Error closing client {}: {:?}", client.app_id(), error);
                }
            }
        }
    }

    async fn update_by_context(&mut self, context: &Context) -> Result<(), Error> {
        log::info!("Updating rich presence by context");

        let player = match context.player() {
            Some(player) => player,
            None => return Err(Error::UpdateError("No player in context".to_owned())),
        };
        log::debug!("Player: {:?}", player);

        let identity = player.identity().to_lowercase().replace(" ", "_");
        let unique_name = player.unique_name().to_owned();

        log::info!("Player: {}, Unique Name: {}", identity, unique_name);

        let playback_status = player
            .get_playback_status()
            .unwrap_or(PlaybackStatus::Stopped);

        log::debug!("Getting client from client map");
        let c = Client::new(&identity, &unique_name);
        let client = match self.client_map.get_mut(&unique_name) {
            Some(client) => client,
            None => {
                self.client_map.insert(unique_name.clone(), c);
                self.client_map.get_mut(&unique_name).unwrap()
            }
        };
        if playback_status == PlaybackStatus::Stopped {
            client.close()?;
            return Ok(());
        }
        log::info!("Connecting client");
        client.connect()?;
        log::debug!("Client after connecting: {:?}", client);

        if playback_status == PlaybackStatus::Paused && CONFIG.clear_on_pause {
            log::info!("Clearing activity");
            client.clear()?;
            return Ok(());
        }

        log::info!("Creating activity");
        let mut activity = Activity::new();

        let data = context.data();
        log::debug!("Data: {:?}", data);

        let reg = Handlebars::new();
        log::debug!("Reg: {:?}", reg);

        let detail = match reg.render_template(&CONFIG.template.detail, &data) {
            Ok(detail) => detail,
            Err(e) => {
                log::warn!("Error rendering detail template, using default: {:?}", e);
                reg.render_template(DEFAULT_DETAIL_TEMPLATE, &data).unwrap()
            }
        };
        log::debug!("Detail: {}", detail);

        let state = match reg.render_template(&CONFIG.template.state, &data) {
            Ok(state) => state,
            Err(e) => {
                log::warn!("Error rendering state template, using default: {:?}", e);
                reg.render_template(DEFAULT_STATE_TEMPLATE, &data).unwrap()
            }
        };
        log::debug!("State: {}", state);

        if !detail.is_empty() {
            log::debug!("Setting activity detail to {}", detail);
            activity.set_details(detail);
        } else {
            log::warn!("Detail is empty, not setting activity detail")
        }

        if !state.is_empty() {
            log::debug!("Setting activity state to {}", state);
            activity.set_state(state);
        } else {
            log::warn!("State is empty, not setting activity state")
        }

        let pic_url = match context.metadata() {
            Some(metadata) => self.image_url_finder.from_metadata(metadata).await,
            None => {
                log::warn!("No audio metadata, not setting album art");
                None
            }
        };

        let large_image: String;
        let small_image: String;
        let large_text: String;
        let small_text: String;

        // If we have a pic_url, then we have a song with an album image
        if pic_url.is_some() {
            log::info!("Album image found, setting album art");

            large_image = pic_url.unwrap_or_default();
            log::debug!("Large image: {}", large_image);
            if !large_image.is_empty() {
                activity.set_large_image(&large_image);

                large_text = match reg.render_template(&CONFIG.template.large_text, &data) {
                    Ok(large_text) => large_text,
                    Err(_) => {
                        log::warn!("Error rendering large text template, using default");
                        reg.render_template(&DEFAULT_LARGE_TEXT_TEMPLATE, &data)
                            .unwrap()
                    }
                };
                log::debug!("Large text: {}", large_text);

                activity.set_large_text(&large_text);

                if (CONFIG.show_icon && client.has_icon)
                    || (CONFIG.show_icon && CONFIG.show_default_player_icon)
                {
                    small_image = client.icon().to_string();
                    log::debug!("Small image: {}", small_image);
                    if !small_image.is_empty() {
                        activity.set_small_image(&small_image);

                        small_text = match reg.render_template(&CONFIG.template.small_text, &data) {
                            Ok(small_text) => small_text,
                            Err(_) => {
                                log::warn!("Error rendering small text template, using default");
                                reg.render_template(&DEFAULT_SMALL_TEXT_TEMPLATE, &data)
                                    .unwrap()
                            }
                        };
                        log::debug!("Small text: {}", small_text);
                        activity.set_small_text(&small_text);
                    } else {
                        log::warn!("Small image is empty, not setting small image and small text");
                    }
                }
            } else {
                log::warn!("Large image is empty, not setting large image and large text");
            }
        } else {
            log::info!("Album image not found, using player icon");

            large_image = client.icon().to_string();
            log::debug!("Large image: {}", large_image);

            if !large_image.is_empty() {
                activity.set_large_image(&large_image);

                large_text =
                    match reg.render_template(&CONFIG.template.large_text_no_album_image, &data) {
                        Ok(large_text) => large_text,
                        Err(_) => {
                            log::warn!("Error rendering large text template, using default");
                            reg.render_template(&DEFAULT_LARGE_TEXT_NO_ALBUM_IMAGE_TEMPLATE, &data)
                                .unwrap()
                        }
                    };

                log::debug!("Large text: {}", large_text);

                activity.set_large_text(&large_text);
            } else {
                log::warn!("Large image is empty, not setting large image and large text");
            }
        }

        if playback_status == PlaybackStatus::Playing {
            set_timestamps(&mut activity, context);
        }

        client.set_activity(&activity)?;

        Ok(())
    }
}

fn get_players() -> Vec<Player> {
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

fn set_timestamps(activity: &mut Activity, context: &Context) {
    // Get the current time.
    let now = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(t) => t,
        Err(e) => {
            log::error!("Error getting current time: {:?}", e);
            return;
        }
    };

    // Get the current track's position.
    let position = match context.player() {
        Some(p) => p.get_position(),
        None => {
            log::warn!("No player in context, returning timestamps as none");
            return;
        }
    };

    let position_dur = match position {
        Ok(p) => p,
        Err(e) => {
            log::warn!("Error getting position: {:?}", e);
            return;
        }
    };
    log::debug!("Position: {:?}", position_dur);

    // Subtract the position from the current time. This will give us the amount
    // of time that has elapsed since the start of the track.
    let start_dur = match now > position_dur {
        true => now - position_dur,
        false => now,
    };
    log::debug!("Start duration: {:?}", start_dur);

    if CONFIG.time.as_elapsed {
        // Set the start timestamp.
        activity.set_start_time(start_dur);
    }

    // Get the current track's metadata.
    let m = match context.metadata() {
        Some(m) => m,
        None => {
            log::warn!("No metadata in context, returning timestamps as none");
            return;
        }
    };

    // Get the current track's length.
    let length = match m.length() {
        Some(l) => l,
        None => {
            log::warn!("No length in metadata, returning timestamps as none");
            return;
        }
    };

    // Add the start time to the track length. This gives us the time that the
    // track will end at.
    let end_dur = start_dur + length;
    log::debug!("End duration: {:?}", end_dur);

    // Set the end timestamp.
    activity.set_end_time(end_dur);
}
