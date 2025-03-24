use mime_guess::Mime;
use url::Url;

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
