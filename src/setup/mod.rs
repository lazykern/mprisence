mod bridge;
mod cover;
mod defaults;
mod discovery;
mod display;
mod effective;
mod editor;
mod fields;
mod global;
mod hints;
mod hub;
mod mpris;
mod patch;
mod players;
mod quick_toggle;
mod ui;
mod web;

use std::path::PathBuf;

use crate::config;
use crate::error::Error;

pub use hub::HubTopic;

pub fn run(section: Option<HubTopic>) -> Result<(), Error> {
    let _terminal = ui::SetupTerminal::enter()?;

    let (config_path, config) = config::load_merged_config(None)?;

    if let Some(topic) = section {
        if topic == HubTopic::Exit {
            return Ok(());
        }
        hub::run_topic(topic, &config, &config_path)?;
        return Ok(());
    }

    hub_loop(&config_path)
}

fn hub_loop(initial_path: &PathBuf) -> Result<(), Error> {
    loop {
        let (config_path, config) = config::load_merged_config(Some(initial_path))?;
        ui::redraw(&["Settings"], "mprisence setup")?;

        let items = hub::hub_items(&config);
        let labels: Vec<String> = items.iter().map(|item| item.line.clone()).collect();

        let choice = match ui::prompt_select("", labels)? {
            Some(line) => line,
            None => break,
        };

        let topic = items
            .into_iter()
            .find(|item| item.line == choice)
            .map(|item| item.topic)
            .unwrap_or(HubTopic::Exit);

        if hub::run_topic(topic, &config, &config_path)? {
            break;
        }
    }

    Ok(())
}

pub fn hub_topic_from_cli(name: &str) -> Option<HubTopic> {
    match name {
        "players" => Some(HubTopic::Players),
        "web" | "web-sites" | "websites" => Some(HubTopic::WebSites),
        "discovery" => Some(HubTopic::Discovery),
        "display" => Some(HubTopic::Display),
        "cover" | "cover-art" => Some(HubTopic::CoverArt),
        "defaults" => Some(HubTopic::Defaults),
        "bridge" | "web-bridge" => Some(HubTopic::WebBridge),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::hub_topic_from_cli;
    use super::HubTopic;

    #[test]
    fn hub_topic_from_cli_primary_names() {
        assert_eq!(hub_topic_from_cli("players"), Some(HubTopic::Players));
        assert_eq!(hub_topic_from_cli("discovery"), Some(HubTopic::Discovery));
        assert_eq!(hub_topic_from_cli("display"), Some(HubTopic::Display));
        assert_eq!(hub_topic_from_cli("defaults"), Some(HubTopic::Defaults));
    }

    #[test]
    fn hub_topic_from_cli_aliases() {
        assert_eq!(hub_topic_from_cli("web"), Some(HubTopic::WebSites));
        assert_eq!(hub_topic_from_cli("web-sites"), Some(HubTopic::WebSites));
        assert_eq!(hub_topic_from_cli("websites"), Some(HubTopic::WebSites));
        assert_eq!(hub_topic_from_cli("cover-art"), Some(HubTopic::CoverArt));
        assert_eq!(hub_topic_from_cli("web-bridge"), Some(HubTopic::WebBridge));
    }

    #[test]
    fn hub_topic_from_cli_unknown() {
        assert_eq!(hub_topic_from_cli("nope"), None);
        assert_eq!(hub_topic_from_cli(""), None);
    }
}
