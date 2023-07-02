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
        let mut app_id = fallback_player_config.app_id.as_ref().unwrap();
        let mut icon = fallback_player_config.icon.as_ref().unwrap();
        let mut has_icon = false;

        match CONFIG.player.get("default") {
            Some(player_config) => {
                app_id = player_config.app_id.as_ref().unwrap_or(app_id);
                icon = player_config.icon.as_ref().unwrap_or(icon);
            }
            None => {}
        }

        match CONFIG
            .player
            .get(&identity.to_lowercase().replace(" ", "_"))
        {
            Some(player_config) => {
                app_id = player_config.app_id.as_ref().unwrap_or(app_id);
                if let Some(i) = player_config.icon.as_ref() {
                    icon = i;
                    has_icon = true;
                }
            }
            None => {}
        };

        let app_id = app_id.to_string();
        let icon = icon.to_string();

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

    pub fn reconnect(&mut self) -> Result<(), Error> {
        match &mut self.client {
            Some(client) => match client.reconnect() {
                Ok(_) => {
                    self.client = None;
                }
                Err(_) => {
                    return Err(Error::DiscordError(
                        "Failed to reconnect to Discord".to_string(),
                    ))
                }
            },
            None => {}
        }
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
