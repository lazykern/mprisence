pub const APP_NAME: &str = "mprisence";
pub const DEFAULT_APP_ID: &str = "1121632048155742288";
pub fn default_app_id() -> String {
    DEFAULT_APP_ID.to_string()
}

pub const DEFAULT_ICON: &str =
    "https://raw.githubusercontent.com/phusitsom/mprisence/main/assets/icon.png";
pub fn default_icon() -> String {
    DEFAULT_ICON.to_string()
}

pub const DEFAULT_DETAIL: &str = "{{{title}}} - {{{artist}}}";

pub fn default_detail() -> String {
    DEFAULT_DETAIL.to_string()
}
pub const DEFAULT_STATE: &str = "{{{album}}}";

pub fn default_state() -> String {
    DEFAULT_STATE.to_string()
}

pub fn default_i8max() -> i8 {
    i8::MAX
}

pub fn default_false() -> bool {
    false
}
