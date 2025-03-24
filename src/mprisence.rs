use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use discord_rich_presence::{
    activity::{Activity, Assets, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use log::{debug, error, info, trace, warn};
use mpris::{DBusError, PlaybackStatus, Player};
use parking_lot::Mutex;
use thiserror::Error;

use crate::{
    config::get_config, cover::CoverManager, error::TemplateError, player::PlayerState,
    template::TemplateManager, utils,
};

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

    #[error("Discord error: {0}")]
    DiscordError(String),

    #[error("Failed to create player finder")]
    DBus(#[from] DBusError),

    #[error("Template error: {0}")]
    Template(#[from] TemplateError),
}

pub struct Mprisence {
    player: Player,
    template_manager: Arc<TemplateManager>,
    cover_manager: Arc<CoverManager>,
    last_player_state: Option<PlayerState>,
    discord_client: Arc<Mutex<DiscordIpcClient>>,
    should_connect: AtomicBool,
    should_reconnect: AtomicBool,
}

impl Mprisence {
    pub fn new(
        player: Player,
        template_manager: Arc<TemplateManager>,
        cover_manager: Arc<CoverManager>,
    ) -> Self {
        let config = get_config();
        let player_config = config.player_config(player.identity());
        Self {
            player,
            template_manager,
            cover_manager,
            last_player_state: None,
            discord_client: Arc::new(Mutex::new(
                DiscordIpcClient::new(&player_config.app_id).unwrap(),
            )),
            should_connect: AtomicBool::new(true),
            should_reconnect: AtomicBool::new(false),
        }
    }

    pub async fn update(&mut self, player: Player) -> Result<(), MprisenceError> {
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

        if self.should_connect.load(Ordering::Relaxed) {
            if let Err(err) = self.discord_client.lock().connect() {
                return Err(MprisenceError::DiscordError(err.to_string()));
            }
            self.should_connect.store(false, Ordering::Relaxed);
        }

        if self.should_reconnect.load(Ordering::Relaxed) {
            if let Err(err) = self.discord_client.lock().reconnect() {
                return Err(MprisenceError::DiscordError(err.to_string()));
            }
            self.should_reconnect.store(false, Ordering::Relaxed);
        }

        let new_state = PlayerState::from(&player);

        let should_update = match &self.last_player_state {
            Some(previous_state) => {
                let has_relevant_changes = new_state.has_metadata_changes(previous_state);
                let has_position_jump = new_state.has_position_jump(
                    previous_state,
                    Duration::from_millis(get_config().interval()),
                );

                has_relevant_changes || has_position_jump
            }
            None => true, // Always update if there's no previous state
        };

        if !should_update {
            trace!("Skipping update due to no relevant changes");
            self.last_player_state = Some(new_state); // Still update the state even if we don't update Discord
            return Ok(());
        }

        self.last_player_state = Some(new_state);

        if let Err(err) = self.update_activity(player).await {
            match err {
                MprisenceError::DiscordError(err) => {
                    self.last_player_state = None;
                    self.should_reconnect.store(true, Ordering::Relaxed);
                    return Err(MprisenceError::DiscordError(err.to_string()));
                }
                _ => (),
            }
        }
        Ok(())
    }

    pub fn destroy(&mut self) -> Result<(), MprisenceError> {
        self.discord_client.lock().close().unwrap();
        Ok(())
    }

    async fn update_activity(&self, player: Player) -> Result<(), MprisenceError> {
        let playback_status = player.get_playback_status().unwrap();

        if player.get_playback_status().unwrap() == PlaybackStatus::Stopped {
            debug!("Player is stopped, returning empty activity");
            if let Err(err) = self.discord_client.lock().clear_activity() {
                return Err(MprisenceError::DiscordError(err.to_string()));
            }
            return Ok(());
        }

        let config = get_config();
        let player_config = config.player_config(player.identity());
        let as_elapsed = config.time_config().as_elapsed;

        if config.clear_on_pause() && playback_status == PlaybackStatus::Paused {
            debug!("Player is paused, clearing activity");
            if let Err(err) = self.discord_client.lock().clear_activity() {
                return Err(MprisenceError::DiscordError(err.to_string()));
            }
            return Ok(());
        }

        let metadata = player.get_metadata().unwrap();

        let length = metadata.length().unwrap_or_default();

        // Calculate timestamps if playing
        let (start_s, end_s) = if playback_status == PlaybackStatus::Playing {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards");

            let start_dur = now
                .checked_sub(player.get_position().unwrap_or_default())
                .unwrap_or_default();
            let start_s = start_dur.as_secs() as i64;

            let mut end_s = None;
            if !as_elapsed && !length.is_zero() {
                let end = start_dur.checked_add(length).unwrap_or_default();
                end_s = Some(end.as_secs() as i64);
            }

            (Some(start_s as u64), end_s.map(|s| s as u64))
        } else {
            (None, None)
        };

        let mut activity = Activity::default();

        if let Some(url) = metadata.url() {
            let content_type = utils::get_content_type_from_metadata(url);
            let activity_type = player_config.activity_type(content_type.as_deref());

            activity = activity.activity_type(activity_type.into());
        } else {
            let activity_type = player_config.activity_type(None);
            activity = activity.activity_type(activity_type.into());
        }

        let activity_texts = self.template_manager.render_activity_texts(player)?;

        if !activity_texts.details.is_empty() {
            activity = activity.details(&activity_texts.details);
        }

        if !activity_texts.state.is_empty() {
            activity = activity.state(&activity_texts.state);
        }

        if let Some(start) = start_s {
            activity = activity.timestamps({
                let ts = Timestamps::default();
                if let Some(end) = end_s {
                    ts.start(start as i64).end(end as i64)
                } else {
                    ts.start(start as i64)
                }
            });
        }

        // Get cover art URL using cover art manager
        let cover_art_url = match self.cover_manager.get_cover_art(&metadata).await {
            Ok(Some(url)) => {
                info!("Found cover art URL for Discord");
                debug!("Using cover art URL: {}", url);
                Some(url)
            }
            Ok(None) => {
                debug!("No cover art URL available for Discord");
                None
            }
            Err(e) => {
                warn!("Failed to get cover art: {}", e);
                debug!(
                    "Discord requires HTTP/HTTPS URLs for images, not file paths or base64 data"
                );
                None
            }
        };

        activity = activity.assets({
            let mut assets = Assets::default();

            // Set large image (album art) if available
            if let Some(img_url) = &cover_art_url {
                debug!("Setting Discord large image to: {}", img_url);
                assets = assets.large_image(img_url);
                if !activity_texts.large_text.is_empty() {
                    assets = assets.large_text(&activity_texts.large_text);
                }
            }

            // Set small image (player icon) if enabled
            if player_config.show_icon {
                debug!(
                    "Setting Discord small image to player icon: {}",
                    player_config.icon
                );
                assets = assets.small_image(player_config.icon.as_str());
                if !activity_texts.small_text.is_empty() {
                    assets = assets.small_text(&activity_texts.small_text);
                }
            }

            assets
        });

        if let Err(err) = self.discord_client.lock().set_activity(activity) {
            return Err(MprisenceError::DiscordError(err.to_string()));
        }
        Ok(())
    }
}
