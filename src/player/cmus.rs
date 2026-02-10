use std::path::PathBuf;
use std::time::Duration;

use thiserror::Error;
use tokio::process::Command;
use tokio::time::timeout;

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
