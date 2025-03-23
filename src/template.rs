use log::{debug, info};
use std::sync::Arc;

use handlebars::{handlebars_helper, Handlebars};
use mpris::{Metadata, PlaybackStatus};

use crate::{config::{get_config, ConfigManager}, error::TemplateError, player::{PlayerId, PlayerState}, utils::{format_duration}};
use std::collections::BTreeMap;

pub struct TemplateManager {
    handlebars: Handlebars<'static>,
}

pub struct ActivityTexts {
    pub details: String,
    pub state: String,
    pub large_text: String,
    pub small_text: String,
}

handlebars_helper!(eq: |x: str, y: str| x == y);

impl TemplateManager {
    pub fn new(config: &Arc<ConfigManager>) -> Result<Self, TemplateError> {
        info!("Initializing TemplateManager");
        let mut handlebars = Handlebars::new();
        let template_config = config.template_config();

        // Register custom helpers
        handlebars.register_helper("eq", Box::new(eq));

        // Register all templates
        handlebars.register_template_string("detail", &template_config.detail)?;
        handlebars.register_template_string("state", &template_config.state)?;
        handlebars.register_template_string("large_text", &template_config.large_text)?;
        handlebars.register_template_string("small_text", &template_config.small_text)?;

        info!("Template registration successful");
        Ok(Self { handlebars })
    }

    pub fn reload(&mut self, config: &Arc<ConfigManager>) -> Result<(), TemplateError> {
        debug!("Reloading templates");
        let template_config = config.template_config();

        // Reregister all templates without recreating Handlebars instance
        self.handlebars
            .register_template_string("detail", &template_config.detail)?;
        self.handlebars
            .register_template_string("state", &template_config.state)?;
        self.handlebars
            .register_template_string("large_text", &template_config.large_text)?;
        self.handlebars
            .register_template_string("small_text", &template_config.small_text)?;

        Ok(())
    }

    pub fn render(
        &self,
        template_name: &str,
        data: &BTreeMap<String, String>,
    ) -> Result<String, TemplateError> {
        // Directly render without caching
        Ok(self.handlebars.render(template_name, data)?)
    }

    /// Create a complete Activity object from player state and metadata
    pub fn render_activity_texts(
        &self,
        player_id: &PlayerId,
        state: &PlayerState,
        metadata: &Metadata,
    ) -> Result<ActivityTexts, TemplateError> {
        // Create template data with metadata fields
        let template_data = Self::create_full_data(player_id, state, metadata);
        
        // Render templates with full metadata
        let details = self.render("detail", &template_data)?;
        let state_text = self.render("state", &template_data)?;
        let large_text = self.render("large_text", &template_data)?;
        let small_text = self.render("small_text", &template_data)?;
        
        Ok(ActivityTexts {
            details,
            state: state_text,
            large_text,
            small_text,
        })
    }

    /// Helper to create template data from player state and full metadata
    pub fn create_full_data(
        player_id: &PlayerId, 
        state: &PlayerState,
        metadata: &Metadata,
    ) -> BTreeMap<String, String> {
        let mut data = Self::create_data(player_id, state);
        
        // Add additional metadata fields
        if let Some(length) = metadata.length() {
            data.insert("length".to_string(), format_duration(length.as_secs()));
        }
        if let Some(track_number) = metadata.track_number() {
            data.insert("track_number".to_string(), track_number.to_string());
        }
        if let Some(disc_number) = metadata.disc_number() {
            data.insert("disc_number".to_string(), disc_number.to_string());
        }
        if let Some(album_name) = metadata.album_name() {
            data.insert("album".to_string(), album_name.to_string());
        }
        if let Some(album_artists) = metadata.album_artists() {
            data.insert("album_artists".to_string(), album_artists.join(", "));
        }
        
        data
    }

    /// Helper to create template data from player state
    pub fn create_data(
        player_id: &PlayerId,
        state: &PlayerState,
    ) -> BTreeMap<String, String> {
        let mut data = BTreeMap::new();
        let config = get_config();
        let template_config = config.template_config();

        // Helper function to handle unknown values
        let handle_unknown = |value: Option<String>| -> String {
            value.unwrap_or_else(|| template_config.unknown_text.to_string())
        };

        // Player information
        data.insert("player".to_string(), player_id.identity.to_string());
        data.insert(
            "player_bus_name".to_string(),
            player_id.player_bus_name.to_string(),
        );

        // Playback information - use static strings where possible
        data.insert("status".to_string(), format!("{:?}", state.playback_status));

        let status_icon = match state.playback_status {
            PlaybackStatus::Playing => "▶",
            PlaybackStatus::Paused => "⏸️",
            PlaybackStatus::Stopped => "⏹️",
        };
        data.insert("status_icon".to_string(), status_icon.to_string());

        if let Some(volume) = state.volume {
            data.insert("volume".to_string(), volume.to_string());
        }

        // Basic track metadata with unknown handling
        data.insert(
            "title".to_string(),
            handle_unknown(state.title.as_ref().map(|s| s.to_string())),
        );

        data.insert(
            "artists".to_string(),
            handle_unknown(state.artists.as_ref().map(|s| s.to_string())),
        );

        // URL
        if let Some(url) = state.url.as_ref().map(|s| s.to_string()) {
            data.insert("url".to_string(), url);
        }

        data
    }
}
