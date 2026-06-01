use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use mpris::Metadata;
use parking_lot::Mutex;
use thiserror::Error;
use tokio::process::Command;
use tokio::time::timeout;
use url::Url;

pub struct CmusState {
    pub track_id: Mutex<Option<Box<str>>>,
    pub path: Mutex<Option<PathBuf>>,
    pub error_logged: AtomicBool,
}

impl CmusState {
    pub fn new() -> Self {
        Self {
            track_id: Mutex::new(None),
            path: Mutex::new(None),
            error_logged: AtomicBool::new(false),
        }
    }

    pub fn reset(&self) {
        *self.track_id.lock() = None;
        *self.path.lock() = None;
        self.error_logged.store(false, Ordering::Relaxed);
    }

    /// Resolve a `file://` URL for the currently playing cmus track.
    /// Caches the path between ticks; resets on track change.
    pub async fn resolve_url(&self, metadata: &Metadata) -> Option<String> {
        let track_token = metadata
            .track_id()
            .map(|id| id.to_string())
            .or_else(|| metadata.url().map(|url| url.to_string()))
            .or_else(|| metadata.title().map(|title| title.to_string()));

        let track_changed = {
            let guard = self.track_id.lock();
            track_token.as_deref() != guard.as_deref()
        };

        if track_changed {
            *self.track_id.lock() = track_token.map(|token| token.into_boxed_str());
            *self.path.lock() = None;
            self.error_logged.store(false, Ordering::Relaxed);
        }

        if self.path.lock().is_none() {
            match get_current_track_path().await {
                Ok(Some(path)) => {
                    *self.path.lock() = Some(path);
                }
                Ok(None) => {}
                Err(err) => {
                    if !self.error_logged.load(Ordering::Relaxed) {
                        log::warn!("cmus-remote failed: {}", err);
                        self.error_logged.store(true, Ordering::Relaxed);
                    }
                }
            }
        }

        let cmus_path = self.path.lock().clone();
        if let Some(path) = cmus_path {
            match Url::from_file_path(&path) {
                Ok(url) => Some(url.to_string()),
                Err(_) => {
                    if !self.error_logged.load(Ordering::Relaxed) {
                        log::warn!("cmus-remote returned non-file path: {:?}", path);
                        self.error_logged.store(true, Ordering::Relaxed);
                    }
                    None
                }
            }
        } else {
            None
        }
    }
}

const CMUS_REMOTE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Error)]
pub enum CmusRemoteError {
    #[error("cmus-remote timed out after {0:?}")]
    Timeout(Duration),
    #[error("cmus-remote failed to launch: {0}")]
    Launch(#[from] std::io::Error),
    #[error("cmus-remote exited with status {status:?}: {stderr}")]
    NonZeroExit { status: Option<i32>, stderr: String },
}

pub async fn get_current_track_path() -> Result<Option<PathBuf>, CmusRemoteError> {
    let output = match timeout(
        CMUS_REMOTE_TIMEOUT,
        Command::new("cmus-remote").arg("-Q").output(),
    )
    .await
    {
        Ok(result) => result?,
        Err(_) => return Err(CmusRemoteError::Timeout(CMUS_REMOTE_TIMEOUT)),
    };

    if !output.status.success() {
        let mut stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            stderr = String::from_utf8_lossy(&output.stdout).trim().to_string();
        }
        if stderr.is_empty() {
            stderr = "no output".to_string();
        }
        return Err(CmusRemoteError::NonZeroExit {
            status: output.status.code(),
            stderr,
        });
    }

    Ok(parse_track_path(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_track_path(output: &str) -> Option<PathBuf> {
    for line in output.lines() {
        if let Some(rest) = line.strip_prefix("file ") {
            let path_str = rest.trim();
            if path_str.is_empty() {
                continue;
            }
            let path = PathBuf::from(path_str);
            if path.is_absolute() {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::parse_track_path;
    use std::path::PathBuf;

    #[test]
    fn parses_file_line() {
        let output = "status playing\nfile /music/test.flac\ntag artist Foo";
        assert_eq!(
            parse_track_path(output),
            Some(PathBuf::from("/music/test.flac"))
        );
    }

    #[test]
    fn ignores_non_file_lines() {
        let output = "status playing\nstream https://example.com/stream";
        assert_eq!(parse_track_path(output), None);
    }

    #[test]
    fn ignores_relative_paths() {
        let output = "file relative/path.mp3";
        assert_eq!(parse_track_path(output), None);
    }
}
