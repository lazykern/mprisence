use std::{collections::HashMap, rc::Rc, time::Duration};

use dbus::{
    arg::{Arg, Get, Variant},
    ffidisp::Connection,
    Message,
};
use mpris::{Metadata, MetadataValue, PlaybackStatus};
use thiserror::Error;

const DBUS_DESTINATION: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";
const DBUS_INTERFACE: &str = "org.freedesktop.DBus";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const ROOT_INTERFACE: &str = "org.mpris.MediaPlayer2";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const MPRIS_BUS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const DEFAULT_TIMEOUT_MS: i32 = 5_000;

#[derive(Debug, Error)]
pub enum ClientError {
    #[error("D-Bus error: {0}")]
    DBus(#[from] dbus::Error),
    #[error("invalid D-Bus message: {0}")]
    Message(String),
    #[error("invalid playback status: {0}")]
    PlaybackStatus(String),
}

impl ClientError {
    pub fn name(&self) -> Option<&str> {
        match self {
            Self::DBus(error) => error.name(),
            Self::Message(_) | Self::PlaybackStatus(_) => None,
        }
    }

    pub fn message(&self) -> Option<&str> {
        match self {
            Self::DBus(error) => error.message(),
            Self::Message(message) | Self::PlaybackStatus(message) => Some(message),
        }
    }
}

#[derive(Debug)]
pub struct PlayerFinder {
    connection: Rc<Connection>,
    player_timeout_ms: i32,
}

impl PlayerFinder {
    pub fn new() -> Result<Self, ClientError> {
        Ok(Self {
            connection: Rc::new(Connection::new_session()?),
            player_timeout_ms: DEFAULT_TIMEOUT_MS,
        })
    }

    pub fn set_player_timeout_ms(&mut self, timeout_ms: i32) {
        self.player_timeout_ms = timeout_ms;
    }

    pub fn iter_players(
        &self,
    ) -> Result<std::vec::IntoIter<Result<Player, ClientError>>, ClientError> {
        let request = method_call(DBUS_DESTINATION, DBUS_PATH, DBUS_INTERFACE, "ListNames")?;
        let reply = self
            .connection
            .send_with_reply_and_block(request, self.player_timeout_ms)?;
        let mut names = reply
            .read1::<Vec<String>>()
            .map_err(|error| ClientError::Message(error.to_string()))?;
        names.retain(|name| name.starts_with(MPRIS_BUS_PREFIX));
        names.sort_unstable();

        let players = names
            .into_iter()
            .map(|bus_name| Player::new(self.connection.clone(), bus_name, self.player_timeout_ms))
            .collect::<Vec<_>>();
        Ok(players.into_iter())
    }
}

#[derive(Debug)]
pub struct Player {
    connection: Rc<Connection>,
    bus_name: String,
    unique_name: String,
    identity: String,
    timeout_ms: i32,
}

impl Player {
    fn new(
        connection: Rc<Connection>,
        bus_name: String,
        timeout_ms: i32,
    ) -> Result<Self, ClientError> {
        let unique_name = call_string_method(
            &connection,
            DBUS_DESTINATION,
            DBUS_PATH,
            DBUS_INTERFACE,
            "GetNameOwner",
            &bus_name,
            timeout_ms,
        )?;
        let identity = get_property::<String>(
            &connection,
            &bus_name,
            ROOT_INTERFACE,
            "Identity",
            timeout_ms,
        )?;

        Ok(Self {
            connection,
            bus_name,
            unique_name,
            identity,
            timeout_ms,
        })
    }

    pub fn bus_name(&self) -> &str {
        &self.bus_name
    }

    pub fn unique_name(&self) -> &str {
        &self.unique_name
    }

    pub fn identity(&self) -> &str {
        &self.identity
    }

    pub fn set_dbus_timeout_ms(&mut self, timeout_ms: i32) {
        self.timeout_ms = timeout_ms;
    }

    pub fn get_playback_status(&self) -> Result<PlaybackStatus, ClientError> {
        let value = get_property::<String>(
            &self.connection,
            &self.bus_name,
            PLAYER_INTERFACE,
            "PlaybackStatus",
            self.timeout_ms,
        )?;
        value
            .parse()
            .map_err(|_| ClientError::PlaybackStatus(value))
    }

    pub fn get_metadata(&self) -> Result<Metadata, ClientError> {
        let values = get_property::<HashMap<String, MetadataValue>>(
            &self.connection,
            &self.bus_name,
            PLAYER_INTERFACE,
            "Metadata",
            self.timeout_ms,
        )?;
        Ok(Metadata::from(values))
    }

    pub fn get_position(&self) -> Result<Duration, ClientError> {
        let position = get_property::<i64>(
            &self.connection,
            &self.bus_name,
            PLAYER_INTERFACE,
            "Position",
            self.timeout_ms,
        )?;
        Ok(Duration::from_micros(position.max(0) as u64))
    }

    pub fn get_volume(&self) -> Result<f64, ClientError> {
        get_property::<f64>(
            &self.connection,
            &self.bus_name,
            PLAYER_INTERFACE,
            "Volume",
            self.timeout_ms,
        )
    }
}

fn method_call(
    destination: &str,
    path: &str,
    interface: &str,
    member: &str,
) -> Result<Message, ClientError> {
    Message::new_method_call(destination, path, interface, member).map_err(ClientError::Message)
}

fn call_string_method(
    connection: &Connection,
    destination: &str,
    path: &str,
    interface: &str,
    member: &str,
    argument: &str,
    timeout_ms: i32,
) -> Result<String, ClientError> {
    let request = method_call(destination, path, interface, member)?.append1(argument.to_string());
    let reply = connection.send_with_reply_and_block(request, timeout_ms)?;
    reply
        .read1::<String>()
        .map_err(|error| ClientError::Message(error.to_string()))
}

fn get_property<T>(
    connection: &Connection,
    destination: &str,
    interface: &str,
    property: &str,
    timeout_ms: i32,
) -> Result<T, ClientError>
where
    T: Arg + for<'a> Get<'a> + 'static,
{
    let request = method_call(destination, MPRIS_PATH, PROPERTIES_INTERFACE, "Get")?
        .append2(interface.to_string(), property.to_string());
    let reply = connection.send_with_reply_and_block(request, timeout_ms)?;
    reply
        .read1::<Variant<T>>()
        .map(|value| value.0)
        .map_err(|error| ClientError::Message(error.to_string()))
}
