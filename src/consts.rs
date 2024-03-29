pub const APP_NAME: &str = "mprisence";
pub const DEFAULT_APP_ID: &str = "1121632048155742288";
pub const DEFAULT_INTERVAL: u64 = 2000;

pub const DEFAULT_ICON: &str =
    "https://raw.githubusercontent.com/lazykern/mprisence/main/assets/icon.png";

pub const DEFAULT_DETAIL_TEMPLATE: &str = "{{{title}}}";
pub const DEFAULT_STATE_TEMPLATE: &str = "{{{status_icon}}} {{{artists}}} ";
pub const DEFAULT_LARGE_TEXT_TEMPLATE: &str =
    "{{#if album_name includeZero=true}}{{{album_name}}}{{else}}{{{title}}}{{/if}}";
pub const DEFAULT_SMALL_TEXT_TEMPLATE: &str = "Playing on {{player}}";
pub const DEFAULT_LARGE_TEXT_NO_COVER_TEMPLATE: &str =
    "{{#if album_name includeZero=true}}{{{album_name}}} | {{/if}}Playing on {{player}}";

pub const DEFAULT_IMAGE_FILE_NAMES: [&str; 5] = ["cover", "folder", "front", "album", "art"];

pub const DEFAULT_COVER_PROVIDER: &str = "musicbrainz";
