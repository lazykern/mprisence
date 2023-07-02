pub mod activity;
pub mod client;
pub mod config;
pub mod consts;
pub mod context;
pub mod error;
pub mod hbs;
pub mod image;
pub mod player;

use client::Client;
use consts::{
    DEFAULT_DETAIL_TEMPLATE, DEFAULT_LARGE_TEXT_NO_ALBUM_IMAGE_TEMPLATE,
    DEFAULT_LARGE_TEXT_TEMPLATE, DEFAULT_SMALL_TEXT_TEMPLATE, DEFAULT_STATE_TEMPLATE,
};

use image::provider::Provider;
use image::ImageURLFinder;
use lazy_static::lazy_static;
use mpris::{PlaybackStatus, Player};
use std::collections::BTreeMap;
use std::time::Duration;

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
    handlebars: handlebars::Handlebars<'static>,
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
            handlebars: hbs::new_hbs(),
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
        let players = player::get_players();

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
        let position = player.get_position().unwrap_or(Duration::from_secs(0));

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

        if (playback_status == PlaybackStatus::Paused && CONFIG.clear_on_pause)
            && (playback_status == PlaybackStatus::Playing && position.as_millis() == 0)
        {
            log::info!("Clearing activity");
            client.clear()?;
            return Ok(());
        }

        log::info!("Creating activity");
        let mut activity = Activity::new();

        let data = context.data();
        log::debug!("Data: {:?}", data);

        log::debug!("self.handlebars: {:?}", self.handlebars);

        let detail = match self
            .handlebars
            .render_template(&CONFIG.template.detail, &data)
        {
            Ok(detail) => detail,
            Err(e) => {
                log::warn!("Error rendering detail template, using default: {:?}", e);
                self.handlebars
                    .render_template(DEFAULT_DETAIL_TEMPLATE, &data)
                    .unwrap()
            }
        };
        log::debug!("Detail: {}", detail);

        let state = match self
            .handlebars
            .render_template(&CONFIG.template.state, &data)
        {
            Ok(state) => state,
            Err(e) => {
                log::warn!("Error rendering state template, using default: {:?}", e);
                self.handlebars
                    .render_template(DEFAULT_STATE_TEMPLATE, &data)
                    .unwrap()
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

        let mut pic_url = match context.metadata() {
            Some(metadata) => self.image_url_finder.from_metadata(metadata).await,
            None => {
                log::info!("No audio metadata found, could not get album image from metadata");
                None
            }
        };

        if pic_url.is_none() {
            if identity == "cmus" {
                let audio_path = player::cmus::get_audio_path();

                match audio_path {
                    Some(audio_path) => {
                        log::info!("CMUS: Found audio path: {}", audio_path);
                        pic_url = self.image_url_finder.from_audio_path(audio_path).await;
                    }
                    None => {
                        log::warn!("CMUS: No audio path, not setting album image");
                    }
                }
            }
        }

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

                large_text = match self
                    .handlebars
                    .render_template(&CONFIG.template.large_text, &data)
                {
                    Ok(large_text) => large_text,
                    Err(_) => {
                        log::warn!("Error rendering large text template, using default");
                        self.handlebars
                            .render_template(&DEFAULT_LARGE_TEXT_TEMPLATE, &data)
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

                        small_text = match self
                            .handlebars
                            .render_template(&CONFIG.template.small_text, &data)
                        {
                            Ok(small_text) => small_text,
                            Err(_) => {
                                log::warn!("Error rendering small text template, using default");
                                self.handlebars
                                    .render_template(&DEFAULT_SMALL_TEXT_TEMPLATE, &data)
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

                large_text = match self
                    .handlebars
                    .render_template(&CONFIG.template.large_text_no_album_image, &data)
                {
                    Ok(large_text) => large_text,
                    Err(_) => {
                        log::warn!("Error rendering large text template, using default");
                        self.handlebars
                            .render_template(&DEFAULT_LARGE_TEXT_NO_ALBUM_IMAGE_TEMPLATE, &data)
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
            activity.set_timestamps_from_context(context);
        }

        client.set_activity(&activity)?;

        Ok(())
    }
}
