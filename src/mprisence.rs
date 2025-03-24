use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use discord_presence::{models::Activity, Client};
use log::{debug, error, info, warn};
use mpris::Player;
use parking_lot::Mutex;
use thiserror::Error;

use crate::{config::get_config, player::PlayerState};

#[derive(Error, Debug)]
pub enum MprisenceError {
    #[error("Invalid player: {0}")]
    InvalidPlayer(String),

    #[error("Player not found")]
    PlayerNotFound,

    #[error("Player metadata error: {0}")]
    MetadataError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Mprisence client error: {0}")]
    ClientError(#[from] MprisenceClientError),
}

#[derive(Error, Debug)]
pub enum MprisenceClientError {
    #[error("Discord client not ready")]
    DiscordClientNotReady,

    #[error("Discord client disconnected")]
    DiscordClientDisconnected,

    #[error("Discord client error")]
    DiscordClientError,

    #[error("Discord error: {0}")]
    DiscordError(#[from] discord_presence::DiscordError),

    #[error("Activity update failed: {0}")]
    ActivityUpdateFailed(String),
}

pub struct MprisenceClient {
    client: Client,
    activity: Arc<Mutex<Option<Activity>>>,
    got_ready_event: Arc<AtomicBool>,
    got_connected_event: Arc<AtomicBool>,
    got_disconnect_event: Arc<AtomicBool>,
    got_error_event: Arc<AtomicBool>,
}

impl MprisenceClient {
    pub fn new(app_id: u64) -> Self {
        let discord_client = Client::new(app_id);
        let got_connected_event = Arc::new(AtomicBool::new(false));
        let got_ready_event = Arc::new(AtomicBool::new(false));
        let got_disconnected_event = Arc::new(AtomicBool::new(false));
        let got_error_event = Arc::new(AtomicBool::new(false));

        discord_client
            .on_connected({
                let got_connected_event = got_connected_event.clone();
                move |ctx| {
                    info!("Connected to Discord: {:?}", ctx);
                    got_connected_event.store(true, Ordering::SeqCst);
                }
            })
            .persist();

        discord_client
            .on_ready({
                let got_ready_event = got_ready_event.clone();
                move |ctx| {
                    info!("Discord client is ready: {:?}", ctx);
                    got_ready_event.store(true, Ordering::SeqCst);
                }
            })
            .persist();

        discord_client
            .on_disconnected({
                let got_disconnected_event = got_disconnected_event.clone();
                move |ctx| {
                    info!("Disconnected from Discord: {:?}", ctx);
                    got_disconnected_event.store(true, Ordering::SeqCst);
                }
            })
            .persist();

        discord_client
            .on_error({
                let got_error_event = got_error_event.clone();
                move |ctx| {
                    warn!("Discord error: {:?}", ctx);
                    got_error_event.store(true, Ordering::SeqCst);
                }
            })
            .persist();

        Self {
            client: discord_client,
            got_ready_event: got_ready_event.clone(),
            got_connected_event,
            got_disconnect_event: got_disconnected_event.clone(),
            got_error_event: got_error_event.clone(),
            activity: Arc::new(Mutex::new(None)),
        }
    }

    pub fn start(&mut self) {
        debug!("Starting Discord client");
        self.client.start();
    }

    pub fn update_activity(&mut self, activity: Activity) -> Result<(), MprisenceClientError> {
        if !self.got_ready_event() && !self.got_connected_event() {
            warn!("Discord client not ready");
            return Err(MprisenceClientError::DiscordClientNotReady);
        }
        if self.got_disconnect_event() {
            warn!("Discord client disconnected");
            return Err(MprisenceClientError::DiscordClientDisconnected);
        }
        if self.got_error_event() {
            warn!("Discord client encountered an error");
            return Err(MprisenceClientError::DiscordClientError);
        }

        match self.client.set_activity(|act| {
            println!("act: {:?}", act);
            activity
        }) {
            Ok(payload) => {
                match payload.data {
                    Some(data) => self.activity.lock().replace(data),
                    None => self.activity.lock().take(),
                };
                Ok(())
            }
            Err(e) => {
                warn!("Failed to update activity: {}", e);
                Err(MprisenceClientError::DiscordError(e))
            }
        }
    }

    pub fn clear_activity(&mut self) -> Result<(), MprisenceClientError> {
        match self.client.clear_activity() {
            Ok(payload) => {
                match payload.data {
                    Some(data) => self.activity.lock().replace(data),
                    None => self.activity.lock().take(),
                };
                Ok(())
            }
            Err(e) => Err(MprisenceClientError::DiscordError(e)),
        }
    }

    pub fn got_connected_event(&self) -> bool {
        self.got_connected_event.load(Ordering::Relaxed)
    }

    pub fn got_ready_event(&self) -> bool {
        self.got_ready_event.load(Ordering::Relaxed)
    }

    pub fn got_disconnect_event(&self) -> bool {
        self.got_disconnect_event.load(Ordering::Relaxed)
    }

    pub fn got_error_event(&self) -> bool {
        self.got_error_event.load(Ordering::Relaxed)
    }
}

pub struct Mprisence {
    player: Player,
    last_player_state: Option<PlayerState>,
    xdiscord_client: Option<MprisenceClient>,
}

impl Mprisence {
    pub fn new(player: Player) -> Self {
        let player_config = get_config().player_config(player.identity());
        let mut xdiscord_client = MprisenceClient::new(player_config.app_id.parse().unwrap());
        xdiscord_client.start();
        Self {
            player,
            last_player_state: None,
            xdiscord_client: Some(xdiscord_client),
        }
    }

    pub fn update(&mut self, player: Player) -> Result<(), MprisenceError> {
        if player.identity() != self.player.identity()
            || player.bus_name() != self.player.bus_name()
            || player.unique_name() != self.player.unique_name()
        {
            return Err(MprisenceError::InvalidPlayer(format!(
                "Expected {}, got {}",
                self.player.identity(),
                player.identity()
            )));
        }

        println!("Updating presence for player: {}", player.identity());

        let activity = Activity::default().details("test");
        match self.xdiscord_client.as_mut() {
            Some(client) => client
                .update_activity(activity)
                .map_err(MprisenceError::from),
            None => Err(MprisenceError::ClientError(
                MprisenceClientError::DiscordClientNotReady,
            )),
        }
    }

    pub fn destroy(&mut self) -> Result<(), MprisenceError> {
        if let Some(mut client) = self.xdiscord_client.take() {
            client.clear_activity().map_err(MprisenceError::from)?;
            let _ = client.client.shutdown();
        }
        Ok(())
    }
}
