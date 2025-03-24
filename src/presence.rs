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
use log::{debug, info, trace, warn};
use mime_guess::mime;
use mpris::{PlaybackStatus, Player};
use parking_lot::Mutex;

use crate::{
    config::{
        get_config,
        schema::{ActivityType, ActivityTypesConfig, PlayerConfig},
    },
    cover::CoverManager,
    error::DiscordError,
    player::PlaybackState,
    template::TemplateManager,
    utils,
};

pub struct Presence {
    player: Player,
    template_manager: Arc<TemplateManager>,
    cover_manager: Arc<CoverManager>,
    last_player_state: Option<PlaybackState>,
    discord_client: Arc<Mutex<DiscordIpcClient>>,
    should_connect: AtomicBool,
    should_reconnect: AtomicBool,
}

impl Presence {
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

    pub async fn update(&mut self, player: Player) -> Result<(), DiscordError> {
        self.validate_player(&player)?;

        self.ensure_connection()?;

        let new_state = PlaybackState::from(&player);
        let should_update = self
            .last_player_state
            .as_ref()
            .map(|previous_state| {
                new_state.has_significant_changes(previous_state)
                    || new_state.has_position_jump(
                        previous_state,
                        Duration::from_millis(get_config().interval()),
                    )
            })
            .unwrap_or(true);

        if !should_update {
            trace!("Skipping update due to no relevant changes");
            self.last_player_state = Some(new_state);
            return Ok(());
        }

        self.last_player_state = Some(new_state);
        self.update_activity(player).await.map_err(|err| {
            if matches!(
                err,
                DiscordError::ConnectionError(_) | DiscordError::ReconnectionError(_)
            ) {
                self.last_player_state = None;
                self.should_reconnect.store(true, Ordering::Relaxed);
            }
            err
        })
    }

    fn validate_player(&self, player: &Player) -> Result<(), DiscordError> {
        if player.identity() != self.player.identity()
            || player.bus_name() != self.player.bus_name()
            || player.unique_name() != self.player.unique_name()
        {
            return Err(DiscordError::InvalidPlayer(format!(
                "Expected {}, got {}",
                self.player.identity(),
                player.identity()
            )));
        }
        Ok(())
    }

    fn ensure_connection(&self) -> Result<(), DiscordError> {
        if self.should_connect.load(Ordering::Relaxed) {
            self.discord_client
                .lock()
                .connect()
                .map_err(|err| DiscordError::ConnectionError(err.to_string()))?;
            self.should_connect.store(false, Ordering::Relaxed);
        }

        if self.should_reconnect.load(Ordering::Relaxed) {
            self.discord_client
                .lock()
                .reconnect()
                .map_err(|err| DiscordError::ReconnectionError(err.to_string()))?;
            self.should_reconnect.store(false, Ordering::Relaxed);
        }
        Ok(())
    }

    pub fn destroy(&mut self) -> Result<(), DiscordError> {
        self.discord_client.lock().close().unwrap();
        Ok(())
    }

    fn determine_activity_type(
        &self,
        activity_type_config: &ActivityTypesConfig,
        player_config: &PlayerConfig,
        metadata_url: Option<&str>,
    ) -> ActivityType {
        if let Some(override_type) = player_config.override_activity_type {
            return override_type;
        }

        if activity_type_config.use_content_type && metadata_url.is_some() {
            if let Some(content_type) = metadata_url.and_then(utils::get_content_type_from_metadata)
            {
                match content_type.type_() {
                    mime::AUDIO => return ActivityType::Listening,
                    mime::VIDEO | mime::IMAGE => return ActivityType::Watching,
                    _ => {}
                }
            }
        }

        activity_type_config.default
    }

    async fn update_activity(&self, player: Player) -> Result<(), DiscordError> {
        let playback_status = player.get_playback_status().unwrap();
        let config = get_config();

        if playback_status == PlaybackStatus::Stopped
            || (config.clear_on_pause() && playback_status == PlaybackStatus::Paused)
        {
            debug!(
                "Player is {}, clearing activity",
                if playback_status == PlaybackStatus::Stopped {
                    "stopped"
                } else {
                    "paused"
                }
            );
            self.discord_client
                .lock()
                .clear_activity()
                .map_err(|err| DiscordError::ClearActivityError(err.to_string()))?;
            return Ok(());
        }

        let metadata = player.get_metadata().unwrap();
        let player_config = config.player_config(player.identity());
        let as_elapsed = config.time_config().as_elapsed;

        let (start_s, end_s) = if playback_status == PlaybackStatus::Playing {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards");

            let position = player.get_position().unwrap_or_default();
            let start_dur = now.checked_sub(position).unwrap_or_default();
            let start_s = Some(start_dur.as_secs() as u64);

            let length = metadata.length().unwrap_or_default();
            let end_s = if !as_elapsed && !length.is_zero() {
                start_dur
                    .checked_add(length)
                    .map(|end| end.as_secs() as u64)
            } else {
                None
            };

            (start_s, end_s)
        } else {
            (None, None)
        };

        let activity_texts = self.template_manager.render_activity_texts(player)?;

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

        let activity_type = self.determine_activity_type(
            &config.activity_type_config(),
            &player_config,
            metadata.url(),
        );

        let mut activity = Activity::default().activity_type(activity_type.into());

        if !activity_texts.details.is_empty() {
            activity = activity.details(&activity_texts.details);
        }

        if !activity_texts.state.is_empty() {
            activity = activity.state(&activity_texts.state);
        }

        if let Some(start) = start_s {
            activity = activity.timestamps({
                let ts = Timestamps::default().start(start as i64);
                if let Some(end) = end_s {
                    ts.end(end as i64)
                } else {
                    ts
                }
            });
        }

        let mut assets = Assets::default();

        if let Some(img_url) = &cover_art_url {
            debug!("Setting Discord large image to: {}", img_url);
            assets = assets.large_image(img_url);
            if !activity_texts.large_text.is_empty() {
                assets = assets.large_text(&activity_texts.large_text);
            }
        }

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

        activity = activity.assets(assets);

        self.discord_client
            .lock()
            .set_activity(activity)
            .map_err(|err| DiscordError::ActivityError(err.to_string()))?;

        Ok(())
    }
}
