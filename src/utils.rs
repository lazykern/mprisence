use crate::player::{PlayerId, PlayerState};
use mime_guess::mime;
use mpris::Metadata;
use mpris::PlaybackStatus;
use std::collections::BTreeMap;
use std::path::Path;
use url::Url;

pub fn to_snake_case(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut prev_is_lowercase = false;
    let mut prev_is_underscore = true; // Start true to handle first char

    while let Some(c) = chars.next() {
        if c.is_whitespace() {
            if !prev_is_underscore {
                result.push('_');
                prev_is_underscore = true;
            }
            prev_is_lowercase = false;
            continue;
        }

        if c.is_uppercase() {
            // Add underscore if previous char was lowercase
            // or if previous char was uppercase and next char is lowercase
            if (!prev_is_underscore && prev_is_lowercase)
                || (!prev_is_underscore
                    && !prev_is_lowercase
                    && chars.peek().map_or(false, |next| next.is_lowercase()))
            {
                result.push('_');
            }
            result.push(c.to_ascii_lowercase());
            prev_is_lowercase = false;
        } else {
            result.push(c.to_ascii_lowercase());
            prev_is_lowercase = true;
        }
        prev_is_underscore = c == '_';
    }

    result
}

pub fn format_duration(seconds: u64) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// Try to determine content type from metadata
pub fn get_content_type_from_metadata(metadata: &Metadata) -> Option<String> {
    // Check if we have a URL that might indicate content type
    if let Some(url) = metadata.url() {
        // Try to parse it as a URL first
        if let Ok(parsed_url) = Url::parse(url) {
            let path = parsed_url.path();
            let guess = mime_guess::from_path(path);
            if let Some(mime_type) = guess.first() {
                return Some(mime_type.to_string());
            }
        }

        // Check if it's a file path
        let path = Path::new(url);
        if path.exists() {
            let guess = mime_guess::from_path(path);
            if let Some(mime_type) = guess.first() {
                return Some(mime_type.to_string());
            }
        }
    }

    // Check for content type based on track id
    if let Some(track_id) = metadata.track_id() {
        let track_id_str = track_id.to_string();
        if track_id_str.contains("video") {
            return Some("video/unknown".to_string());
        } else if track_id_str.contains("audio") {
            return Some("audio/unknown".to_string());
        }
    }

    // Fallback: if we have audio artists, it's probably audio
    if metadata.artists().is_some() {
        return Some("audio/unknown".to_string());
    }

    None
}
