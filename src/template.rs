use log::{debug, info, trace, error};
use std::sync::Arc;

use handlebars::{handlebars_helper, Handlebars};
use mpris::Player;
use serde::Serialize;

use crate::{
    config::ConfigManager,
    error::TemplateError,
    metadata::MediaMetadata,
    utils::format_playback_status_icon,
};

/// A struct containing all variables available for template rendering,
/// including player state and media metadata.
#[derive(Debug, Clone, Serialize)]
pub struct RenderContext {
    pub player: String,
    pub player_bus_name: String,
    pub status: Option<String>,
    pub status_icon: Option<String>,
    pub volume: Option<f64>,

    #[serde(flatten)]
    pub metadata: MediaMetadata,
}

impl RenderContext {
    pub fn new(player: &Player, metadata: MediaMetadata) -> Self {
        let status = player.get_playback_status()
            .map(|s| format!("{:?}", s)).ok();

        let status_icon = player.get_playback_status()
            .map(format_playback_status_icon)
            .map(String::from)
            .ok();

        Self {
            player: player.identity().to_string(),
            player_bus_name: player.bus_name_player_name_part().to_string(),
            status,
            status_icon,
            volume: player.get_volume().ok(),
            metadata,
        }
    }
}

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
        context: &RenderContext,
    ) -> Result<String, TemplateError> {
        trace!("Rendering template: {}", template_name);
        self.handlebars.render(template_name, context).map_err(|e| {
            error!("Failed to render template '{}': {}", template_name, e);
            e.into()
        })
    }

    pub fn render_activity_texts(&self, player: Player, metadata: MediaMetadata) -> Result<ActivityTexts, TemplateError> {
        trace!("Creating activity texts for player: {}", player.identity());
        
        debug!("Creating render context with player and metadata information");
        let render_context = RenderContext::new(&player, metadata);

        trace!("Rendering all activity text templates");
        let details = self.render("detail", &render_context)?;
        let state_text = self.render("state", &render_context)?;
        let large_text = self.render("large_text", &render_context)?;
        let small_text = self.render("small_text", &render_context)?;

        trace!("Activity text rendering completed successfully");
        Ok(ActivityTexts {
            details,
            state: state_text,
            large_text,
            small_text,
        })
    }
}
