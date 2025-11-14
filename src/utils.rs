use mime_guess::Mime;
use mpris::PlaybackStatus;
use semver::Version;
use thiserror::Error;
use url::Url;

pub fn normalize_player_identity(input: &str) -> String {
    input
        .trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join("_")
}

pub fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, secs)
    } else {
        format!("{:02}:{:02}", minutes, secs)
    }
}

pub fn get_content_type_from_metadata(url: &str) -> Option<Mime> {
    if let Ok(parsed_url) = Url::parse(url) {
        let path = parsed_url.path();
        let guess = mime_guess::from_path(path);
        if let Some(mime_type) = guess.first() {
            return Some(mime_type);
        }
    }

    None
}

pub fn is_streaming_url(url: &str) -> bool {
    Url::parse(url)
        .map(|parsed| matches!(parsed.scheme(), "http" | "https"))
        .unwrap_or(false)
}

pub fn format_track_number(number: u32, total: Option<u32>) -> String {
    match total {
        Some(total) => format!("{}/{}", number, total),
        None => number.to_string(),
    }
}

pub fn format_audio_channels(channels: u8) -> String {
    match channels {
        1 => "Mono".to_string(),
        2 => "Stereo".to_string(),
        n => format!("{} channels", n),
    }
}

pub fn format_bitrate(bitrate: u32) -> String {
    format!("{} kbps", bitrate)
}

pub fn format_sample_rate(rate: u32) -> String {
    format!("{:.1} kHz", rate as f32 / 1000.0)
}

pub fn format_bit_depth(depth: u8) -> String {
    format!("{}-bit", depth)
}

pub fn format_playback_status_icon(status: PlaybackStatus) -> &'static str {
    match status {
        PlaybackStatus::Playing => "▶",
        PlaybackStatus::Paused => "⏸️",
        PlaybackStatus::Stopped => "⏹️",
    }
}

#[derive(Error, Debug)]
pub enum VersionError {
    #[error("Invalid version format: {0}")]
    InvalidFormat(#[from] semver::Error),
}

pub fn validate_version(version: &str) -> Result<String, VersionError> {
    // Parse version to validate it
    let version = Version::parse(version)?;
    Ok(version.to_string())
}

#[allow(dead_code)]
pub fn to_package_version(version: &str) -> Result<String, VersionError> {
    let version = Version::parse(version)?;
    Ok(version.to_string())
}

#[cfg(test)]
mod tests {

    use super::*;
    use mpris::PlaybackStatus;

    #[test]
    fn test_validate_version() {
        assert!(validate_version("1.0.0").is_ok());
        assert!(validate_version("1.0.0-beta.1").is_ok());
        assert!(validate_version("1.0.0+build.123").is_ok());
        assert!(validate_version("1.0.0-beta.1+build.123").is_ok());

        // Invalid versions
        assert!(validate_version("1.0").is_err());
        assert!(validate_version("1.0.0.beta1").is_err());
        assert!(validate_version("v1.0.0").is_err());
    }

    #[test]
    fn test_to_package_version() {
        assert_eq!(to_package_version("1.0.0-beta.1").unwrap(), "1.0.0-beta.1");
        assert_eq!(to_package_version("1.0.0").unwrap(), "1.0.0");
    }

    #[test]
    fn test_normalize_player_identity() {
        assert_eq!(normalize_player_identity("Spotify"), "spotify");
        assert_eq!(
            normalize_player_identity("  VLC Media Player  "),
            "vlc_media_player"
        );
        assert_eq!(normalize_player_identity("RHYTHMBOX"), "rhythmbox");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(0), "00:00");
        assert_eq!(format_duration(61), "01:01");
        assert_eq!(format_duration(3600), "1:00:00"); // 1 hour
        assert_eq!(format_duration(3723), "1:02:03"); // 1h 2m 3s
        assert_eq!(format_duration(7323), "2:02:03"); // 2h 2m 3s
        assert_eq!(format_duration(45296), "12:34:56"); // 12h 34m 56s
    }

    #[test]
    fn test_format_track_number() {
        assert_eq!(format_track_number(1, None), "1");
        assert_eq!(format_track_number(5, Some(12)), "5/12");
    }

    #[test]
    fn test_format_audio_channels() {
        assert_eq!(format_audio_channels(1), "Mono");
        assert_eq!(format_audio_channels(2), "Stereo");
        assert_eq!(format_audio_channels(6), "6 channels");
    }

    #[test]
    fn test_format_bitrate() {
        assert_eq!(format_bitrate(320), "320 kbps");
        assert_eq!(format_bitrate(128), "128 kbps");
    }

    #[test]
    fn test_format_sample_rate() {
        assert_eq!(format_sample_rate(44100), "44.1 kHz");
        assert_eq!(format_sample_rate(48000), "48.0 kHz");
    }

    #[test]
    fn test_format_bit_depth() {
        assert_eq!(format_bit_depth(16), "16-bit");
        assert_eq!(format_bit_depth(24), "24-bit");
    }

    #[test]
    fn test_format_playback_status_icon() {
        assert_eq!(format_playback_status_icon(PlaybackStatus::Playing), "▶");
        assert_eq!(format_playback_status_icon(PlaybackStatus::Paused), "⏸️");
        assert_eq!(format_playback_status_icon(PlaybackStatus::Stopped), "⏹️");
    }

    #[test]
    fn test_get_content_type_from_metadata() {
        let audio_url = "file:///music/song.mp3";
        let video_url = "file:///videos/movie.mp4";
        let image_url = "file:///images/cover.jpg";
        let unknown_url = "file:///unknown/file.unknown"; // Changed extension to something mime_guess won't recognize

        assert_eq!(
            get_content_type_from_metadata(audio_url)
                .unwrap()
                .type_()
                .as_str(),
            "audio"
        );
        assert_eq!(
            get_content_type_from_metadata(video_url)
                .unwrap()
                .type_()
                .as_str(),
            "video"
        );
        assert_eq!(
            get_content_type_from_metadata(image_url)
                .unwrap()
                .type_()
                .as_str(),
            "image"
        );
        assert!(get_content_type_from_metadata(unknown_url).is_none());
    }

    #[test]
    fn test_is_streaming_url() {
        assert!(is_streaming_url("https://example.com/track"));
        assert!(is_streaming_url("http://example.com/stream"));
        assert!(!is_streaming_url("file:///music/song.mp3"));
        assert!(!is_streaming_url("not a valid url"));
    }
}
