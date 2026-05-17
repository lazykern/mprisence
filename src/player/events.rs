use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;

use log::{debug, trace, warn};
use mpris::{Event as MprisEvent, PlayerFinder};
use smol_str::SmolStr;
use tokio::sync::{mpsc, Notify};

/// Event emitted by a per-player listener thread, forwarded to the async event loop.
#[derive(Debug)]
pub struct PlayerEvent {
    /// Normalised identity of the player (matches the key in `Mprisence::media_players`).
    pub norm_id: SmolStr,
    pub kind: PlayerEventKind,
}

#[derive(Debug)]
pub enum PlayerEventKind {
    /// A raw event from the `mpris` crate.
    Mpris(MprisEvent),
    /// The listener thread's iterator terminated (player quit or `is_running()` returned false).
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

/// Spawn an OS thread that subscribes to MPRIS `PropertiesChanged` / `Seeked` for a single
/// player and forwards each event into `tx`. Construct the `mpris::Player` *inside* the
/// thread because `PooledConnection` is `!Send`.
pub fn spawn_listener(
    bus_name: SmolStr,
    norm_id: SmolStr,
    tx: mpsc::Sender<PlayerEvent>,
    cancel: Arc<AtomicBool>,
    update_generation: Arc<std::sync::atomic::AtomicU64>,
    update_notify: Arc<Notify>,
) -> JoinHandle<()> {
    std::thread::Builder::new()
        .name(format!("mpris-listener:{}", norm_id))
        .spawn(move || {
            run_listener(
                bus_name,
                norm_id,
                tx,
                cancel,
                update_generation,
                update_notify,
            )
        })
        .expect("failed to spawn mpris listener thread")
}

fn run_listener(
    bus_name: SmolStr,
    norm_id: SmolStr,
    tx: mpsc::Sender<PlayerEvent>,
    cancel: Arc<AtomicBool>,
    update_generation: Arc<std::sync::atomic::AtomicU64>,
    update_notify: Arc<Notify>,
) {
    debug!("listener spawn for {} (bus={})", norm_id, bus_name.as_str());

    let player = match find_player_by_bus_name(&bus_name) {
        Ok(p) => p,
        Err(err) => {
            let msg = format!("failed to locate player on bus {}: {}", bus_name, err);
            warn!("{}", msg);
            let _ = tx.blocking_send(PlayerEvent {
                norm_id: norm_id.clone(),
                kind: PlayerEventKind::ListenerError(msg),
            });
            let _ = tx.blocking_send(PlayerEvent {
                norm_id,
                kind: PlayerEventKind::ListenerExited,
            });
            return;
        }
    };

    let events = match player.events() {
        Ok(e) => e,
        Err(err) => {
            let msg = format!("player.events() failed for {}: {}", bus_name, err);
            warn!("{}", msg);
            let _ = tx.blocking_send(PlayerEvent {
                norm_id: norm_id.clone(),
                kind: PlayerEventKind::ListenerError(msg),
            });
            let _ = tx.blocking_send(PlayerEvent {
                norm_id,
                kind: PlayerEventKind::ListenerExited,
            });
            return;
        }
    };

    for event in events {
        if cancel.load(Ordering::Relaxed) {
            debug!("listener for {} cancelled", norm_id);
            break;
        }
        match event {
            Ok(ev) => {
                trace!("event from {}: {:?}", norm_id, ev);
                if matches!(ev, MprisEvent::TrackChanged(_)) {
                    update_generation.fetch_add(1, Ordering::Relaxed);
                    update_notify.notify_waiters();
                }
                let msg = PlayerEvent {
                    norm_id: norm_id.clone(),
                    kind: PlayerEventKind::Mpris(ev),
                };
                if tx.blocking_send(msg).is_err() {
                    debug!("listener for {} exiting: receiver dropped", norm_id);
                    return;
                }
            }
            Err(err) => {
                warn!("event stream error for {}: {}", norm_id, err);
                let _ = tx.blocking_send(PlayerEvent {
                    norm_id: norm_id.clone(),
                    kind: PlayerEventKind::ListenerError(err.to_string()),
                });
                break;
            }
        }
    }

    debug!("listener for {} exited", norm_id);
    let _ = tx.blocking_send(PlayerEvent {
        norm_id,
        kind: PlayerEventKind::ListenerExited,
    });
}

fn find_player_by_bus_name(bus_name: &str) -> Result<mpris::Player, String> {
    let mut finder = PlayerFinder::new().map_err(|e| e.to_string())?;
    finder.set_player_timeout_ms(5000);
    for candidate in finder.iter_players().map_err(|e| e.to_string())? {
        match candidate {
            Ok(p) if p.bus_name() == bus_name => return Ok(p),
            Ok(_) => continue,
            Err(_) => continue,
        }
    }
    Err(format!("no player on bus {}", bus_name))
}
