pub mod activity;
pub mod client;
pub mod config;
pub mod consts;
pub mod context;
pub mod cover;
pub mod error;
pub mod hbs;
pub mod player;

use client::Client;

use config::PlayerConfig;
use hbs::HANDLEBARS;
use mpris::{PlaybackStatus, Player};
use std::collections::BTreeMap;
use std::time::Duration;

use crate::activity::Activity;
use crate::config::CONFIG;
use crate::context::Context;
use crate::error::Error;

pub struct Mprisence {
    client_map: BTreeMap<String, Client>,
}

impl Mprisence {
    pub fn new() -> Self {
        log::info!("Creating mprisence instance");

        Mprisence {
            client_map: BTreeMap::new(),
        }
    }

    pub async fn start(&mut self) {
        log::info!("Starting mprisence discord rich presence");
        loop {
            match self.update().await {
                Ok(_) => {}
                Err(error) => {
                    log::error!("Error updating rich presence: {:?}", error);
                }
            }
            log::info!("Waiting for {} milliseconds", CONFIG.interval);
            std::thread::sleep(Duration::from_millis(CONFIG.interval));
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

    async fn update_by_context(&mut self, context: &Context) -> Result<(), Error> {
        let identity = context.identity();
        let unique_name = context.unique_name();
        let config_identity = context.config_identity();
        let player_config = PlayerConfig::get_or_default(&config_identity);

        if context.is_ignored() {
            log::info!("Ignoring player {:?}", identity);
            return Ok(());
        }

        let c = Client::from_context(context);
        let client = match self.client_map.get_mut(&unique_name) {
            Some(client) => client,
            None => {
                self.client_map.insert(unique_name.clone(), c);
                self.client_map.get_mut(&unique_name).unwrap()
            }
        };

        if context.playback_status() == PlaybackStatus::Stopped
            || (context.playback_status() == PlaybackStatus::Paused && CONFIG.clear_on_pause)
        {
            log::info!("Clearing rich presence for {:?}", identity);
            client.clear()?;
            return Ok(());
        }

        if context.is_streaming() && !player_config.allow_streaming_or_default() {
            log::info!("Ignoring streaming player {:?}", identity);
            client.clear()?;
            return Ok(());
        }

        log::info!("Connecting to discord for {:?}", identity);
        if !client.is_connected() {
            client.connect()?;
        }

        let mut activity = Activity::new();

        let config_identity = context.config_identity();

        let player_config = PlayerConfig::get_or_default(&config_identity);

        let data = context.data();

        activity.set_details(render_details(&data));
        activity.set_state(render_state(&data));

        if context.playback_status() == PlaybackStatus::Playing && CONFIG.time.show {
            activity.set_timestamps_from_context(context);
        }

        activity.set_large_text(render_large_text(&data));
        activity.set_small_text(render_small_text(&data));


        if let Some(current_activity) = client.activity() {
            if current_activity == &activity {
                log::info!("Activity is the same, skipping update");
                return Ok(());
            }
        }

        if let Some(cover_url) = context.cover_url().await {
            activity.set_large_image(cover_url);


            if player_config.show_icon_or_default() {
                activity.set_small_image(player_config.icon_or_default());
            }
        } else {
            log::info!("No cover found");

            activity.set_large_image(player_config.icon_or_default());
        }

        client.set_activity(&activity)?;

        Ok(())
    }

    fn clean_client_map(&mut self, players: &Vec<Player>) {
        log::debug!("Cleaning client map");

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
                    log::error!("Error clearing client {}: {:?}", client.app_id(), error);
                }
            }

            match client.close() {
                Ok(_) => {
                    self.client_map.remove(&unique_name);
                }
                Err(error) => {
                    log::error!("Error closing client {}: {:?}", client.app_id(), error);
                }
            }
        }
    }
}

fn render_details(data: &BTreeMap<String, String>) -> String {
    match HANDLEBARS.render_template(&CONFIG.template.detail, &data) {
        Ok(detail) => detail,
        Err(e) => {
            log::warn!("Error rendering detail template, using default: {:?}", e);
            HANDLEBARS
                .render_template(consts::DEFAULT_DETAIL_TEMPLATE, &data)
                .unwrap()
        }
    }
}

fn render_state(data: &BTreeMap<String, String>) -> String {
    match HANDLEBARS.render_template(&CONFIG.template.state, &data) {
        Ok(state) => state,
        Err(e) => {
            log::warn!("Error rendering state template, using default: {:?}", e);
            HANDLEBARS
                .render_template(consts::DEFAULT_STATE_TEMPLATE, &data)
                .unwrap()
        }
    }
}

fn render_large_text(data: &BTreeMap<String, String>) -> String {
    match HANDLEBARS.render_template(&CONFIG.template.large_text, &data) {
        Ok(large_text) => large_text,
        Err(e) => {
            log::warn!(
                "Error rendering large text template, using default: {:?}",
                e
            );
            HANDLEBARS
                .render_template(consts::DEFAULT_LARGE_TEXT_TEMPLATE, &data)
                .unwrap()
        }
    }
}

fn render_small_text(data: &BTreeMap<String, String>) -> String {
    match HANDLEBARS.render_template(&CONFIG.template.small_text, &data) {
        Ok(small_text) => small_text,
        Err(e) => {
            log::warn!(
                "Error rendering small text template, using default: {:?}",
                e
            );
            HANDLEBARS
                .render_template(consts::DEFAULT_SMALL_TEXT_TEMPLATE, &data)
                .unwrap()
        }
    }
}
