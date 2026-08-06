use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;

use dbus::{ffidisp::Connection, message::MatchRule, Message, MessageType};
use log::{debug, trace, warn};
use smol_str::SmolStr;
use tokio::sync::mpsc;

const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const DBUS_INTERFACE: &str = "org.freedesktop.DBus";
const PROPERTIES_INTERFACE: &str = "org.freedesktop.DBus.Properties";
const PLAYER_INTERFACE: &str = "org.mpris.MediaPlayer2.Player";
const TRACK_LIST_INTERFACE: &str = "org.mpris.MediaPlayer2.TrackList";
const LISTENER_POLL_MS: u32 = 250;
const DBUS_CALL_TIMEOUT_MS: i32 = 1_000;

/// Event emitted by a per-player listener thread, forwarded to the async event loop.
#[derive(Debug)]
pub struct PlayerEvent {
    /// Normalised identity of the player (matches the key in `Mprisence::media_players`).
    pub norm_id: SmolStr,
    /// Distinguishes delayed events from a listener that has already been replaced.
    pub listener_generation: u64,
    pub kind: PlayerEventKind,
}

#[derive(Debug)]
pub enum PlayerEventKind {
    /// A Discord-relevant MPRIS signal was received; re-read the current player state.
    Refresh,
    /// The player released or transferred its well-known D-Bus name.
    ListenerExited,
    /// The listener encountered a D-Bus error while polling for events.
    ListenerError(String),
}

/// Returned by `Presence::handle_event` so the main loop can act on lifecycle changes.
#[derive(Debug, PartialEq, Eq)]
pub enum EventOutcome {
    Continue,
    ShouldRemove,
}

#[derive(Debug, PartialEq, Eq)]
enum SignalAction {
    Refresh,
    PlayerExited,
    Ignore,
}

#[derive(Debug, PartialEq, Eq)]
enum ListenerExit {
    Cancelled,
    PlayerExited,
    ReceiverClosed,
}

/// Spawn a cancellable listener on a private D-Bus connection. The bus match rules are scoped
/// to this player's unique owner, so signals from other MPRIS players never enter this listener's
/// queue.
pub fn spawn_listener(
    bus_name: SmolStr,
    norm_id: SmolStr,
    listener_generation: u64,
    tx: mpsc::Sender<PlayerEvent>,
    cancel: Arc<AtomicBool>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name(format!("mpris-listener:{}", norm_id))
        .spawn(move || run_listener(bus_name, norm_id, listener_generation, tx, cancel))
        .expect("failed to spawn mpris listener thread")
}

fn run_listener(
    bus_name: SmolStr,
    norm_id: SmolStr,
    listener_generation: u64,
    tx: mpsc::Sender<PlayerEvent>,
    cancel: Arc<AtomicBool>,
) {
    debug!("listener spawn for {} (bus={})", norm_id, bus_name.as_str());

    match listen(&bus_name, &norm_id, listener_generation, &tx, &cancel) {
        Ok(ListenerExit::Cancelled) => {
            debug!("listener for {} cancelled", norm_id);
        }
        Ok(ListenerExit::ReceiverClosed) => {
            debug!("listener for {} exiting: receiver dropped", norm_id);
        }
        Ok(ListenerExit::PlayerExited) => {
            debug!("listener for {} observed player exit", norm_id);
            send_event(
                &tx,
                &norm_id,
                listener_generation,
                PlayerEventKind::ListenerExited,
            );
        }
        Err(err) => {
            if cancel.load(Ordering::Acquire) {
                debug!("listener for {} cancelled during setup", norm_id);
                return;
            }
            warn!("event listener error for {}: {}", norm_id, err);
            send_event(
                &tx,
                &norm_id,
                listener_generation,
                PlayerEventKind::ListenerError(err),
            );
            send_event(
                &tx,
                &norm_id,
                listener_generation,
                PlayerEventKind::ListenerExited,
            );
        }
    }
}

fn listen(
    bus_name: &str,
    norm_id: &str,
    listener_generation: u64,
    tx: &mpsc::Sender<PlayerEvent>,
    cancel: &AtomicBool,
) -> Result<ListenerExit, String> {
    let connection = Connection::new_session().map_err(|err| err.to_string())?;

    // Subscribe to ownership changes first so a player exit cannot race with GetNameOwner.
    connection
        .add_match(&name_owner_match_rule(bus_name))
        .map_err(|err| err.to_string())?;

    let unique_owner = get_name_owner(&connection, bus_name)?;
    let player_rule = MatchRule::new()
        .with_type(MessageType::Signal)
        .with_strict_sender(unique_owner.as_str())
        .with_path(MPRIS_PATH)
        .match_str();
    connection
        .add_match(&player_rule)
        .map_err(|err| err.to_string())?;

    debug!(
        "listener attached for {} (bus={}, owner={})",
        norm_id, bus_name, unique_owner
    );

    loop {
        if cancel.load(Ordering::Acquire) {
            return Ok(ListenerExit::Cancelled);
        }

        let Some(message) = connection.incoming(LISTENER_POLL_MS).next() else {
            if !connection.is_connected() {
                return Err("session D-Bus connection closed".to_string());
            }
            continue;
        };

        match classify_signal(&message, bus_name, &unique_owner) {
            SignalAction::Refresh => {
                trace!("refresh signal from {}", norm_id);
                let event = PlayerEvent {
                    norm_id: SmolStr::new(norm_id),
                    listener_generation,
                    kind: PlayerEventKind::Refresh,
                };
                match tx.try_send(event) {
                    Ok(()) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        trace!("dropping coalescible refresh for {}: channel full", norm_id);
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        return Ok(ListenerExit::ReceiverClosed);
                    }
                }
            }
            SignalAction::PlayerExited => return Ok(ListenerExit::PlayerExited),
            SignalAction::Ignore => {}
        }
    }
}

fn get_name_owner(connection: &Connection, bus_name: &str) -> Result<String, String> {
    let request = Message::new_method_call(
        DBUS_INTERFACE,
        "/org/freedesktop/DBus",
        DBUS_INTERFACE,
        "GetNameOwner",
    )
    .map_err(|err| err.to_string())?
    .append1(bus_name.to_string());
    let reply = connection
        .send_with_reply_and_block(request, DBUS_CALL_TIMEOUT_MS)
        .map_err(|err| err.to_string())?;
    reply.read1::<String>().map_err(|err| err.to_string())
}

fn name_owner_match_rule(bus_name: &str) -> String {
    format!(
        "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',member='NameOwnerChanged',arg0='{}'",
        bus_name
    )
}

fn classify_signal(message: &Message, bus_name: &str, unique_owner: &str) -> SignalAction {
    let message_interface = message.interface();
    let message_member = message.member();
    let interface = message_interface.as_deref();
    let member = message_member.as_deref();

    if interface == Some(DBUS_INTERFACE) && member == Some("NameOwnerChanged") {
        let Ok((name, old_owner, new_owner)) = message.read3::<String, String, String>() else {
            return SignalAction::Ignore;
        };
        return if name == bus_name && old_owner == unique_owner && new_owner != unique_owner {
            SignalAction::PlayerExited
        } else {
            SignalAction::Ignore
        };
    }

    let message_sender = message.sender();
    let message_path = message.path();
    if message_sender.as_deref() != Some(unique_owner)
        || message_path.as_deref() != Some(MPRIS_PATH)
    {
        return SignalAction::Ignore;
    }

    match (interface, member) {
        (Some(PROPERTIES_INTERFACE), Some("PropertiesChanged")) => match message.read1::<&str>() {
            Ok(PLAYER_INTERFACE | TRACK_LIST_INTERFACE) => SignalAction::Refresh,
            _ => SignalAction::Ignore,
        },
        (Some(PLAYER_INTERFACE), Some("Seeked"))
        | (Some(TRACK_LIST_INTERFACE), Some("TrackMetadataChanged")) => SignalAction::Refresh,
        _ => SignalAction::Ignore,
    }
}

fn send_event(
    tx: &mpsc::Sender<PlayerEvent>,
    norm_id: &str,
    listener_generation: u64,
    kind: PlayerEventKind,
) {
    let event = PlayerEvent {
        norm_id: SmolStr::new(norm_id),
        listener_generation,
        kind,
    };
    if let Err(err) = tx.try_send(event) {
        trace!("dropping listener lifecycle event for {}: {}", norm_id, err);
    }
}

/// Drain queued refreshes for the same player/listener while preserving lifecycle events and
/// events belonging to other players.
pub fn drain_latest_refresh(
    mut event: PlayerEvent,
    rx: &mut mpsc::Receiver<PlayerEvent>,
) -> (PlayerEvent, Vec<PlayerEvent>) {
    let mut deferred = Vec::new();
    if !matches!(event.kind, PlayerEventKind::Refresh) {
        return (event, deferred);
    }

    while let Ok(newer) = rx.try_recv() {
        if newer.norm_id == event.norm_id
            && newer.listener_generation == event.listener_generation
            && matches!(newer.kind, PlayerEventKind::Refresh)
        {
            trace!("drain: coalescing refresh for {}", event.norm_id);
            event = newer;
        } else {
            deferred.push(newer);
        }
    }
    (event, deferred)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbus::strings::BusName;

    const BUS_NAME: &str = "org.mpris.MediaPlayer2.test";
    const UNIQUE_OWNER: &str = ":1.42";

    fn signal(interface: &str, member: &str, sender: &str) -> Message {
        let mut message = Message::new_signal(MPRIS_PATH, interface, member).unwrap();
        message.set_sender(Some(BusName::new(sender).unwrap()));
        message
    }

    fn event(norm_id: &str, listener_generation: u64, kind: PlayerEventKind) -> PlayerEvent {
        PlayerEvent {
            norm_id: SmolStr::new(norm_id),
            listener_generation,
            kind,
        }
    }

    #[test]
    fn accepts_player_properties_from_expected_owner() {
        let message = signal(PROPERTIES_INTERFACE, "PropertiesChanged", UNIQUE_OWNER)
            .append1(PLAYER_INTERFACE.to_string());

        assert_eq!(
            classify_signal(&message, BUS_NAME, UNIQUE_OWNER),
            SignalAction::Refresh
        );
    }

    #[test]
    fn rejects_player_signal_from_unrelated_owner() {
        let message = signal(PROPERTIES_INTERFACE, "PropertiesChanged", ":1.99")
            .append1(PLAYER_INTERFACE.to_string());

        assert_eq!(
            classify_signal(&message, BUS_NAME, UNIQUE_OWNER),
            SignalAction::Ignore
        );
    }

    #[test]
    fn detects_loss_of_expected_owner() {
        let message = signal(DBUS_INTERFACE, "NameOwnerChanged", DBUS_INTERFACE).append3(
            BUS_NAME.to_string(),
            UNIQUE_OWNER.to_string(),
            String::new(),
        );

        assert_eq!(
            classify_signal(&message, BUS_NAME, UNIQUE_OWNER),
            SignalAction::PlayerExited
        );
    }

    #[test]
    fn coalesces_only_refreshes_from_same_listener() {
        let (tx, mut rx) = mpsc::channel(8);
        tx.try_send(event("alpha", 7, PlayerEventKind::Refresh))
            .unwrap();
        tx.try_send(event("alpha", 8, PlayerEventKind::Refresh))
            .unwrap();
        tx.try_send(event("beta", 7, PlayerEventKind::Refresh))
            .unwrap();
        tx.try_send(event("alpha", 7, PlayerEventKind::ListenerExited))
            .unwrap();

        let first = event("alpha", 7, PlayerEventKind::Refresh);
        let (_, deferred) = drain_latest_refresh(first, &mut rx);

        assert_eq!(deferred.len(), 3);
        assert_eq!(deferred[0].listener_generation, 8);
        assert_eq!(deferred[1].norm_id, "beta");
        assert!(matches!(deferred[2].kind, PlayerEventKind::ListenerExited));
    }
}
