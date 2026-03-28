use log::{debug, error, info, trace};
use std::sync::Arc;

use handlebars::{handlebars_helper, no_escape, Handlebars};
use handlebars_misc_helpers::regex_helpers;
use mpris::Player;
use serde::Serialize;

use crate::{
    config::ConfigManager, error::TemplateError, metadata::MediaMetadata,
    player::canonical_player_bus_name, utils::format_playback_status_icon,
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
        let status = player
            .get_playback_status()
            .map(|s| format!("{:?}", s))
            .ok();

        let status_icon = player
            .get_playback_status()
            .map(format_playback_status_icon)
            .map(String::from)
            .ok();

        Self {
            player: player.identity().to_string(),
            player_bus_name: canonical_player_bus_name(player.bus_name()),
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
handlebars_helper!(contains: |haystack: str, needle: str| haystack.contains(needle));
handlebars_helper!(icontains: |haystack: str, needle: str| haystack.to_lowercase().contains(&needle.to_lowercase()));

fn register_template_helpers(handlebars: &mut Handlebars<'static>) {
    handlebars.register_helper("eq", Box::new(eq));
    handlebars.register_helper("contains", Box::new(contains));
    handlebars.register_helper("icontains", Box::new(icontains));
    regex_helpers::register(handlebars);
}

impl TemplateManager {
    pub fn new(config: &Arc<ConfigManager>) -> Result<Self, TemplateError> {
        info!("Initializing template manager");
        let mut handlebars = Handlebars::new();
        handlebars.register_escape_fn(no_escape);
        let template_config = config.template_config();

        trace!("Registering custom template helpers");
        register_template_helpers(&mut handlebars);

        handlebars
            .register_template_string("details", &template_config.details)
            .map_err(|e| {
                error!("Failed to register 'details' template: {}", e);
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

    /// Create a TemplateManager with specified templates for testing
    #[allow(dead_code)]
    pub fn new_raw(
        details: &str,
        state: &str,
        large_text: &str,
        small_text: &str,
    ) -> Result<Self, TemplateError> {
        let mut handlebars = Handlebars::new();
        handlebars.register_escape_fn(no_escape);

        register_template_helpers(&mut handlebars);

        handlebars.register_template_string("details", details)?;
        handlebars.register_template_string("state", state)?;
        handlebars.register_template_string("large_text", large_text)?;
        handlebars.register_template_string("small_text", small_text)?;

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

    pub fn render_activity_texts(
        &self,
        player: Player,
        metadata: MediaMetadata,
    ) -> Result<ActivityTexts, TemplateError> {
        trace!("Creating activity texts for player: {}", player.identity());

        debug!("Creating render context with player and metadata information");
        let render_context = RenderContext::new(&player, metadata);

        trace!("Rendering all activity text templates");
        let details = self.render("details", &render_context)?;
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

#[cfg(test)]
mod tests {
    use super::{RenderContext, TemplateManager};
    use crate::{error::TemplateError, metadata::MediaMetadata};

    fn test_context() -> RenderContext {
        RenderContext {
            player: "Spotify Desktop".into(),
            player_bus_name: "spotify".into(),
            status: Some("Playing".into()),
            status_icon: Some(">".into()),
            volume: Some(0.5),
            metadata: MediaMetadata {
                title: Some("Song Title".into()),
                artist_display: Some("Artist Name".into()),
                ..MediaMetadata::default()
            },
        }
    }

    #[test]
    fn renders_contains_helper() {
        let manager = TemplateManager::new_raw(
            "{{#if (contains player \"Spotify\")}}match{{else}}no{{/if}}",
            "",
            "",
            "",
        )
        .expect("template manager should initialize");

        let rendered = manager
            .render("details", &test_context())
            .expect("contains helper should render");

        assert_eq!(rendered, "match");
    }

    #[test]
    fn renders_icontains_helper() {
        let manager = TemplateManager::new_raw(
            "{{#if (icontains player \"spotify\")}}match{{else}}no{{/if}}",
            "",
            "",
            "",
        )
        .expect("template manager should initialize");

        let rendered = manager
            .render("details", &test_context())
            .expect("icontains helper should render");

        assert_eq!(rendered, "match");
    }

    #[test]
    fn renders_regex_is_match_helper() {
        let manager = TemplateManager::new_raw(
            "{{#if (regex_is_match pattern=\"^Spot.*\" on=player)}}match{{else}}no{{/if}}",
            "",
            "",
            "",
        )
        .expect("template manager should initialize");

        let rendered = manager
            .render("details", &test_context())
            .expect("regex helper should render");

        assert_eq!(rendered, "match");
    }

    #[test]
    fn renders_regex_captures_helper() {
        let manager = TemplateManager::new_raw(
            "{{#with (regex_captures pattern=\"^(?<name>.+) Desktop$\" on=player)}}{{name}}{{/with}}",
            "",
            "",
            "",
        )
        .expect("template manager should initialize");

        let rendered = manager
            .render("details", &test_context())
            .expect("regex captures helper should render");

        assert_eq!(rendered, "Spotify");
    }

    #[test]
    fn invalid_regex_returns_template_error() {
        let manager =
            TemplateManager::new_raw("{{regex_is_match pattern=\"(\" on=player}}", "", "", "")
                .expect("template manager should initialize");

        let err = manager
            .render("details", &test_context())
            .expect_err("invalid regex should fail");

        match err {
            TemplateError::HandlebarsRender(render_err) => {
                assert!(render_err.to_string().contains("regex parse error"));
            }
            other => panic!("unexpected template error: {other:?}"),
        }
    }
}
