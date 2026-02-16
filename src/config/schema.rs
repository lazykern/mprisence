use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

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
pub const DEFAULT_PLAYER_STATUS_DISPLAY_TYPE: StatusDisplayType = StatusDisplayType::Name;

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

pub(crate) mod normalized_string {
    use crate::utils::normalize_player_identity;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::HashMap;

    pub fn serialize<S>(
        map: &HashMap<String, super::PlayerConfigLayer>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        map.serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<HashMap<String, super::PlayerConfigLayer>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let temp_map = HashMap::<String, super::PlayerConfigLayer>::deserialize(deserializer)?;

        let mut final_map: HashMap<String, super::PlayerConfigLayer> = HashMap::new();

        for (key, value) in temp_map {
            let normalized_key = normalize_player_identity(&key);

            if let Some(existing) = final_map.get_mut(&normalized_key) {
                // If we have a duplicate key after normalization, merge the configs
                log::debug!(
                    "Merging duplicate player config for '{}' (from '{}')",
                    normalized_key,
                    key
                );

                existing.merge_from(value);
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

    #[serde(default = "default_allowed_players")]
    pub allowed_players: Vec<String>,

    pub template: TemplateConfig,

    pub time: TimeConfig,

    pub cover: CoverConfig,

    pub activity_type: ActivityTypesConfig,

    #[serde(default)]
    #[serde(with = "normalized_string")]
    pub player: HashMap<String, PlayerConfigLayer>,

    #[serde(skip)]
    pub bundled_player: HashMap<String, PlayerConfigLayer>,

    #[serde(skip)]
    pub user_player: HashMap<String, PlayerConfigLayer>,

    #[serde(skip)]
    pub user_player_patterns: HashSet<String>,
}

fn default_clear_on_pause() -> bool {
    DEFAULT_CLEAR_ON_PAUSE
}

fn default_interval() -> u64 {
    DEFAULT_INTERVAL
}

fn default_allowed_players() -> Vec<String> {
    Vec::new()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            clear_on_pause: default_clear_on_pause(),
            interval: default_interval(),
            allowed_players: default_allowed_players(),
            template: TemplateConfig::default(),
            time: TimeConfig::default(),
            cover: CoverConfig::default(),
            activity_type: ActivityTypesConfig::default(),
            player: HashMap::default(),
            bundled_player: HashMap::default(),
            user_player: HashMap::default(),
            user_player_patterns: HashSet::new(),
        }
    }
}

impl Config {
    pub fn is_player_allowed(&self, identity: &str, player_bus_name: &str) -> bool {
        if self.allowed_players.is_empty() {
            return true;
        }

        let normalized_identity = normalize_player_identity(identity);
        let normalized_player_bus_name = normalize_player_identity(player_bus_name);

        self.allowed_players.iter().any(|pattern| {
            let normalized_pattern = normalize_player_identity(pattern);
            matches_player_pattern(&normalized_pattern, &normalized_identity)
                || matches_player_pattern(&normalized_pattern, &normalized_player_bus_name)
        })
    }

    pub fn get_player_config(&self, identity: &str, player_bus_name: &str) -> PlayerConfig {
        let normalized_identity = normalize_player_identity(identity);
        let normalized_player_bus_name = normalize_player_identity(player_bus_name);

        let mut matches = Vec::new();

        if normalized_identity != normalized_player_bus_name {
            matches.extend(self.collect_ordered_matches(&normalized_player_bus_name));
        }

        matches.extend(self.collect_ordered_matches(&normalized_identity));

        self.resolve_player_config(matches)
    }

    pub fn effective_player_configs(&self) -> HashMap<String, PlayerConfig> {
        let mut keys: HashSet<String> = HashSet::new();
        for key in self.bundled_player.keys().chain(self.user_player.keys()) {
            if key != "default" {
                keys.insert(key.clone());
            }
        }

        let mut result = HashMap::new();
        for key in keys {
            let mut resolved = PlayerConfig::default();

            if let Some(layer) = self.bundled_player.get("default") {
                resolved = layer.apply_over(resolved);
            }
            if let Some(layer) = self.user_player.get("default") {
                resolved = layer.apply_over(resolved);
            }
            if let Some(layer) = self.bundled_player.get(&key) {
                resolved = layer.apply_over(resolved);
            }
            if let Some(layer) = self.user_player.get(&key) {
                resolved = layer.apply_over(resolved);
            }

            result.insert(key, resolved);
        }

        result
    }

    fn resolve_player_config(&self, matches: Vec<PlayerConfigLayer>) -> PlayerConfig {
        let mut resolved = PlayerConfig::default();

        if let Some(layer) = self.bundled_player.get("default") {
            resolved = layer.apply_over(resolved);
        }

        if let Some(layer) = self.user_player.get("default") {
            resolved = layer.apply_over(resolved);
        }

        for layer in matches {
            resolved = layer.apply_over(resolved);
        }

        resolved
    }

    fn collect_ordered_matches(&self, normalized_identity: &str) -> Vec<PlayerConfigLayer> {
        let user_matches =
            self.collect_best_matches_for_source(&self.user_player, normalized_identity);
        let bundled_matches =
            self.collect_best_matches_for_source(&self.bundled_player, normalized_identity);

        // Order from lowest priority to highest so later items override earlier ones during overlay.
        let mut ordered: Vec<PlayerConfigLayer> = Vec::new();

        if let Some(layer) = bundled_matches.wildcard {
            ordered.push(layer);
        }
        if let Some(layer) = bundled_matches.regex {
            ordered.push(layer);
        }
        if let Some(layer) = bundled_matches.exact {
            ordered.push(layer);
        }
        if let Some(layer) = user_matches.wildcard {
            ordered.push(layer);
        }
        if let Some(layer) = user_matches.regex {
            ordered.push(layer);
        }
        if let Some(layer) = user_matches.exact {
            ordered.push(layer);
        }

        ordered
    }

    fn collect_best_matches_for_source(
        &self,
        source: &HashMap<String, PlayerConfigLayer>,
        normalized_identity: &str,
    ) -> MatchGroupLayers {
        let mut result = MatchGroup::default();

        for (pattern_key, cfg) in source {
            if pattern_key == "default" {
                continue;
            }

            if pattern_key == normalized_identity {
                result.exact = Some(ScoredLayer::new(cfg.clone(), pattern_key.len(), 0));
                continue;
            }

            if let Some(re) = regex_from_pattern(pattern_key) {
                if re.is_match(normalized_identity) {
                    let total_len = pattern_key.len();
                    match &result.regex {
                        Some(existing) if existing.pattern_len >= total_len => {}
                        _ => result.regex = Some(ScoredLayer::new(cfg.clone(), total_len, 0)),
                    }
                }
                continue;
            }

            if is_wildcard_pattern(pattern_key) && wildcard_match(pattern_key, normalized_identity)
            {
                let specificity = pattern_specificity(pattern_key);
                let total_len = pattern_key.len();
                match &result.wildcard {
                    Some(existing)
                        if existing.specificity > specificity
                            || (existing.specificity == specificity
                                && existing.pattern_len >= total_len) => {}
                    _ => {
                        result.wildcard =
                            Some(ScoredLayer::new(cfg.clone(), total_len, specificity))
                    }
                }
            }
        }

        result.into_layers()
    }
}

#[derive(Default)]
struct MatchGroup {
    exact: Option<ScoredLayer>,
    regex: Option<ScoredLayer>,
    wildcard: Option<ScoredLayer>,
}

impl MatchGroup {
    fn into_layers(self) -> MatchGroupLayers {
        MatchGroupLayers {
            exact: self.exact.map(|s| s.layer),
            regex: self.regex.map(|s| s.layer),
            wildcard: self.wildcard.map(|s| s.layer),
        }
    }
}

#[derive(Default)]
struct MatchGroupLayers {
    exact: Option<PlayerConfigLayer>,
    regex: Option<PlayerConfigLayer>,
    wildcard: Option<PlayerConfigLayer>,
}

#[derive(Clone)]
struct ScoredLayer {
    layer: PlayerConfigLayer,
    pattern_len: usize,
    specificity: usize,
}

impl ScoredLayer {
    fn new(layer: PlayerConfigLayer, pattern_len: usize, specificity: usize) -> Self {
        Self {
            layer,
            pattern_len,
            specificity,
        }
    }
}

fn matches_player_pattern(pattern: &str, normalized_identity: &str) -> bool {
    if pattern == normalized_identity {
        return true;
    }

    if let Some(re) = regex_from_pattern(pattern) {
        if re.is_match(normalized_identity) {
            return true;
        }
    }

    if is_wildcard_pattern(pattern) {
        return wildcard_match(pattern, normalized_identity);
    }

    false
}

fn is_wildcard_pattern(s: &str) -> bool {
    !is_regex_pattern(s) && (s.contains('*') || s.contains('?'))
}

fn is_regex_pattern(s: &str) -> bool {
    (s.starts_with("re:") && s.len() > 3) || (s.starts_with('/') && s.ends_with('/') && s.len() > 2)
}

fn regex_from_pattern(pattern: &str) -> Option<Regex> {
    if !is_regex_pattern(pattern) {
        return None;
    }

    let raw = if pattern.starts_with("re:") {
        pattern[3..].to_string()
    } else {
        pattern
            .trim_start_matches('/')
            .trim_end_matches('/')
            .to_string()
    };

    match Regex::new(&raw) {
        Ok(regex) => Some(regex),
        Err(err) => {
            log::warn!("Invalid regex pattern '{}': {}", pattern, err);
            None
        }
    }
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

    fn layer(
        show_icon: Option<bool>,
        ignore: Option<bool>,
        app_id: Option<&str>,
    ) -> PlayerConfigLayer {
        PlayerConfigLayer {
            show_icon,
            ignore,
            app_id: app_id.map(|s| s.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn matches_exact_before_wildcard() {
        let mut cfg = Config::default();
        cfg.user_player.insert(
            "vlc*".to_string(),
            layer(Some(true), Some(false), Some("A")),
        );
        cfg.user_player.insert(
            "vlc_media_player".to_string(),
            layer(Some(false), Some(false), Some("B")),
        );

        let res = cfg.get_player_config("VLC Media Player", "vlc");
        assert_eq!(res.app_id, "B");
        assert_eq!(res.show_icon, false);
    }

    #[test]
    fn chooses_more_specific_wildcard() {
        let mut cfg = Config::default();
        cfg.user_player.insert(
            "vlc_*".to_string(),
            layer(Some(true), Some(false), Some("A")),
        );
        cfg.user_player.insert(
            "vlc_media_*".to_string(),
            layer(Some(false), Some(false), Some("B")),
        );

        let res = cfg.get_player_config("vlc media classic", "vlc");
        assert_eq!(res.app_id, "B");
        assert_eq!(res.show_icon, false);
    }

    #[test]
    fn wildcard_only_then_default() {
        let mut cfg = Config::default();
        cfg.user_player.insert(
            "*spotify*".to_string(),
            layer(Some(true), Some(true), Some("S")),
        );
        cfg.bundled_player.insert(
            "default".to_string(),
            layer(Some(false), Some(false), Some("D")),
        );

        let sp = cfg.get_player_config("Spotify", "spotify");
        assert_eq!(sp.app_id, "S");
        assert!(sp.ignore);

        let other = cfg.get_player_config("Some Player", "other_player");
        assert_eq!(other.app_id, "D");
    }

    #[test]
    fn matches_player_bus_name_when_identity_differs() {
        let mut cfg = Config::default();
        cfg.user_player
            .insert("vlc".to_string(), layer(Some(true), Some(false), Some("A")));
        cfg.bundled_player.insert(
            "default".to_string(),
            layer(Some(false), Some(false), Some("D")),
        );

        let res = cfg.get_player_config("Fancy VLC", "vlc");
        assert_eq!(res.app_id, "A");
        assert_eq!(res.show_icon, true);
    }

    #[test]
    fn matches_regex_pattern_for_identity() {
        let mut cfg = Config::default();
        cfg.user_player.insert(
            "re:.*mpdris2.*".to_string(),
            layer(Some(true), Some(false), Some("R")),
        );
        cfg.bundled_player.insert(
            "default".to_string(),
            layer(Some(false), Some(false), Some("D")),
        );

        let res = cfg.get_player_config("Music Player Daemon (mpdris2-rs)", "mpd");
        assert_eq!(res.app_id, "R");
        assert_eq!(res.show_icon, true);
    }

    #[test]
    fn regex_priority_over_wildcard() {
        let mut cfg = Config::default();
        cfg.bundled_player.insert(
            "*mpd*".to_string(),
            layer(Some(false), Some(false), Some("G")),
        );
        cfg.user_player.insert(
            "re:.*mpdris2.*".to_string(),
            layer(Some(true), Some(false), Some("R")),
        );
        cfg.bundled_player.insert(
            "default".to_string(),
            layer(Some(false), Some(false), Some("D")),
        );

        let res = cfg.get_player_config("Music Player Daemon (mpdris2-rs)", "mpd");
        assert_eq!(res.app_id, "R");
        assert_eq!(res.show_icon, true);
    }

    #[test]
    fn regex_matches_bus_name_when_identity_differs() {
        let mut cfg = Config::default();
        cfg.user_player.insert(
            "re:.*mpdris2.*".to_string(),
            layer(Some(true), Some(false), Some("R")),
        );
        cfg.bundled_player.insert(
            "default".to_string(),
            layer(Some(false), Some(false), Some("D")),
        );

        let res = cfg.get_player_config("Some Custom Player", "mpdris2-rs");
        assert_eq!(res.app_id, "R");
        assert_eq!(res.show_icon, true);
    }

    #[test]
    fn user_patterns_override_defaults() {
        let mut cfg = Config::default();
        cfg.bundled_player.insert(
            "re:.*mpdris2.*".to_string(),
            layer(Some(false), Some(false), Some("D")),
        );
        cfg.user_player.insert(
            "*mpd*".to_string(),
            layer(Some(true), Some(false), Some("U")),
        );

        let res = cfg.get_player_config("Music Player Daemon (mpdris2-rs)", "mpd");
        assert_eq!(res.app_id, "U");
        assert_eq!(res.show_icon, true);
    }

    #[test]
    fn user_layers_fill_missing_fields_from_bundled_match() {
        let mut cfg = Config::default();
        cfg.bundled_player.insert(
            "vlc".to_string(),
            layer(Some(false), Some(false), Some("BUNDLED")),
        );
        cfg.user_player.insert(
            "vlc".to_string(),
            PlayerConfigLayer {
                show_icon: Some(true),
                ..Default::default()
            },
        );

        let res = cfg.get_player_config("vlc", "vlc");
        assert_eq!(res.app_id, "BUNDLED"); // comes from bundled match
        assert_eq!(res.show_icon, true); // overridden by user layer
        assert_eq!(res.ignore, false); // inherited from bundled + defaults
    }

    #[test]
    fn user_regex_overrides_bundled_exact_and_inherits_fields() {
        let mut cfg = Config::default();
        cfg.bundled_player.insert(
            "vlc_media_player".to_string(),
            layer(Some(false), Some(true), Some("BUNDLED")),
        );
        cfg.user_player.insert(
            "re:vlc.*".to_string(),
            PlayerConfigLayer {
                show_icon: Some(true),
                ..Default::default()
            },
        );

        let res = cfg.get_player_config("VLC media player", "vlc_media_player");
        assert_eq!(res.app_id, "BUNDLED"); // inherited
        assert_eq!(res.show_icon, true); // overridden by user regex
        assert_eq!(res.ignore, true); // inherited from bundled exact
    }

    #[test]
    fn bus_name_layers_apply_even_when_identity_matches() {
        let mut cfg = Config::default();
        cfg.bundled_player.insert(
            "mpd".to_string(),
            layer(Some(false), Some(false), Some("BUNDLED")),
        );
        cfg.user_player.insert(
            "*mpd*".to_string(),
            PlayerConfigLayer {
                show_icon: Some(true),
                ..Default::default()
            },
        );

        let res = cfg.get_player_config("Music Player Daemon (mpdris2-rs)", "mpd");
        assert_eq!(res.app_id, "BUNDLED"); // inherited from bus-name match
        assert!(res.show_icon); // overridden by identity wildcard
    }

    #[test]
    fn allows_all_players_when_unset() {
        let cfg = Config::default();

        assert!(cfg.is_player_allowed("Any Player", "any_player"));
    }

    #[test]
    fn filters_players_by_allowed_patterns() {
        let mut cfg = Config::default();
        cfg.allowed_players = vec![
            "vlc_media_player".to_string(),
            "*mpd*".to_string(),
            "re:.*youtube_music.*".to_string(),
        ];

        assert!(cfg.is_player_allowed("VLC media player", "vlc"));
        assert!(cfg.is_player_allowed("Music Player Daemon (mpdris2-rs)", "mpd"));
        assert!(cfg.is_player_allowed("YouTube Music", "youtube-music"));
        assert!(!cfg.is_player_allowed("spotify", "spotify"));
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Copy)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum StatusDisplayType {
    #[default]
    Name,
    State,
    Details,
}

impl From<StatusDisplayType> for discord_rich_presence::activity::StatusDisplayType {
    fn from(status_display_type: StatusDisplayType) -> Self {
        match status_display_type {
            StatusDisplayType::Name => Self::Name,
            StatusDisplayType::State => Self::State,
            StatusDisplayType::Details => Self::Details,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PlayerConfigLayer {
    #[serde(default)]
    pub ignore: Option<bool>,

    #[serde(default)]
    pub app_id: Option<String>,

    #[serde(default)]
    pub icon: Option<String>,

    #[serde(default)]
    pub show_icon: Option<bool>,

    #[serde(default)]
    pub allow_streaming: Option<bool>,

    #[serde(default)]
    pub status_display_type: Option<StatusDisplayType>,

    #[serde(default)]
    pub override_activity_type: Option<ActivityType>,
}

impl PlayerConfigLayer {
    pub fn apply_over(&self, mut base: PlayerConfig) -> PlayerConfig {
        if let Some(value) = self.ignore {
            base.ignore = value;
        }
        if let Some(value) = &self.app_id {
            base.app_id = value.clone();
        }
        if let Some(value) = &self.icon {
            base.icon = value.clone();
        }
        if let Some(value) = self.show_icon {
            base.show_icon = value;
        }
        if let Some(value) = self.allow_streaming {
            base.allow_streaming = value;
        }
        if let Some(value) = self.status_display_type {
            base.status_display_type = value;
        }
        if let Some(value) = self.override_activity_type {
            base.override_activity_type = Some(value);
        }

        base
    }

    pub fn merge_from(&mut self, other: PlayerConfigLayer) {
        if other.ignore.is_some() {
            self.ignore = other.ignore;
        }
        if other.app_id.is_some() {
            self.app_id = other.app_id;
        }
        if other.icon.is_some() {
            self.icon = other.icon;
        }
        if other.show_icon.is_some() {
            self.show_icon = other.show_icon;
        }
        if other.allow_streaming.is_some() {
            self.allow_streaming = other.allow_streaming;
        }
        if other.status_display_type.is_some() {
            self.status_display_type = other.status_display_type;
        }
        if other.override_activity_type.is_some() {
            self.override_activity_type = other.override_activity_type;
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

    #[serde(default = "default_player_status_display_type")]
    pub status_display_type: StatusDisplayType,

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

fn default_player_status_display_type() -> StatusDisplayType {
    DEFAULT_PLAYER_STATUS_DISPLAY_TYPE
}

impl Default for PlayerConfig {
    fn default() -> PlayerConfig {
        PlayerConfig {
            ignore: default_player_ignore(),
            app_id: default_player_app_id(),
            icon: default_player_icon(),
            show_icon: default_player_show_icon(),
            allow_streaming: default_player_allow_streaming(),
            status_display_type: default_player_status_display_type(),
            override_activity_type: None,
        }
    }
}
