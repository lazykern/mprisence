use std::fmt::Display;

use crate::config::schema::{
    ActivityType, PlayerConfig, PlayerConfigLayer, StatusDisplayType, WebPlayerConfig,
    WebPlayerConfigLayer,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueSource {
    Explicit,
    DefaultLayer,
    Bundled,
    Global,
}

pub fn layer_source<T>(draft: &Option<T>) -> ValueSource {
    if draft.is_some() {
        ValueSource::Explicit
    } else {
        ValueSource::DefaultLayer
    }
}

pub fn web_option_source(draft: &Option<impl Sized>, effective: &Option<impl Sized>) -> ValueSource {
    if draft.is_some() {
        ValueSource::Explicit
    } else if effective.is_some() {
        ValueSource::DefaultLayer
    } else {
        ValueSource::Bundled
    }
}

pub fn format_summary(field: &str, display: impl Display, source: ValueSource) -> String {
    format!("  {field}: {display}{}", source_suffix(source))
}

pub fn format_menu(label: &str, display: impl Display, source: ValueSource) -> String {
    format!("{label} ({display}{})", menu_source_suffix(source))
}

pub fn format_menu_bool(label: &str, draft: Option<bool>, effective: bool) -> String {
    if let Some(value) = draft {
        format_menu(label, value, ValueSource::Explicit)
    } else {
        format_menu(label, effective, ValueSource::DefaultLayer)
    }
}

pub fn format_menu_bool_web(
    label: &str,
    draft: Option<bool>,
    effective: Option<bool>,
    runtime: bool,
) -> String {
    let source = web_option_source(&draft, &effective);
    let value = draft.or(effective).unwrap_or(runtime);
    format_menu(label, value, source)
}

pub struct ActivityDisplay {
    pub label: &'static str,
    pub source: ValueSource,
}

pub fn activity_display(
    draft: Option<ActivityType>,
    effective: Option<ActivityType>,
    global: ActivityType,
) -> ActivityDisplay {
    if let Some(value) = draft {
        ActivityDisplay {
            label: activity_type_str(value),
            source: ValueSource::Explicit,
        }
    } else if let Some(value) = effective {
        ActivityDisplay {
            label: activity_type_str(value),
            source: ValueSource::DefaultLayer,
        }
    } else {
        ActivityDisplay {
            label: activity_type_str(global),
            source: ValueSource::Global,
        }
    }
}

pub fn player_runtime(draft: &PlayerConfigLayer, effective: &PlayerConfig) -> PlayerConfig {
    draft.apply_over(effective.clone())
}

pub fn web_merged(draft: &WebPlayerConfigLayer, effective: &WebPlayerConfig) -> WebPlayerConfig {
    let mut base = effective.clone();
    let patterns = draft.effective_patterns();
    if !patterns.is_empty() {
        base.match_patterns = patterns.into_iter().map(str::to_string).collect();
    }
    if let Some(value) = &draft.title_suffix {
        base.title_suffix = Some(value.clone());
    }
    if let Some(value) = &draft.name {
        base.name = Some(value.clone());
    }
    if let Some(value) = draft.ignore {
        base.ignore = value;
    }
    if let Some(value) = &draft.app_id {
        base.app_id = Some(value.clone());
    }
    if let Some(value) = &draft.icon {
        base.icon = Some(value.clone());
    }
    if let Some(value) = draft.show_icon {
        base.show_icon = Some(value);
    }
    if let Some(value) = draft.allow_streaming {
        base.allow_streaming = Some(value);
    }
    if let Some(value) = draft.status_display_type {
        base.status_display_type = Some(value);
    }
    if let Some(value) = draft.override_activity_type {
        base.override_activity_type = Some(value);
    }
    base
}

pub fn web_runtime(draft: &WebPlayerConfigLayer, effective: &WebPlayerConfig) -> PlayerConfig {
    web_merged(draft, effective).into_player_config()
}

pub fn resolved_web_status(
    draft: &WebPlayerConfigLayer,
    effective: &WebPlayerConfig,
) -> (StatusDisplayType, ValueSource) {
    let source = web_option_source(&draft.status_display_type, &effective.status_display_type);
    let value = draft
        .status_display_type
        .or(effective.status_display_type)
        .unwrap_or_else(|| PlayerConfig::default().status_display_type);
    (value, source)
}

pub fn status_display_type_str(value: StatusDisplayType) -> &'static str {
    match value {
        StatusDisplayType::Name => "name",
        StatusDisplayType::State => "state",
        StatusDisplayType::Details => "details",
    }
}

pub fn activity_type_str(value: ActivityType) -> &'static str {
    match value {
        ActivityType::Listening => "listening",
        ActivityType::Watching => "watching",
        ActivityType::Playing => "playing",
        ActivityType::Competing => "competing",
    }
}

pub fn parse_status_display_type(value: &str) -> StatusDisplayType {
    match value.trim_start_matches('→').trim() {
        "state" => StatusDisplayType::State,
        "details" => StatusDisplayType::Details,
        _ => StatusDisplayType::Name,
    }
}

pub fn parse_activity_type(value: &str) -> ActivityType {
    match value.trim_start_matches('→').trim() {
        "watching" => ActivityType::Watching,
        "playing" => ActivityType::Playing,
        "competing" => ActivityType::Competing,
        _ => ActivityType::Listening,
    }
}

fn source_suffix(source: ValueSource) -> &'static str {
    match source {
        ValueSource::Explicit => "",
        ValueSource::DefaultLayer => " (default)",
        ValueSource::Bundled => " (bundled)",
        ValueSource::Global => " (global)",
    }
}

fn menu_source_suffix(source: ValueSource) -> &'static str {
    match source {
        ValueSource::Explicit => "",
        ValueSource::DefaultLayer => ", default",
        ValueSource::Bundled => ", bundled",
        ValueSource::Global => ", global",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_summary_explicit_has_no_suffix() {
        assert_eq!(format_summary("show_icon", false, ValueSource::Explicit), "  show_icon: false");
    }

    #[test]
    fn format_summary_default_layer_suffix() {
        assert_eq!(
            format_summary("show_icon", false, ValueSource::DefaultLayer),
            "  show_icon: false (default)"
        );
    }

    #[test]
    fn format_menu_inherited_bool() {
        assert_eq!(
            format_menu_bool("Show icon", None, false),
            "Show icon (false, default)"
        );
    }

    #[test]
    fn format_menu_explicit_bool() {
        assert_eq!(
            format_menu_bool("Show icon", Some(true), false),
            "Show icon (true)"
        );
    }

    #[test]
    fn web_runtime_fills_show_icon_from_bundled_defaults() {
        let draft = WebPlayerConfigLayer::default();
        let effective = WebPlayerConfig::default();
        let runtime = web_runtime(&draft, &effective);
        assert_eq!(runtime.show_icon, PlayerConfig::default().show_icon);
        assert!(runtime.allow_streaming);
    }

    #[test]
    fn activity_display_global_when_unset() {
        let display = activity_display(None, None, ActivityType::Watching);
        assert_eq!(display.label, "watching");
        assert_eq!(display.source, ValueSource::Global);
    }

    #[test]
    fn activity_display_default_layer_from_effective() {
        let display = activity_display(None, Some(ActivityType::Playing), ActivityType::Listening);
        assert_eq!(display.label, "playing");
        assert_eq!(display.source, ValueSource::DefaultLayer);
    }

    #[test]
    fn web_option_source_bundled_when_both_none() {
        assert_eq!(
            web_option_source(&None::<bool>, &None::<bool>),
            ValueSource::Bundled
        );
    }
}
