use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use discord_rich_presence::{
    activity::{Activity, Assets, Timestamps},
    DiscordIpc, DiscordIpcClient,
};
use log::{debug, error, info, trace, warn};
use mime_guess::mime;
use mpris::{PlaybackStatus, Player};
use parking_lot::Mutex;

use lofty::file::AudioFile as _;
use lofty::prelude::TaggedFileExt as _;

use crate::{
    config::{
        schema::{ActivityType, ActivityTypesConfig, PlayerConfig},
        ConfigManager,
    },
    cover::CoverManager,
    error::DiscordError,
    metadata,
    player::PlaybackState,
    template::TemplateManager,
    utils,
};

pub struct Presence {
    player: Player,
    template_manager: Arc<TemplateManager>,
    cover_manager: Arc<CoverManager>,
    last_player_state: Option<PlaybackState>,
    discord_client: Option<Arc<Mutex<DiscordIpcClient>>>,
    needs_initial_connection: AtomicBool,
    needs_reconnection: AtomicBool,
    error_logged: AtomicBool,
    last_reconnect_attempt: Mutex<Instant>,
    config: Arc<ConfigManager>,
}

impl Presence {
    pub fn new(
        player: Player,
        template_manager: Arc<TemplateManager>,
        cover_manager: Arc<CoverManager>,
        config: Arc<ConfigManager>,
    ) -> Self {
        let player_config = config.get_player_config(player.identity());
        info!("Initializing presence for player: {}", player.identity());
        trace!("Using Discord application ID: {}", player_config.app_id);
        trace!("Player configuration: {:#?}", player_config);
        Self {
            player,
            template_manager,
            cover_manager,
            last_player_state: None,
            discord_client: None,
            needs_initial_connection: AtomicBool::new(true),
            needs_reconnection: AtomicBool::new(false),
            error_logged: AtomicBool::new(false),
            last_reconnect_attempt: Mutex::new(Instant::now()),
            config,
        }
    }

    pub fn initialize_discord_client(&mut self) -> Result<(), DiscordError> {
        if self.discord_client.is_none() {
            let player_config = self.config.get_player_config(self.player.identity());
            let client = DiscordIpcClient::new(&player_config.app_id).unwrap();
            self.discord_client = Some(Arc::new(Mutex::new(client)));
            self.needs_initial_connection.store(true, Ordering::Relaxed);
        }
        Ok(())
    }

    pub fn destroy_discord_client(&mut self) -> Result<(), DiscordError> {
        if let Some(client) = &self.discord_client {
            debug!(
                "Closing Discord connection for player: {}",
                self.player.identity()
            );
            client.lock().close().map_err(|err| {
                error!("Failed to close Discord connection: {}", err);
                DiscordError::CloseError(err.to_string())
            })?;
            trace!("Discord connection closed successfully");
            self.discord_client = None;
        }
        Ok(())
    }

    pub async fn update(&mut self, player: Player) -> Result<(), DiscordError> {
        trace!("Updating presence for player: {}", player.identity());
        self.validate_player(&player)?;

        let Some(_discord_client) = &self.discord_client else {
            return Ok(());
        };

        self.ensure_connection()?;

        let start_time = Instant::now();
        let new_state = PlaybackState::from(&player);
        let dbus_delay = start_time.elapsed();
        trace!("D-Bus interaction took: {:?}", dbus_delay);

        let should_update = self
            .last_player_state
            .as_ref()
            .map(|previous_state| {
                new_state.has_significant_changes(previous_state)
                    || new_state.has_position_jump(
                        previous_state,
                        Duration::from_millis(self.config.interval()),
                        dbus_delay,
                    )
            })
            .unwrap_or(true);

        if !should_update {
            trace!("Skipping update - no significant changes detected");
            self.last_player_state = Some(new_state);
            return Ok(());
        }

        trace!("Updating Discord presence");
        self.last_player_state = Some(new_state);
        self.update_activity(player).await.map_err(|err| {
            if matches!(err, DiscordError::ActivityError(_)) {
                if !self.error_logged.load(Ordering::Relaxed) {
                    warn!("Discord connection error, will attempt to reconnect next update");
                    self.error_logged.store(true, Ordering::Relaxed);
                }
                self.last_player_state = None;
                self.needs_reconnection.store(true, Ordering::Relaxed);
            }
            err
        })
    }

    fn validate_player(&self, player: &Player) -> Result<(), DiscordError> {
        if player.identity() != self.player.identity()
            || player.bus_name() != self.player.bus_name()
            || player.unique_name() != self.player.unique_name()
        {
            error!(
                "Player validation failed - identity mismatch. Expected: {}, got: {}",
                self.player.identity(),
                player.identity()
            );
            return Err(DiscordError::InvalidPlayer(format!(
                "Expected {}, got {}",
                self.player.identity(),
                player.identity()
            )));
        }
        trace!("Player validation successful");
        Ok(())
    }

    fn ensure_connection(&mut self) -> Result<(), DiscordError> {
        const MIN_RECONNECT_INTERVAL: Duration = Duration::from_secs(10);

        let Some(discord_client) = &self.discord_client else {
            return Ok(());
        };

        if self.needs_initial_connection.load(Ordering::Relaxed) {
            debug!("Establishing initial Discord connection");
            discord_client.lock().connect().map_err(|err| {
                if !self.error_logged.load(Ordering::Relaxed) {
                    error!("Failed to establish Discord connection: {}", err);
                    self.error_logged.store(true, Ordering::Relaxed);
                }
                *self.last_reconnect_attempt.lock() = Instant::now();
                DiscordError::ConnectionError(err.to_string())
            })?;
            debug!("Discord connection established successfully");
            self.needs_initial_connection
                .store(false, Ordering::Relaxed);
            self.error_logged.store(false, Ordering::Relaxed);
        }

        if self.needs_reconnection.load(Ordering::Relaxed) {
            let now = Instant::now();
            let last_attempt = *self.last_reconnect_attempt.lock();

            if now.duration_since(last_attempt) < MIN_RECONNECT_INTERVAL {
                return Ok(());
            }

            if !self.error_logged.load(Ordering::Relaxed) {
                debug!("Attempting to reconnect to Discord");
            }

            *self.last_reconnect_attempt.lock() = now;

            discord_client.lock().reconnect().map_err(|err| {
                if !self.error_logged.load(Ordering::Relaxed) {
                    error!("Failed to reconnect to Discord: {}", err);
                    self.error_logged.store(true, Ordering::Relaxed);
                }
                DiscordError::ReconnectionError(err.to_string())
            })?;
            debug!("Discord reconnection successful");
            self.needs_reconnection.store(false, Ordering::Relaxed);
            self.error_logged.store(false, Ordering::Relaxed);
            self.last_player_state = None;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn force_reconnect(&mut self) {
        debug!(
            "Forcing Discord reconnection for player: {}",
            self.player.identity()
        );
        self.needs_reconnection.store(true, Ordering::Relaxed);
        self.last_player_state = None;
    }

    fn determine_activity_type(
        &self,
        activity_type_config: &ActivityTypesConfig,
        player_config: &PlayerConfig,
        url: Option<&str>,
    ) -> ActivityType {
        trace!(
            "Determining activity type for player: {}",
            self.player.identity()
        );

        if let Some(override_type) = player_config.override_activity_type {
            debug!("Using overridden activity type: {:?}", override_type);
            return override_type;
        }

        if activity_type_config.use_content_type && url.is_some() {
            trace!("Attempting to determine activity type from content type");
            if let Some(content_type) = url.and_then(utils::get_content_type_from_metadata) {
                match content_type.type_() {
                    mime::AUDIO => {
                        debug!("Content type is audio, using Listening activity type");
                        return ActivityType::Listening;
                    }
                    mime::VIDEO | mime::IMAGE => {
                        debug!("Content type is video/image, using Watching activity type");
                        return ActivityType::Watching;
                    }
                    _ => {
                        trace!("Unrecognized content type, falling back to default");
                    }
                }
            }
        }

        debug!(
            "Using default activity type: {:?}",
            activity_type_config.default
        );
        activity_type_config.default
    }

    async fn update_activity(&self, player: Player) -> Result<(), DiscordError> {
        let Some(discord_client) = &self.discord_client else {
            return Ok(());
        };

        let playback_status = player.get_playback_status().map_err(|err| {
            error!("Failed to get playback status: {}", err);
            DiscordError::ActivityError(format!("Failed to get playback status: {}", err))
        })?;

        if playback_status == PlaybackStatus::Stopped
            || (self.config.clear_on_pause() && playback_status == PlaybackStatus::Paused)
        {
            if !self.error_logged.load(Ordering::Relaxed) {
                info!(
                    "Clearing Discord activity - player {} is {}",
                    player.identity(),
                    if playback_status == PlaybackStatus::Stopped {
                        "stopped"
                    } else {
                        "paused"
                    }
                );
            }
            discord_client.lock().clear_activity().map_err(|err| {
                if !self.error_logged.load(Ordering::Relaxed) {
                    error!("Failed to clear Discord activity: {}", err);
                    self.error_logged.store(true, Ordering::Relaxed);
                }
                DiscordError::ActivityError(err.to_string())
            })?;
            return Ok(());
        }

        trace!(
            "Building Discord activity for player: {}",
            player.identity()
        );
        let metadata = match player.get_metadata() {
            Ok(metadata) => metadata,
            Err(e) => {
                warn!("Failed to get metadata for player: {}", e);
                return Ok(());
            }
        };
        trace!("Metadata: {:?}", metadata);

        let metadata_source = metadata::MetadataSource::from_mpris(metadata.clone());

        debug!("--- Raw Metadata Start ---");
        if let Some(mpris_meta) = metadata_source.mpris_metadata() {
            debug!("MPRIS Metadata Map:");
            for (key, value) in mpris_meta.iter() {
                debug!("  MPRIS Key: '{}', Value: {:?}", key, value);
            }
        } else {
            debug!("No MPRIS Metadata available in source.");
        }
        if let Some(lofty_tag) = metadata_source.lofty_tag() {
            debug!("Lofty Primary Tag ({:?}):", lofty_tag.file_type());
            if let Some(tag) = lofty_tag.primary_tag() {
                for item in tag.items() {
                    debug!("  Lofty Key: {:?}, Value: {:?}", item.key(), item.value());
                }
            } else {
                debug!("  No primary tag found by Lofty.");
            }
            debug!("Lofty Properties: {:?}", lofty_tag.properties());
        } else {
            debug!("No Lofty TaggedFile available in source (likely not a local file or read failed).");
        }
        debug!("--- Raw Metadata End ---");

        let media_metadata = metadata_source.to_media_metadata();

        let player_config = self.config.get_player_config(player.identity());
        let as_elapsed = self.config.time_config().as_elapsed;

        let (start_s, end_s) = if playback_status == PlaybackStatus::Playing {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Time went backwards");

            let position = player.get_position().unwrap_or_default();
            trace!("Player position: {:?}", position);
            let start_dur = now.checked_sub(position).unwrap_or_default();
            trace!("Start duration: {:?}", start_dur);
            let start_s = Some(start_dur.as_secs());
            trace!("Start seconds: {:?}", start_s);

            let length = metadata.length().unwrap_or_default();
            trace!("Length: {:?}", length);
            let end_s = if !as_elapsed && !length.is_zero() {
                start_dur.checked_add(length).map(|end| {
                    trace!("End duration: {:?}", end); // Fix: Log the end duration
                    end.as_secs()
                })
            } else {
                None
            };
            trace!("End seconds: {:?}", end_s);

            (start_s, end_s)
        } else {
            (None, None)
        };

        let activity_texts = self
            .template_manager
            .render_activity_texts(player, media_metadata)?;

        let cover_art_url = if let Some(art_source) = metadata_source.art_source() {
            match self
                .cover_manager
                .get_cover_art(art_source, &metadata_source)
                .await
            {
                Ok(Some(url)) => {
                    debug!("Found cover art URL for Discord presence");
                    trace!("Cover art URL: {}", url);
                    Some(url)
                }
                Ok(None) => {
                    debug!("No cover art available for Discord presence");
                    None
                }
                Err(e) => {
                    warn!("Failed to retrieve cover art: {}", e);
                    trace!("Cover art must be accessible via HTTP/HTTPS for Discord");
                    None
                }
            }
        } else {
            debug!("No art source found in metadata");
            None
        };

        let activity_type = self.determine_activity_type(
            &self.config.activity_type_config(),
            &player_config,
            metadata_source.url().as_deref(),
        );

        let mut activity = Activity::default().activity_type(activity_type.into());

        if !activity_texts.details.is_empty() {
            trace!("Setting activity details: {}", activity_texts.details);
            activity = activity.details(&activity_texts.details);
        }

        if !activity_texts.state.is_empty() {
            trace!("Setting activity state: {}", activity_texts.state);
            activity = activity.state(&activity_texts.state);
        }

        if let Some(start) = start_s {
            activity = activity.timestamps({
                let ts = Timestamps::default().start(start as i64);
                if let Some(end) = end_s {
                    trace!("Setting activity timestamps: start={}, end={}", start, end);
                    ts.end(end as i64)
                } else {
                    trace!("Setting activity timestamps: start={}", start);
                    ts
                }
            });
        }

        let mut assets = Assets::default();

        if let Some(img_url) = &cover_art_url {
            trace!("Setting Discord large image asset (cover art): {}", img_url);
            assets = assets.large_image(img_url);
            if !activity_texts.large_text.is_empty() {
                trace!("Setting Discord large text: {}", activity_texts.large_text);
                assets = assets.large_text(&activity_texts.large_text);
            }

            if player_config.show_icon {
                trace!(
                    "Setting Discord small image asset (player icon): {}",
                    player_config.icon
                );
                assets = assets.small_image(player_config.icon.as_str());
                if !activity_texts.small_text.is_empty() {
                    trace!("Setting Discord small text: {}", activity_texts.small_text);
                    assets = assets.small_text(&activity_texts.small_text);
                }
            }
        } else {
            trace!(
                "No cover art found, using player icon as large image: {}",
                player_config.icon
            );
            assets = assets.large_image(player_config.icon.as_str());
            if !activity_texts.large_text.is_empty() {
                trace!("Setting Discord large text: {}", activity_texts.large_text);
                assets = assets.large_text(&activity_texts.large_text);
            }
        }

        activity = activity.assets(assets);

        if !self.error_logged.load(Ordering::Relaxed) {
            debug!("Updating Discord activity");
        }
        discord_client
            .lock()
            .set_activity(activity)
            .map_err(|err| {
                if !self.error_logged.load(Ordering::Relaxed) {
                    error!("Failed to set Discord activity: {}", err);
                    self.error_logged.store(true, Ordering::Relaxed);
                }
                DiscordError::ActivityError(err.to_string())
            })?;
        if !self.error_logged.load(Ordering::Relaxed) {
            info!(
                "Updated Discord activity for {} - {} ({:?})",
                self.player.identity(),
                activity_texts.details,
                playback_status
            );
        }
        self.error_logged.store(false, Ordering::Relaxed);

        Ok(())
    }

    pub fn update_managers(
        &mut self,
        template_manager: Arc<TemplateManager>,
        cover_manager: Arc<CoverManager>,
        config: Arc<ConfigManager>,
    ) {
        trace!(
            "Updating presence managers for player: {}",
            self.player.identity()
        );
        self.template_manager = template_manager;
        self.cover_manager = cover_manager;
        self.config = config;
        trace!("Presence managers updated successfully");

        self.last_player_state = None;
    }
}
