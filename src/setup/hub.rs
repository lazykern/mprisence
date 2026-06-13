use crate::config::schema::Config;
use crate::error::Error;
use crate::web_bridge;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HubTopic {
    Players,
    WebSites,
    Discovery,
    Display,
    CoverArt,
    Defaults,
    WebBridge,
    Exit,
}

pub struct HubItem {
    pub topic: HubTopic,
    pub line: String,
}

pub fn hub_items(config: &Config) -> Vec<HubItem> {
    vec![
        HubItem {
            topic: HubTopic::Players,
            line: format!("Players          {}", players_chip(config)),
        },
        HubItem {
            topic: HubTopic::WebSites,
            line: format!("Web sites        {}", web_chip(config)),
        },
        HubItem {
            topic: HubTopic::Discovery,
            line: format!("Discovery        {}", discovery_chip(config)),
        },
        HubItem {
            topic: HubTopic::Display,
            line: format!("Display          {}", display_chip(config)),
        },
        HubItem {
            topic: HubTopic::CoverArt,
            line: format!("Cover art        {}", cover_chip(config)),
        },
        HubItem {
            topic: HubTopic::Defaults,
            line: format!("Defaults         {}", defaults_chip(config)),
        },
        HubItem {
            topic: HubTopic::WebBridge,
            line: format!("Web bridge       {}", bridge_chip()),
        },
        HubItem {
            topic: HubTopic::Exit,
            line: "Exit".to_string(),
        },
    ]
}

pub fn run_topic(
    topic: HubTopic,
    _config: &Config,
    config_path: &std::path::Path,
) -> Result<bool, Error> {
    match topic {
        HubTopic::Exit => Ok(true),
        HubTopic::Players => {
            super::players::run(config_path)?;
            Ok(false)
        }
        HubTopic::WebSites => {
            super::web::run(config_path)?;
            Ok(false)
        }
        HubTopic::Discovery => {
            super::discovery::run(config_path)?;
            Ok(false)
        }
        HubTopic::Display => {
            super::display::run(config_path)?;
            Ok(false)
        }
        HubTopic::CoverArt => {
            super::cover::run(config_path)?;
            Ok(false)
        }
        HubTopic::Defaults => {
            super::defaults::run(config_path)?;
            Ok(false)
        }
        HubTopic::WebBridge => {
            super::bridge::run()?;
            Ok(false)
        }
    }
}

fn players_chip(config: &Config) -> String {
    let players = config.effective_player_configs();
    let enabled = players.values().filter(|p| !p.ignore).count();
    let total = players.len();
    format!("{enabled}/{total} on")
}

fn web_chip(config: &Config) -> String {
    let sites = config.effective_web_player_configs();
    let enabled: Vec<_> = sites
        .iter()
        .filter(|(k, v)| *k != "default" && !v.ignore)
        .map(|(k, v)| v.name.as_deref().unwrap_or(k.as_str()))
        .collect();
    if enabled.is_empty() {
        "none on".to_string()
    } else if enabled.len() <= 2 {
        format!("{} on", enabled.join(", "))
    } else {
        format!("{} on", enabled.len())
    }
}

fn discovery_chip(config: &Config) -> String {
    let mode = if config.event_driven {
        format!("event-driven, {}ms fallback", config.fallback_poll_interval)
    } else {
        format!("poll {}ms", config.interval)
    };
    if config.allowed_players.is_empty() {
        mode
    } else {
        format!("{} · {} allowed", mode, config.allowed_players.len())
    }
}

fn display_chip(config: &Config) -> String {
    let activity = format!("{:?}", config.activity_type.default).to_lowercase();
    let time = if config.time.show { "time on" } else { "time off" };
    format!("{activity} · {time}")
}

fn cover_chip(config: &Config) -> String {
    let providers = config.cover.provider.provider.join("→");
    let imgbb = if config.cover.provider.imgbb.api_key.is_some() {
        "imgbb set"
    } else {
        "no imgbb"
    };
    format!("{providers} · {imgbb}")
}

fn defaults_chip(config: &Config) -> String {
    let policy = if config.ignore_unmatched_players() {
        "hide unknown"
    } else {
        "allow unknown"
    };
    let overrides = match (
        config.has_user_default_player_override(),
        config.has_user_default_web_override(),
    ) {
        (true, true) => " · defaults overridden",
        (true, false) => " · player default overridden",
        (false, true) => " · web default overridden",
        (false, false) => "",
    };
    format!("{policy}{overrides}")
}

fn bridge_chip() -> String {
    let issues = web_bridge::native_host_issue_count();
    if issues == 0 {
        "ok".to_string()
    } else {
        format!("{issues} issue(s)")
    }
}
