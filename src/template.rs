use log::{debug, info, trace, warn, error};
use std::sync::Arc;

use handlebars::{handlebars_helper, Handlebars};
use mpris::{PlaybackStatus, Player};

use crate::{
    config::ConfigManager,
    error::TemplateError,
    utils::format_duration,
};
use std::collections::BTreeMap;

pub struct TemplateManager {
    handlebars: Handlebars<'static>,
}

#[derive(Debug)]
pub struct ActivityTexts {
    pub details: String,
    pub state: String,
    pub large_text: String,
    pub small_text: String,
}

handlebars_helper!(eq: |x: str, y: str| x == y);

impl TemplateManager {
    pub fn new(config: &Arc<ConfigManager>) -> Result<Self, TemplateError> {
        info!("Initializing template manager");
        let mut handlebars = Handlebars::new();
        let template_config = config.template_config();

        trace!("Registering custom template helpers");
        handlebars.register_helper("eq", Box::new(eq));

        debug!("Registering template strings");
        // Register all templates
        handlebars
            .register_template_string("detail", &template_config.detail)
            .map_err(|e| {
                error!("Failed to register 'detail' template: {}", e);
                e
            })?;
        handlebars
            .register_template_string("state", &template_config.state)
            .map_err(|e| {
                error!("Failed to register 'state' template: {}", e);
                e
            })?;
        handlebars
            .register_template_string("large_text", &template_config.large_text)
            .map_err(|e| {
                error!("Failed to register 'large_text' template: {}", e);
                e
            })?;
        handlebars
            .register_template_string("small_text", &template_config.small_text)
            .map_err(|e| {
                error!("Failed to register 'small_text' template: {}", e);
                e
            })?;

        debug!("Template manager initialization completed successfully");
        Ok(Self { handlebars })
    }

    pub fn render(
        &self,
        template_name: &str,
        data: &BTreeMap<String, String>,
    ) -> Result<String, TemplateError> {
        trace!("Rendering template: {}", template_name);
        self.handlebars.render(template_name, data).map_err(|e| {
            error!("Failed to render template '{}': {}", template_name, e);
            e.into()
        })
    }

    /// Create a complete Activity object from player state and metadata
    pub fn render_activity_texts(&self, player: Player) -> Result<ActivityTexts, TemplateError> {
        trace!("Creating activity texts for player: {}", player.identity());
        
        // Create template data with metadata fields
        debug!("Gathering player metadata and state for templates");
        let template_data = Self::create_data(player);

        // Render templates with full metadata
        trace!("Rendering all activity text templates");
        let details = self.render("detail", &template_data)?;
        let state_text = self.render("state", &template_data)?;
        let large_text = self.render("large_text", &template_data)?;
        let small_text = self.render("small_text", &template_data)?;

        trace!("Activity text rendering completed successfully");
        Ok(ActivityTexts {
            details,
            state: state_text,
            large_text,
            small_text,
        })
    }

    pub fn create_data(player: Player) -> BTreeMap<String, String> {
        trace!("Creating template data for player: {}", player.identity());
        let mut data = BTreeMap::new();

        // Player information
        trace!("Adding player identity information");
        data.insert("player".to_string(), player.identity().to_string());
        data.insert(
            "player_bus_name".to_string(),
            player.bus_name_player_name_part().to_string(),
        );

        // Playback information
        if let Ok(status) = player.get_playback_status() {
            trace!("Adding playback status: {:?}", status);
            data.insert("status".to_string(), format!("{:?}", status));

            let status_icon = match status {
                PlaybackStatus::Playing => "▶",
                PlaybackStatus::Paused => "⏸️",
                PlaybackStatus::Stopped => "⏹️",
            };
            data.insert("status_icon".to_string(), status_icon.to_string());
        } else {
            warn!("Failed to get playback status for player: {}", player.identity());
        }

        if let Ok(volume) = player.get_volume() {
            trace!("Adding volume information: {}", volume);
            data.insert("volume".to_string(), volume.to_string());
        } else {
            debug!("Volume information not available for player: {}", player.identity());
        }

        trace!("Processing player metadata");
        if let Ok(metadata) = player.get_metadata() {
            if let Some(title) = metadata.title() {
                trace!("Adding title: {}", title);
                data.insert("title".to_string(), title.to_string());
            }

            if let Some(artists) = metadata.artists() {
                trace!("Adding artists: {}", artists.join(", "));
                data.insert("artists".to_string(), artists.join(", "));
            }

            if let Some(url) = metadata.url().as_ref().map(|s| s.to_string()) {
                trace!("Adding media URL");
                data.insert("url".to_string(), url);
            }

            if let Some(length) = metadata.length() {
                trace!("Adding media length: {}s", length.as_secs());
                data.insert("length".to_string(), format_duration(length.as_secs()));
            }

            if let Some(track_number) = metadata.track_number() {
                trace!("Adding track number: {}", track_number);
                data.insert("track_number".to_string(), track_number.to_string());
            }

            if let Some(disc_number) = metadata.disc_number() {
                trace!("Adding disc number: {}", disc_number);
                data.insert("disc_number".to_string(), disc_number.to_string());
            }

            if let Some(album_name) = metadata.album_name() {
                trace!("Adding album name: {}", album_name);
                data.insert("album".to_string(), album_name.to_string());
            }

            if let Some(album_artists) = metadata.album_artists() {
                trace!("Adding album artists: {}", album_artists.join(", "));
                data.insert("album_artists".to_string(), album_artists.join(", "));
            }
        } else {
            warn!("Failed to get metadata for player: {}", player.identity());
        }

        debug!("Template data creation completed with {} fields", data.len());
        data
    }
}
