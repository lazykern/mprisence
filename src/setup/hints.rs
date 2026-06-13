/// Hover-text help when editing display name (players).
pub fn player_name_help(configured: Option<&str>, identity_fallback: &str) -> String {
    let current = match configured {
        Some(name) => format!("Currently \"{name}\""),
        None => format!("Falls back to identity \"{identity_fallback}\""),
    };
    format!(
        "Small-icon hover text in Discord (template {{{{player}}}}). {current}. Leave blank to use identity."
    )
}

/// Hover-text help when editing display name (web sites).
pub fn web_name_help(configured: Option<&str>, key: &str) -> String {
    let current = match configured {
        Some(name) => format!("Currently \"{name}\""),
        None => format!("Falls back to config key \"{key}\""),
    };
    format!(
        "Small-icon hover text in Discord (template {{{{player}}}}). {current}. Leave blank to use config key."
    )
}

pub fn title_suffix_help(configured: Option<&str>) -> String {
    let current = configured
        .map(|s| format!("\"{s}\""))
        .unwrap_or_else(|| "none".to_string());
    format!(
        "When xesam:url missing, match title ending with this suffix (e.g. \" | YouTube Music\"). Currently: {current}."
    )
}

pub fn match_pattern_help(configured: &str) -> String {
    if configured.is_empty() {
        "URL host to match (e.g. music.youtube.com). Required for web sites.".to_string()
    } else {
        format!(
            "URL host to match (e.g. music.youtube.com). Currently: \"{configured}\"."
        )
    }
}

pub fn app_id_help(current: &str) -> String {
    format!("Discord application ID for this presence. Currently: {current}.")
}

pub fn app_id_optional_help(current: Option<&str>) -> String {
    let current = current
        .map(|id| format!("\"{id}\""))
        .unwrap_or_else(|| "inherit from [player.*]".to_string());
    format!("Discord application ID. Currently: {current}. Blank = inherit.")
}

pub fn icon_help(current: &str) -> String {
    format!(
        "Discord large image (track art) and fallback small image asset. Currently: {}.",
        truncate(current)
    )
}

pub fn icon_optional_help(current: Option<&str>) -> String {
    match current {
        Some(url) => format!(
            "Icon URL for Discord assets. Currently: {}. Blank = inherit.",
            truncate(url)
        ),
        None => "Icon URL for Discord assets. Currently: inherit from player config.".to_string(),
    }
}

pub fn show_icon_help(current: bool) -> String {
    format!(
        "Show player icon as Discord small image. Currently: {}.",
        yes_no(current)
    )
}

pub fn allow_streaming_help(current: bool) -> String {
    format!(
        "Allow web/streaming URLs for this player. Currently: {}.",
        yes_no(current)
    )
}

pub fn ignore_help(current: bool) -> String {
    format!(
        "Config key `ignore`. true = hide from Discord, false = active. Currently: ignore = {current}."
    )
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn truncate(value: &str) -> String {
    if value.chars().count() <= 56 {
        value.to_string()
    } else {
        format!("{}…", value.chars().take(53).collect::<String>())
    }
}
