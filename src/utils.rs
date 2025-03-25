use mime_guess::Mime;
use url::Url;
use mpris::PlaybackStatus;

pub fn normalize_player_identity(input: &str) -> String {
    input.trim()
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<&str>>()
        .join("_")
}

pub fn format_duration(seconds: u64) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
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
