use std::fmt::Debug;

use discord_rich_presence::{DiscordIpc, DiscordIpcClient};

use crate::context::Context;
use crate::Activity;
use crate::{config::PlayerConfig, error::Error};

pub struct Client {
    identity: String,
    unique_name: String,
    app_id: String,
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
        let app_id = PlayerConfig::get_or_default(&identity)
            .app_id_or_default()
            .to_string();

        Client {
            identity,
            unique_name,
            app_id,
            client: None,
            activity: None,
        }
    }

    pub fn activity(&self) -> Option<&Activity> {
        self.activity.as_ref()
    }

    pub fn from_context(context: &Context) -> Self {
        let identity = context.identity();
        let unique_name = context.unique_name();

        Self::new(identity, unique_name)
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

    pub fn is_connected(&self) -> bool {
        self.client.is_some()
    }

    pub fn connect(&mut self) -> Result<(), Error> {
        if self.client.is_some() {
            log::info!("Client already connected");
            return Ok(());
        }

        let mut client = match DiscordIpcClient::new(self.app_id()) {
            Ok(client) => client,
            Err(e) => return Err(Error::DiscordError(e)),
        };

        match client.connect() {
            Ok(_) => {}
            Err(e) => return Err(Error::DiscordError(e)),
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
                Err(e) => return Err(Error::DiscordError(e)),
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
                Err(e) => return Err(Error::DiscordError(e)),
            },
            None => {}
        }
        Ok(())
    }

    pub fn set_activity(&mut self, activity: &Activity) -> Result<(), Error> {
        if self.client.is_none() {
            log::warn!("Client is not connected, skipping update");
            return Ok(());
        }

        if activity == &self.activity.clone().unwrap_or_default() {
            log::debug!("Activity is the same, skipping update");
            return Ok(());
        }

        self.clear().unwrap_or_default();

        match &mut self.client {
            Some(client) => match client.set_activity(activity.to_discord_activity()) {
                Ok(_) => {}
                Err(e) => {
                    return Err(Error::DiscordError(e));
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
                Err(e) => return Err(Error::DiscordError(e)),
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
            .field("client_is_some", &self.client.is_some());

        s.finish()
    }
}
