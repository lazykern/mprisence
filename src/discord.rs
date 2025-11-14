use log::{debug, trace};
use std::io::ErrorKind;
use std::path::PathBuf;

use interprocess::local_socket::{prelude::*, GenericFilePath, Stream};
use std::collections::HashMap;
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};

static DISCORD_CONNECTION_ERROR_LOGGED: AtomicBool = AtomicBool::new(false);

fn get_discord_lock_path() -> Option<PathBuf> {
    if cfg!(windows) {
        dirs::config_dir().map(|p| p.join("discord").join("SingletonLock"))
    } else {
        None
    }
}

pub fn is_discord_running() -> bool {
    if cfg!(unix) {
        let runtime_dir = match env::var("XDG_RUNTIME_DIR") {
            Ok(dir) => PathBuf::from(dir),
            Err(_) => PathBuf::from("/tmp"),
        };

        let potential_paths = ((0..=9).map(|i| runtime_dir.join(format!("discord-ipc-{}", i))))
            .chain((0..=9).map(|i| {
                runtime_dir
                    .join("app/com.discordapp.Discord")
                    .join(format!("discord-ipc-{}", i))
            }));

        let mut successful_path: Option<PathBuf> = None;
        let mut error_counts: HashMap<ErrorKind, usize> = HashMap::new();

        for socket_path in potential_paths {
            let name = match socket_path.clone().to_fs_name::<GenericFilePath>() {
                Ok(n) => n,
                Err(e) => {
                    trace!(
                        "Failed to create IPC socket name from path {:?}: {}",
                        socket_path,
                        e
                    );
                    continue;
                }
            };

            match Stream::connect(name) {
                Ok(conn) => {
                    drop(conn);
                    successful_path = Some(socket_path);
                    DISCORD_CONNECTION_ERROR_LOGGED.store(false, Ordering::Relaxed);
                    break;
                }
                Err(e) => {
                    *error_counts.entry(e.kind()).or_insert(0) += 1;
                }
            }
        }

        if let Some(path) = successful_path {
            trace!("Successfully connected via {:?}.", path);
            true
        } else {
            debug!("Could not connect to Discord IPC socket. Assuming not running.");
            if !error_counts.is_empty() {
                let error_summary = error_counts
                    .iter()
                    .map(|(kind, count)| format!("{:?}: {}", kind, count))
                    .collect::<Vec<_>>()
                    .join(", ");
                debug!("Connection errors encountered: [{}]", error_summary);
            }

            if !DISCORD_CONNECTION_ERROR_LOGGED.load(Ordering::Relaxed)
                && DISCORD_CONNECTION_ERROR_LOGGED
                    .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
                    .is_ok()
            {
                log::info!(
                    "Could not connect to Discord IPC socket. Presence updates will be disabled until connection succeeds."
                );
            }
            false
        }
    } else if let Some(lock_path) = get_discord_lock_path() {
        match std::fs::symlink_metadata(&lock_path) {
            Ok(_) => {
                trace!("Discord SingletonLock found at {:?}", lock_path);
                true
            }
            Err(_) => {
                trace!("Discord SingletonLock not found at {:?}", lock_path);
                false
            }
        }
    } else {
        trace!("Could not determine Discord check method on non-unix, assuming running");
        true
    }
}
