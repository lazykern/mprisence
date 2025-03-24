use mime_guess::Mime;
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
