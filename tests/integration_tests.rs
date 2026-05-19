use handlebars::Handlebars;
use mprisence::{
    config::ConfigManager,
    metadata::MediaMetadata,
    template::{RenderContext, TemplateManager},
};
use std::collections::HashMap;
use std::sync::Arc;

#[test]
fn test_template_with_metadata() {
    let mut handlebars = Handlebars::new();

    handlebars
        .register_template_string("detail_template", "{{title}}")
        .unwrap();
    handlebars
        .register_template_string("state_template", "by {{artist_display}}")
        .unwrap();
    handlebars
        .register_template_string("large_text_template", "on {{album}}")
        .unwrap();
    handlebars
        .register_template_string("small_text_template", "Playing on {{player}}")
        .unwrap();

    let mut metadata = HashMap::new();
    metadata.insert("title".to_string(), "Test Song".to_string());
    metadata.insert("artist_display".to_string(), "Test Artist".to_string());
    metadata.insert("album".to_string(), "Test Album".to_string());
    metadata.insert("duration_display".to_string(), "03:45".to_string());
    metadata.insert("player".to_string(), "test_player".to_string());
    metadata.insert("status".to_string(), "Playing".to_string());

    let detail = handlebars.render("detail_template", &metadata).unwrap();
    let state = handlebars.render("state_template", &metadata).unwrap();
    let large_text = handlebars.render("large_text_template", &metadata).unwrap();
    let small_text = handlebars.render("small_text_template", &metadata).unwrap();

    assert_eq!(detail, "Test Song");
    assert_eq!(state, "by Test Artist");
    assert_eq!(large_text, "on Test Album");
    assert_eq!(small_text, "Playing on test_player");

    handlebars
        .register_template_string(
            "conditional_template",
            "{{#if status}}{{status}}{{else}}Stopped{{/if}}",
        )
        .unwrap();

    assert_eq!(
        handlebars
            .render("conditional_template", &metadata)
            .unwrap(),
        "Playing"
    );

    metadata.remove("status");
    assert_eq!(
        handlebars
            .render("conditional_template", &metadata)
            .unwrap(),
        "Stopped"
    );

    handlebars
        .register_template_string(
            "full_info",
            "{{title}} by {{artist_display}} from {{album}} ({{duration_display}})",
        )
        .unwrap();

    let full_info = handlebars.render("full_info", &metadata).unwrap();
    assert_eq!(
        full_info,
        "Test Song by Test Artist from Test Album (03:45)"
    );
}

#[test]
fn test_template_manager_with_config() {
    let config = ConfigManager::create_with_templates(
        "{{title}} by {{artist_display}}",
        "{{#if status}}{{status}}{{else}}Stopped{{/if}}",
        "from {{album}}",
        "Playing on {{player}}",
    );

    let config_arc = Arc::new(config);

    let template_manager =
        TemplateManager::new(&config_arc).expect("Failed to create template manager");

    let metadata = MediaMetadata {
        title: Some("Test Song".to_string()),
        artist_display: Some("Test Artist".to_string()),
        album: Some("Test Album".to_string()),
        duration_display: Some("3:45".to_string()),
        ..Default::default()
    };

    let render_context = RenderContext {
        player: "test_player".to_string(),
        player_bus_name: "org.mpris.MediaPlayer2.test".to_string(),
        status: Some("Playing".to_string()),
        status_icon: Some("▶".to_string()),
        volume: Some(1.0),
        metadata: metadata.clone(),
    };

    let details = template_manager
        .render("details", &render_context)
        .expect("Failed to render details");
    let state = template_manager
        .render("state", &render_context)
        .expect("Failed to render state");
    let large_text = template_manager
        .render("large_text", &render_context)
        .expect("Failed to render large text");
    let small_text = template_manager
        .render("small_text", &render_context)
        .expect("Failed to render small text");

    assert_eq!(details, "Test Song by Test Artist");
    assert_eq!(state, "Playing");
    assert_eq!(large_text, "from Test Album");
    assert_eq!(small_text, "Playing on test_player");

    let render_context_stopped = RenderContext {
        player: "test_player".to_string(),
        player_bus_name: "org.mpris.MediaPlayer2.test".to_string(),
        status: None,
        status_icon: None,
        volume: Some(1.0),
        metadata: metadata.clone(),
    };

    let state_stopped = template_manager
        .render("state", &render_context_stopped)
        .expect("Failed to render state when stopped");
    assert_eq!(
        state_stopped, "Stopped",
        "State should show 'Stopped' when status is None"
    );
}
