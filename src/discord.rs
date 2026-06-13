use log::{debug, trace};
use std::path::PathBuf;

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

        for socket_path in potential_paths {
            if socket_path.exists() {
                trace!("IPC socket found at {:?}", socket_path);
                DISCORD_CONNECTION_ERROR_LOGGED.store(false, Ordering::Relaxed);
                return true;
            }
        }

        debug!("No Discord IPC socket found. Assuming not running.");

        if !DISCORD_CONNECTION_ERROR_LOGGED.load(Ordering::Relaxed)
            && DISCORD_CONNECTION_ERROR_LOGGED
                .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            log::info!(
                "Could not find Discord IPC socket. Presence updates will be disabled until connection succeeds."
            );
        }
        false
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
