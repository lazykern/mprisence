use log::warn;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::utils::normalize_player_identity;

pub const DEFAULT_CLEAR_ON_PAUSE: bool = true;
pub const DEFAULT_INTERVAL: u64 = 2000;
pub const DEFAULT_USE_CONTENT_TYPE: bool = true;
pub const DEFAULT_ACTIVITY_TYPE: ActivityType = ActivityType::Listening;
pub const DEFAULT_TIME_SHOW: bool = true;
pub const DEFAULT_TIME_AS_ELAPSED: bool = false;
pub const DEFAULT_IMGBB_EXPIRATION: u64 = 86400;

pub const DEFAULT_PLAYER_APP_ID: &str = "1121632048155742288";
pub const DEFAULT_PLAYER_ICON: &str =
    "https://raw.githubusercontent.com/lazykern/mprisence/main/assets/icon.png";
pub const DEFAULT_PLAYER_IGNORE: bool = false;
pub const DEFAULT_PLAYER_SHOW_ICON: bool = false;
pub const DEFAULT_PLAYER_ALLOW_STREAMING: bool = false;

const DEFAULT_TEMPLATE_DETAIL: &str = "{{{title}}}";
const DEFAULT_TEMPLATE_STATE: &str = "{{{artists}}}";
const DEFAULT_TEMPLATE_LARGE_TEXT: &str =
    "{{#if album_name includeZero=true}}{{{album_name}}}{{else}}{{{title}}}{{/if}}";
const DEFAULT_TEMPLATE_SMALL_TEXT: &str = "Playing on {{{player}}}";

const DEFAULT_COVER_FILE_NAMES: [&str; 5] = ["cover", "folder", "front", "album", "art"];
const DEFAULT_COVER_PROVIDERS: [&str; 2] = ["musicbrainz", "imgbb"];
const DEFAULT_COVER_LOCAL_SEARCH_DEPTH: usize = 2;
const DEFAULT_MUSICBRAINZ_MIN_SCORE: u8 = 95;
const DEFAULT_CATBOX_USE_LITTER: bool = false;
const DEFAULT_CATBOX_LITTER_HOURS: u8 = 24;

mod normalized_string {
    use crate::utils::normalize_player_identity;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::HashMap;

    pub fn serialize<S>(
        map: &HashMap<String, super::PlayerConfig>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        map.serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<HashMap<String, super::PlayerConfig>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let temp_map = HashMap::<String, super::PlayerConfig>::deserialize(deserializer)?;

        let mut final_map: HashMap<String, super::PlayerConfig> = HashMap::new();

        for (key, value) in temp_map {
            let normalized_key = normalize_player_identity(&key);

            if let Some(existing) = final_map.get(&normalized_key).cloned() {
                // If we have a duplicate key after normalization, merge the configs
                log::debug!(
                    "Merging duplicate player config for '{}' (from '{}')",
                    normalized_key,
                    key
                );

                let merged = super::PlayerConfig {
                    ignore: value.ignore,
                    app_id: if value.app_id != super::DEFAULT_PLAYER_APP_ID {
                        value.app_id
                    } else {
                        existing.app_id
                    },
                    icon: if value.icon != super::DEFAULT_PLAYER_ICON {
                        value.icon
                    } else {
                        existing.icon
                    },
                    show_icon: value.show_icon,
                    allow_streaming: value.allow_streaming,
                    override_activity_type: value
                        .override_activity_type
                        .or(existing.override_activity_type),
                };

                final_map.insert(normalized_key, merged);
            } else {
                log::debug!(
                    "Normalizing player config key from '{}' to '{}'",
                    key,
                    normalized_key
                );
                final_map.insert(normalized_key, value);
            }
        }

        Ok(final_map)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityTypesConfig {
    #[serde(default = "default_use_content_type")]
    pub use_content_type: bool,

    #[serde(default = "default_activity_type")]
    pub default: ActivityType,
}

fn default_use_content_type() -> bool {
    DEFAULT_USE_CONTENT_TYPE
}

fn default_activity_type() -> ActivityType {
    DEFAULT_ACTIVITY_TYPE
}

impl Default for ActivityTypesConfig {
    fn default() -> Self {
        Self {
            use_content_type: default_use_content_type(),
            default: default_activity_type(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_clear_on_pause")]
    pub clear_on_pause: bool,

    #[serde(default = "default_interval")]
    pub interval: u64,

    pub template: TemplateConfig,

    pub time: TimeConfig,

    pub cover: CoverConfig,

    pub activity_type: ActivityTypesConfig,

    #[serde(default)]
    #[serde(with = "normalized_string")]
    pub player: HashMap<String, PlayerConfig>,
}

fn default_clear_on_pause() -> bool {
    DEFAULT_CLEAR_ON_PAUSE
}

fn default_interval() -> u64 {
    DEFAULT_INTERVAL
}

impl Default for Config {
    fn default() -> Self {
        Config {
            clear_on_pause: default_clear_on_pause(),
            interval: default_interval(),
            template: TemplateConfig::default(),
            time: TimeConfig::default(),
            cover: CoverConfig::default(),
            activity_type: ActivityTypesConfig::default(),
            player: HashMap::default(),
        }
    }
}

impl Config {
    pub fn get_player_config(&self, identity: &str, player_bus_name: &str) -> PlayerConfig {
        let normalized_identity = normalize_player_identity(identity);
        let normalized_player_bus_name = normalize_player_identity(player_bus_name);

        self.get_player_config_normalized(&normalized_identity)
            .or_else(|| {
                if normalized_identity != normalized_player_bus_name {
                    self.get_player_config_normalized(&normalized_player_bus_name)
                } else {
                    None
                }
            })
            .unwrap_or_else(|| self.default_player_config())
    }

    fn get_player_config_normalized(&self, normalized_identity: &str) -> Option<PlayerConfig> {
        if let Some(config) = self.player.get(normalized_identity) {
            return Some(config.clone());
        }

        let mut best_match: Option<(usize, usize, PlayerConfig)> = None;
        for (pattern_key, cfg) in &self.player {
            if !is_wildcard_pattern(pattern_key) {
                continue;
            }

            if wildcard_match(pattern_key, normalized_identity) {
                let specificity = pattern_specificity(pattern_key);
                let total_len = pattern_key.len();
                match &best_match {
                    Some((best_spec, best_len, _)) => {
                        if specificity > *best_spec
                            || (specificity == *best_spec && total_len > *best_len)
                        {
                            best_match = Some((specificity, total_len, cfg.clone()));
                        }
                    }
                    None => best_match = Some((specificity, total_len, cfg.clone())),
                }
            }
        }

        best_match.map(|(_, _, cfg)| cfg)
    }

    fn default_player_config(&self) -> PlayerConfig {
        self.player.get("default").cloned().unwrap_or_else(|| {
            warn!("No default player config found, using built-in defaults");
            PlayerConfig::default()
        })
    }
}

fn is_wildcard_pattern(s: &str) -> bool {
    s.contains('*') || s.contains('?')
}

fn pattern_specificity(s: &str) -> usize {
    s.chars().filter(|&c| c != '*' && c != '?').count()
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    // Convert a simple glob-like pattern to a regex
    let mut regex_str = String::from("^");
    for ch in pattern.chars() {
        match ch {
            '*' => regex_str.push_str(".*"),
            '?' => regex_str.push('.'),
            _ => regex_str.push_str(&regex::escape(&ch.to_string())),
        }
    }
    regex_str.push('$');

    if let Ok(re) = Regex::new(&regex_str) {
        re.is_match(text)
    } else {
        false
    }
}

#[cfg(test)]
mod wildcard_tests {
    use super::*;

    fn pc(show_icon: bool, ignore: bool, app_id: &str) -> PlayerConfig {
        let mut cfg = PlayerConfig::default();
        cfg.show_icon = show_icon;
        cfg.ignore = ignore;
        cfg.app_id = app_id.to_string();
        cfg
    }

    #[test]
    fn matches_exact_before_wildcard() {
        let mut cfg = Config::default();
        cfg.player.insert("vlc*".to_string(), pc(true, false, "A"));
        cfg.player
            .insert("vlc_media_player".to_string(), pc(false, false, "B"));

        let res = cfg.get_player_config("VLC Media Player", "vlc");
        assert_eq!(res.app_id, "B");
        assert_eq!(res.show_icon, false);
    }

    #[test]
    fn chooses_more_specific_wildcard() {
        let mut cfg = Config::default();
        cfg.player.insert("vlc_*".to_string(), pc(true, false, "A"));
        cfg.player
            .insert("vlc_media_*".to_string(), pc(false, false, "B"));

        let res = cfg.get_player_config("vlc media classic", "vlc");
        assert_eq!(res.app_id, "B");
        assert_eq!(res.show_icon, false);
    }

    #[test]
    fn wildcard_only_then_default() {
        let mut cfg = Config::default();
        cfg.player
            .insert("*spotify*".to_string(), pc(true, true, "S"));
        cfg.player
            .insert("default".to_string(), pc(false, false, "D"));

        let sp = cfg.get_player_config("Spotify", "spotify");
        assert_eq!(sp.app_id, "S");
        assert!(sp.ignore);

        let other = cfg.get_player_config("Some Player", "other_player");
        assert_eq!(other.app_id, "D");
    }

    #[test]
    fn matches_player_bus_name_when_identity_differs() {
        let mut cfg = Config::default();
        cfg.player.insert("vlc".to_string(), pc(true, false, "A"));
        cfg.player
            .insert("default".to_string(), pc(false, false, "D"));

        let res = cfg.get_player_config("Fancy VLC", "vlc");
        assert_eq!(res.app_id, "A");
        assert_eq!(res.show_icon, true);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateConfig {
    #[serde(default = "default_template_detail")]
    pub detail: Box<str>,

    #[serde(default = "default_template_state")]
    pub state: Box<str>,

    #[serde(default = "default_template_large_text")]
    pub large_text: Box<str>,

    #[serde(default = "default_template_small_text")]
    pub small_text: Box<str>,
}

fn default_template_detail() -> Box<str> {
    DEFAULT_TEMPLATE_DETAIL.into()
}

fn default_template_state() -> Box<str> {
    DEFAULT_TEMPLATE_STATE.into()
}

fn default_template_large_text() -> Box<str> {
    DEFAULT_TEMPLATE_LARGE_TEXT.into()
}

fn default_template_small_text() -> Box<str> {
    DEFAULT_TEMPLATE_SMALL_TEXT.into()
}

impl Default for TemplateConfig {
    fn default() -> Self {
        TemplateConfig {
            detail: default_template_detail(),
            state: default_template_state(),
            large_text: default_template_large_text(),
            small_text: default_template_small_text(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeConfig {
    #[serde(default = "default_time_show")]
    pub show: bool,

    #[serde(default = "default_time_as_elapsed")]
    pub as_elapsed: bool,
}

fn default_time_show() -> bool {
    DEFAULT_TIME_SHOW
}

fn default_time_as_elapsed() -> bool {
    DEFAULT_TIME_AS_ELAPSED
}

impl Default for TimeConfig {
    fn default() -> Self {
        TimeConfig {
            show: default_time_show(),
            as_elapsed: default_time_as_elapsed(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverConfig {
    #[serde(default = "default_cover_file_names")]
    pub file_names: Vec<String>,

    #[serde(default)]
    pub provider: CoverProviderConfig,

    #[serde(default = "default_cover_local_search_depth")]
    pub local_search_depth: usize,
}

fn default_cover_file_names() -> Vec<String> {
    DEFAULT_COVER_FILE_NAMES
        .iter()
        .map(|&s| s.to_string())
        .collect()
}

fn default_cover_local_search_depth() -> usize {
    DEFAULT_COVER_LOCAL_SEARCH_DEPTH
}

impl Default for CoverConfig {
    fn default() -> Self {
        CoverConfig {
            file_names: default_cover_file_names(),
            provider: CoverProviderConfig::default(),
            local_search_depth: default_cover_local_search_depth(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicbrainzConfig {
    #[serde(default = "default_musicbrainz_min_score")]
    pub min_score: u8,
}

fn default_musicbrainz_min_score() -> u8 {
    DEFAULT_MUSICBRAINZ_MIN_SCORE
}

impl Default for MusicbrainzConfig {
    fn default() -> Self {
        Self {
            min_score: default_musicbrainz_min_score(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverProviderConfig {
    #[serde(default = "default_cover_providers")]
    pub provider: Vec<String>,

    #[serde(default)]
    pub imgbb: ImgBBConfig,

    #[serde(default)]
    pub musicbrainz: MusicbrainzConfig,

    #[serde(default)]
    pub catbox: CatboxConfig,
}

fn default_cover_providers() -> Vec<String> {
    DEFAULT_COVER_PROVIDERS
        .iter()
        .map(|&s| s.to_string())
        .collect()
}

impl Default for CoverProviderConfig {
    fn default() -> Self {
        CoverProviderConfig {
            provider: default_cover_providers(),
            imgbb: ImgBBConfig::default(),
            musicbrainz: MusicbrainzConfig::default(),
            catbox: CatboxConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImgBBConfig {
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_cover_imgbb_expiration")]
    pub expiration: u64,
}

fn default_cover_imgbb_expiration() -> u64 {
    DEFAULT_IMGBB_EXPIRATION
}

impl Default for ImgBBConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            expiration: default_cover_imgbb_expiration(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatboxConfig {
    #[serde(default)]
    pub user_hash: Option<String>,
    #[serde(default = "default_catbox_use_litter")]
    pub use_litter: bool,
    #[serde(default = "default_catbox_litter_hours")]
    pub litter_hours: u8,
}

fn default_catbox_use_litter() -> bool {
    DEFAULT_CATBOX_USE_LITTER
}

fn default_catbox_litter_hours() -> u8 {
    DEFAULT_CATBOX_LITTER_HOURS
}

impl Default for CatboxConfig {
    fn default() -> Self {
        Self {
            user_hash: None,
            use_litter: default_catbox_use_litter(),
            litter_hours: default_catbox_litter_hours(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Copy)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum ActivityType {
    #[default]
    Listening,
    Watching,
    Playing,
    Competing,
}

impl From<ActivityType> for discord_rich_presence::activity::ActivityType {
    fn from(activity_type: ActivityType) -> Self {
        match activity_type {
            ActivityType::Listening => Self::Listening,
            ActivityType::Watching => Self::Watching,
            ActivityType::Playing => Self::Playing,
            ActivityType::Competing => Self::Competing,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerConfig {
    #[serde(default = "default_player_ignore")]
    pub ignore: bool,

    #[serde(default = "default_player_app_id")]
    pub app_id: String,

    #[serde(default = "default_player_icon")]
    pub icon: String,

    #[serde(default = "default_player_show_icon")]
    pub show_icon: bool,

    #[serde(default = "default_player_allow_streaming")]
    pub allow_streaming: bool,

    #[serde(default)]
    pub override_activity_type: Option<ActivityType>,
}

fn default_player_ignore() -> bool {
    DEFAULT_PLAYER_IGNORE
}

fn default_player_app_id() -> String {
    DEFAULT_PLAYER_APP_ID.to_string()
}

fn default_player_icon() -> String {
    DEFAULT_PLAYER_ICON.to_string()
}

fn default_player_show_icon() -> bool {
    DEFAULT_PLAYER_SHOW_ICON
}

fn default_player_allow_streaming() -> bool {
    DEFAULT_PLAYER_ALLOW_STREAMING
}

impl Default for PlayerConfig {
    fn default() -> PlayerConfig {
        PlayerConfig {
            ignore: default_player_ignore(),
            app_id: default_player_app_id(),
            icon: default_player_icon(),
            show_icon: default_player_show_icon(),
            allow_streaming: default_player_allow_streaming(),
            override_activity_type: None,
        }
    }
}
