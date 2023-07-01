use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

use crate::{consts, error::Error};

pub enum ClientState {
    Connected,
    Disconnected,
}

pub struct Client {
    app_id: String,
    state: ClientState,
    client: Option<DiscordIpcClient>,
}

impl Client {
    pub fn new<T>(app_id: T) -> Self
    where
        T: Into<String>,
    {
        Client {
            app_id: app_id.into(),
            state: ClientState::Disconnected,
            client: None,
        }
    }

    pub fn connect(&mut self) -> Result<(), Error> {
        match self.state {
            ClientState::Connected => return Ok(()),
            ClientState::Disconnected => {}
        }

        let mut client = match DiscordIpcClient::new(self.app_id.as_str()) {
            Ok(client) => client,
            Err(_) => return Err(Error::DiscordError("Could not create client".to_string())),
        };

        match client.connect() {
            Ok(_) => {
                self.state = ClientState::Connected;
                self.client = Some(client);
                Ok(())
            }
            Err(_) => Err(Error::DiscordError(
                "Could not connect to discord".to_string(),
            )),
        }
    }

    pub fn set_activity(&mut self, activity: activity::Activity) -> Result<(), Error> {
        match self.client {
            Some(ref mut client) => {
                match client.set_activity(activity) {
                    Ok(_) => {}
                    Err(_) => {
                        return Err(Error::DiscordError("Could not set activity".to_string()))
                    }
                }
                Ok(())
            }
            None => Err(Error::DiscordError("Client is not connected".to_string())),
        }
    }

    pub fn clear_activity(&mut self) -> Result<(), Error> {
        match self.client {
            Some(ref mut client) => {
                match client.clear_activity() {
                    Ok(_) => {}
                    Err(_) => {
                        return Err(Error::DiscordError("Could not clear activity".to_string()))
                    }
                }
                Ok(())
            }
            None => Err(Error::DiscordError("Client is not connected".to_string())),
        }
    }

    pub fn close(&mut self) -> Result<(), Error> {
        match self.client {
            Some(ref mut client) => {
                match client.close() {
                    Ok(_) => {}
                    Err(_) => {
                        return Err(Error::DiscordError("Could not close client".to_string()))
                    }
                }
                self.state = ClientState::Disconnected;
                Ok(())
            }
            None => Ok(()),
        }
    }
}

impl Default for Client {
    fn default() -> Self {
        Client::new(consts::DEFAULT_APP_ID)
    }
}
