use discord_rich_presence::{activity::*, DiscordIpc, DiscordIpcClient};

use crate::{config::PlayerConfig, error::Error, CONFIG};

pub struct Client {
    pub has_icon: bool,
    identity: String,
    unique_name: String,
    app_id: String,
    icon: String,
    client: Option<DiscordIpcClient>,
}

impl Client {
    pub fn new<T>(identity: T, unique_name: T) -> Self
    where
        T: Into<String>,
    {
        let identity = identity.into();
        let unique_name = unique_name.into();

        let fallback_player_config = PlayerConfig::default();
        let mut has_icon = false;

        let player_config = match CONFIG
            .player
            .get(&identity.to_lowercase().replace(" ", "_"))
        {
            Some(player_config) => {
                has_icon = true;
                player_config
            }
            None => match CONFIG.player.get("default") {
                Some(player_config) => player_config,
                None => &fallback_player_config,
            },
        };

        let app_id = player_config.app_id.clone();
        let icon = player_config.icon.clone();

        Client {
            identity,
            unique_name,
            app_id,
            icon,
            has_icon,
            client: None,
        }
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn unique_name(&self) -> &str {
        &self.unique_name
    }

    pub fn app_id(&self) -> &str {
        &self.app_id
    }

    pub fn icon(&self) -> &str {
        &self.icon
    }

    pub fn connect(&mut self) -> Result<(), Error> {
        if self.client.is_some() {
            return Ok(());
        }

        let mut client = match DiscordIpcClient::new(self.app_id()) {
            Ok(client) => client,
            Err(_) => {
                return Err(Error::DiscordError(
                    "Failed to connect to Discord".to_string(),
                ))
            }
        };

        match client.connect() {
            Ok(_) => {}
            Err(_) => {
                return Err(Error::DiscordError(
                    "Failed to connect to Discord".to_string(),
                ))
            }
        }

        self.client = Some(client);

        Ok(())
    }

    pub fn close(&mut self) -> Result<(), Error> {
        match &mut self.client {
            Some(client) => match client.close() {
                Ok(_) => {
                    self.client = None;
                }
                Err(_) => {
                    return Err(Error::DiscordError(
                        "Failed to close Discord connection".to_string(),
                    ))
                }
            },
            None => {}
        }
        Ok(())
    }

    pub fn set_activity(&mut self, activity: Activity) -> Result<(), Error> {
        match &mut self.client {
            Some(client) => match client.set_activity(activity) {
                Ok(_) => {}
                Err(_) => {
                    return Err(Error::DiscordError(
                        "Failed to update Discord activity".to_string(),
                    ))
                }
            },
            None => {}
        }
        Ok(())
    }

    pub fn clear(&mut self) -> Result<(), Error> {
        match &mut self.client {
            Some(client) => match client.clear_activity() {
                Ok(_) => {}
                Err(_) => {
                    return Err(Error::DiscordError(
                        "Failed to clear Discord activity".to_string(),
                    ))
                }
            },
            None => {}
        }
        Ok(())
    }
}
