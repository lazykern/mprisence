use std::fmt::Debug;

use discord_rich_presence::{DiscordIpc, DiscordIpcClient};

use crate::Activity;
use crate::{config::PlayerConfig, error::Error, CONFIG};

pub struct Client {
    pub has_icon: bool,
    identity: String,
    unique_name: String,
    app_id: String,
    icon: String,
    client: Option<DiscordIpcClient>,
    activity: Option<Activity>,
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
            activity: None,
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
            log::warn!("Client already connected");
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

        log::info!("Connected to Discord");

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
        if activity == self.activity.clone().unwrap_or_default() {
            log::debug!("Activity is the same, skipping update");
            return Ok(());
        }

        match &mut self.client {
            Some(client) => match client.set_activity(activity.to_discord_activity()) {
                Ok(_) => {}
                Err(_) => {
                    return Err(Error::DiscordError(
                        "Failed to update Discord activity".to_string(),
                    ))
                }
            },
            None => {}
        }

        self.activity = Some(activity.clone());

        log::debug!("Updated activity: {:?}", activity);

        Ok(())
    }

    pub fn clear(&mut self) -> Result<(), Error> {
        if self.activity.is_none() {
            log::debug!("Activity is already cleared, skipping update");
            return Ok(());
        }

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

        self.activity = None;
        log::debug!("Cleared activity");
        Ok(())
    }
}

impl Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut binding = f.debug_struct("Client");
        let s = binding
            .field("identity", &self.identity)
            .field("unique_name", &self.unique_name)
            .field("app_id", &self.app_id)
            .field("icon", &self.icon)
            .field("has_icon", &self.has_icon)
            .field("client_is_some", &self.client.is_some());

        s.finish()
    }
}
