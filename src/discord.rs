use log::trace;
use std::path::PathBuf;

fn get_discord_lock_path() -> Option<PathBuf> {
    if cfg!(unix) {
        dirs::config_dir().map(|p| p.join("discord").join("SingletonLock"))
    } else if cfg!(windows) {
        dirs::config_dir().map(|p| p.join("discord").join("SingletonLock"))
    } else {
        None
    }
}

pub fn is_discord_running() -> bool {
    if let Some(lock_path) = get_discord_lock_path() {
        // Use symlink_metadata instead of metadata to handle symlinks
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
        // If we can't determine the lock path, assume Discord is running
        trace!("Could not determine Discord lock path, assuming Discord is running");
        true
    }
} 