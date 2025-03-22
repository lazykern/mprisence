use log::{debug, info};
use std::sync::Arc;

use handlebars::Handlebars;
use mpris::PlaybackStatus;

use crate::{config::{self, get_config, ConfigManager}, error::TemplateError, player::{PlayerId, PlayerState}, utils::format_duration};
use std::collections::BTreeMap;

pub struct TemplateManager {
    handlebars: Handlebars<'static>,
}

impl TemplateManager {
    pub fn new(config: &Arc<ConfigManager>) -> Result<Self, TemplateError> {
        info!("Initializing TemplateManager");
        let mut handlebars = Handlebars::new();
        let template_config = config.template_config();

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
        data.insert("volume".to_string(), state.volume.to_string());

        data.insert(
            "position".to_string(),
            format_duration(state.position as u64),
        );

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
