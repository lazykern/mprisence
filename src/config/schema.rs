use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use url::Url;

use crate::utils::normalize_player_identity;

pub const DEFAULT_INTERVAL: u64 = 2000;
pub const DEFAULT_EVENT_DRIVEN: bool = true;
pub const DEFAULT_DISCOVERY_INTERVAL: u64 = 5000;
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
const DEFAULT_TEMPLATE_SMALL_TEXT: &str = "{{{player}}}";

const DEFAULT_COVER_FILE_NAMES: [&str; 5] = ["cover", "folder", "front", "album", "art"];
const DEFAULT_COVER_PROVIDERS: [&str; 2] = ["catbox", "musicbrainz"];
const DEFAULT_COVER_LOCAL_SEARCH_DEPTH: usize = 2;
const DEFAULT_MUSICBRAINZ_MIN_SCORE: u8 = 95;
const DEFAULT_CATBOX_USE_LITTER: bool = true;
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

pub(crate) mod normalized_website_string {
    use crate::utils::normalize_player_identity;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::collections::HashMap;

    pub fn serialize<S>(
        map: &HashMap<String, super::WebsiteConfigLayer>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        map.serialize(serializer)
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<HashMap<String, super::WebsiteConfigLayer>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let temp_map = HashMap::<String, super::WebsiteConfigLayer>::deserialize(deserializer)?;

        let mut final_map: HashMap<String, super::WebsiteConfigLayer> = HashMap::new();

        for (key, value) in temp_map {
            let normalized_key = normalize_player_identity(&key);

            if let Some(existing) = final_map.get_mut(&normalized_key) {
                log::debug!(
                    "Merging duplicate website config for '{}' (from '{}')",
                    normalized_key,
                    key
                );
                existing.merge_from(value);
            } else {
                log::debug!(
                    "Normalizing website config key from '{}' to '{}'",
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
    #[serde(default = "default_interval")]
    pub interval: u64,

    #[serde(default = "default_event_driven")]
    pub event_driven: bool,

    #[serde(default = "default_discovery_interval")]
    pub discovery_interval: u64,

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

    #[serde(default)]
    #[serde(with = "normalized_website_string")]
    pub website: HashMap<String, WebsiteConfigLayer>,

    #[serde(skip)]
    pub bundled_website: HashMap<String, WebsiteConfigLayer>,

    #[serde(skip)]
    pub user_website: HashMap<String, WebsiteConfigLayer>,
}

fn default_interval() -> u64 {
    DEFAULT_INTERVAL
}

fn default_event_driven() -> bool {
    DEFAULT_EVENT_DRIVEN
}

fn default_discovery_interval() -> u64 {
    DEFAULT_DISCOVERY_INTERVAL
}

fn default_allowed_players() -> Vec<String> {
    Vec::new()
}

impl Default for Config {
    fn default() -> Self {
        Config {
            interval: default_interval(),
            event_driven: default_event_driven(),
            discovery_interval: default_discovery_interval(),
            allowed_players: default_allowed_players(),
            template: TemplateConfig::default(),
            time: TimeConfig::default(),
            cover: CoverConfig::default(),
            activity_type: ActivityTypesConfig::default(),
            player: HashMap::default(),
            bundled_player: HashMap::default(),
            user_player: HashMap::default(),
            user_player_patterns: HashSet::new(),
            website: HashMap::default(),
            bundled_website: HashMap::default(),
            user_website: HashMap::default(),
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

        let user_exact_bus_match = if normalized_identity != normalized_player_bus_name {
            self.user_player.get(&normalized_player_bus_name).cloned()
        } else {
            None
        };

        if normalized_identity != normalized_player_bus_name {
            matches.extend(self.collect_ordered_matches(&normalized_player_bus_name));
        }

        matches.extend(self.collect_ordered_matches(&normalized_identity));

        if let Some(layer) = user_exact_bus_match {
            matches.push(layer);
        }

        self.resolve_player_config(matches)
    }

    /// Like `get_player_config` but additionally overlays any matching
    /// `[website.*]` layers on top when the current track's URL matches.
    pub fn get_player_config_with_url(
        &self,
        identity: &str,
        player_bus_name: &str,
        url: Option<&str>,
    ) -> PlayerConfig {
        let base = self.get_player_config(identity, player_bus_name);
        self.apply_website_overrides(base, url)
    }

    fn apply_website_overrides(&self, mut base: PlayerConfig, url: Option<&str>) -> PlayerConfig {
        let Some(raw_url) = url else {
            return base;
        };
        if raw_url.is_empty() {
            return base;
        }
        let host_or_url = url_host_for_match(raw_url);
        let matches = self.collect_matching_website_layers(&host_or_url);
        let had_match = !matches.is_empty();
        for layer in matches {
            base = layer.apply_over(base);
        }

        // Unknown web URL (http/https with no matching website override):
        // ignore by default so random browser audio doesn't leak into Discord.
        // Users opt-in by adding a `[website.*]` entry that matches the host.
        if !had_match && is_http_url(raw_url) {
            base.ignore = true;
        }

        base
    }

    fn collect_matching_website_layers(&self, url_host: &str) -> Vec<WebsiteConfigLayer> {
        let mut layers = Vec::new();
        if let Some(layer) = find_matching_website_layer(&self.bundled_website, url_host) {
            layers.push(layer);
        }
        if let Some(layer) = find_matching_website_layer(&self.user_website, url_host) {
            layers.push(layer);
        }
        layers
    }

    /// Resolved (non-Layer) view of every configured website, used by
    /// `mprisence config websites` and for inspection.
    pub fn effective_website_configs(&self) -> HashMap<String, WebsiteConfig> {
        let mut keys: HashSet<String> = HashSet::new();
        for key in self.bundled_website.keys().chain(self.user_website.keys()) {
            keys.insert(key.clone());
        }

        let mut result = HashMap::new();
        for key in keys {
            let mut resolved = WebsiteConfig::default();
            if let Some(layer) = self.bundled_website.get(&key) {
                resolved = layer.apply_into_website(resolved);
            }
            if let Some(layer) = self.user_website.get(&key) {
                resolved = layer.apply_into_website(resolved);
            }
            result.insert(key, resolved);
        }
        result
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

    let raw = if let Some(stripped) = pattern.strip_prefix("re:") {
        stripped.to_string()
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
        assert!(!res.show_icon);
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
        assert!(!res.show_icon);
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
        assert!(res.show_icon);
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
        assert!(res.show_icon);
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
        assert!(res.show_icon);
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
        assert!(res.show_icon);
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
        assert!(res.show_icon);
    }

    #[test]
    fn cover_defaults_prefer_catbox_with_litter() {
        let cfg = Config::default();
        assert_eq!(
            cfg.cover.provider.provider,
            vec!["catbox".to_string(), "musicbrainz".to_string()]
        );
        assert!(cfg.cover.provider.catbox.use_litter);
        assert_eq!(cfg.cover.provider.catbox.litter_hours, 24);
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
        assert!(res.show_icon); // overridden by user layer
        assert!(!res.ignore); // inherited from bundled + defaults
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
        assert!(res.show_icon); // overridden by user regex
        assert!(res.ignore); // inherited from bundled exact
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
    fn user_exact_bus_name_overrides_identity_matches() {
        let mut cfg = Config::default();
        cfg.user_player.insert(
            "mpv".to_string(),
            layer(Some(true), Some(false), Some("IDENTITY")),
        );
        cfg.user_player.insert(
            "playerctld".to_string(),
            layer(Some(false), Some(true), Some("BUS")),
        );

        let res = cfg.get_player_config("mpv", "playerctld");
        assert_eq!(res.app_id, "BUS");
        assert!(res.ignore);
        assert!(!res.show_icon);
    }

    #[test]
    fn allows_all_players_when_unset() {
        let cfg = Config::default();

        assert!(cfg.is_player_allowed("Any Player", "any_player"));
    }

    #[test]
    fn filters_players_by_allowed_patterns() {
        let cfg = Config {
            allowed_players: vec![
                "vlc_media_player".to_string(),
                "*mpd*".to_string(),
                "re:.*youtube_music.*".to_string(),
            ],
            ..Default::default()
        };

        assert!(cfg.is_player_allowed("VLC media player", "vlc"));
        assert!(cfg.is_player_allowed("Music Player Daemon (mpdris2-rs)", "mpd"));
        assert!(cfg.is_player_allowed("YouTube Music", "youtube-music"));
        assert!(!cfg.is_player_allowed("spotify", "spotify"));
    }

    #[test]
    fn template_details_key_is_supported() {
        let template: TemplateConfig = toml::from_str(
            r#"
details = "new details"
"#,
        )
        .expect("template.details should deserialize");

        assert_eq!(template.details.as_ref(), "new details");
    }

    #[test]
    fn template_detail_key_is_still_supported() {
        let template: TemplateConfig = toml::from_str(
            r#"
detail = "legacy detail"
"#,
        )
        .expect("template.detail should deserialize for backward compatibility");

        assert_eq!(template.details.as_ref(), "legacy detail");
    }

    #[test]
    fn template_details_takes_precedence_when_both_keys_exist() {
        let template: TemplateConfig = toml::from_str(
            r#"
detail = "legacy detail"
details = "new details"
"#,
        )
        .expect("template should deserialize when both keys are present");

        assert_eq!(template.details.as_ref(), "new details");
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TemplateConfig {
    pub details: Box<str>,

    #[serde(default = "default_template_state")]
    pub state: Box<str>,

    #[serde(default = "default_template_large_text")]
    pub large_text: Box<str>,

    #[serde(default = "default_template_small_text")]
    pub small_text: Box<str>,
}

fn default_template_details() -> Box<str> {
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
            details: default_template_details(),
            state: default_template_state(),
            large_text: default_template_large_text(),
            small_text: default_template_small_text(),
        }
    }
}

impl<'de> Deserialize<'de> for TemplateConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct TemplateConfigRaw {
            #[serde(default)]
            details: Option<Box<str>>,
            #[serde(default)]
            detail: Option<Box<str>>,
            #[serde(default = "default_template_state")]
            state: Box<str>,
            #[serde(default = "default_template_large_text")]
            large_text: Box<str>,
            #[serde(default = "default_template_small_text")]
            small_text: Box<str>,
        }

        let raw = TemplateConfigRaw::deserialize(deserializer)?;
        Ok(TemplateConfig {
            details: raw
                .details
                .or(raw.detail)
                .unwrap_or_else(default_template_details),
            state: raw.state,
            large_text: raw.large_text,
            small_text: raw.small_text,
        })
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
    pub name: Option<String>,

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
        if let Some(value) = &self.name {
            base.name = Some(value.clone());
        }
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
        if other.name.is_some() {
            self.name = other.name;
        }
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
    #[serde(default)]
    pub name: Option<String>,

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
            name: None,
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

/// Site-specific override applied on top of the resolved `PlayerConfig`
/// whenever `xesam:url` matches `match_pattern`. Mirrors
/// `PlayerConfigLayer` but adds the URL pattern.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebsiteConfigLayer {
    #[serde(default)]
    pub match_pattern: Option<String>,

    #[serde(default)]
    pub match_patterns: Option<Vec<String>>,

    #[serde(default)]
    pub name: Option<String>,

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

impl WebsiteConfigLayer {
    /// Combined view of `match_pattern` (singular) and `match_patterns`
    /// (plural) entries from the TOML, in declared order.
    pub fn effective_patterns(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        if let Some(p) = self.match_pattern.as_deref() {
            if !p.is_empty() {
                out.push(p);
            }
        }
        if let Some(ps) = self.match_patterns.as_deref() {
            for p in ps {
                if !p.is_empty() {
                    out.push(p.as_str());
                }
            }
        }
        out
    }

    pub fn apply_over(&self, mut base: PlayerConfig) -> PlayerConfig {
        if let Some(value) = &self.name {
            base.name = Some(value.clone());
        }
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

    pub fn merge_from(&mut self, other: WebsiteConfigLayer) {
        if other.match_pattern.is_some() {
            self.match_pattern = other.match_pattern;
        }
        if other.match_patterns.is_some() {
            self.match_patterns = other.match_patterns;
        }
        if other.name.is_some() {
            self.name = other.name;
        }
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

    fn apply_into_website(&self, mut base: WebsiteConfig) -> WebsiteConfig {
        let patterns = self.effective_patterns();
        if !patterns.is_empty() {
            base.match_patterns = patterns.into_iter().map(|s| s.to_string()).collect();
        }
        if let Some(value) = &self.name {
            base.name = Some(value.clone());
        }
        if let Some(value) = self.ignore {
            base.ignore = value;
        }
        if let Some(value) = &self.app_id {
            base.app_id = Some(value.clone());
        }
        if let Some(value) = &self.icon {
            base.icon = Some(value.clone());
        }
        if let Some(value) = self.show_icon {
            base.show_icon = Some(value);
        }
        if let Some(value) = self.allow_streaming {
            base.allow_streaming = Some(value);
        }
        if let Some(value) = self.status_display_type {
            base.status_display_type = Some(value);
        }
        if let Some(value) = self.override_activity_type {
            base.override_activity_type = Some(value);
        }
        base
    }
}

/// Resolved, inspectable form of a website entry (used by CLI listing).
/// Unlike `PlayerConfig` we keep fields optional because website entries
/// only override what they explicitly set.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebsiteConfig {
    #[serde(default)]
    pub match_patterns: Vec<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub ignore: bool,
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

fn url_host_for_match(url: &str) -> String {
    if let Ok(parsed) = Url::parse(url) {
        if let Some(host) = parsed.host_str() {
            return host.to_string();
        }
    }
    url.to_string()
}

fn is_http_url(url: &str) -> bool {
    Url::parse(url)
        .map(|p| matches!(p.scheme(), "http" | "https"))
        .unwrap_or(false)
}

#[cfg(test)]
mod website_tests {
    use super::*;

    fn website(match_pattern: &str, app_id: Option<&str>) -> WebsiteConfigLayer {
        WebsiteConfigLayer {
            match_pattern: Some(match_pattern.to_string()),
            app_id: app_id.map(|s| s.to_string()),
            allow_streaming: Some(true),
            ..Default::default()
        }
    }

    #[test]
    fn website_layer_applies_over_player_config() {
        let base = PlayerConfig {
            app_id: "PLAYER".into(),
            icon: "player-icon".into(),
            allow_streaming: false,
            ..PlayerConfig::default()
        };

        let layer = WebsiteConfigLayer {
            match_pattern: Some("music.youtube.com".to_string()),
            app_id: Some("WEBSITE".into()),
            icon: Some("yt-icon".into()),
            allow_streaming: Some(true),
            ..Default::default()
        };

        let result = layer.apply_over(base);
        assert_eq!(result.app_id, "WEBSITE");
        assert_eq!(result.icon, "yt-icon");
        assert!(result.allow_streaming);
        // Untouched fields preserved.
        assert!(!result.ignore);
    }

    #[test]
    fn website_match_host_swaps_app_id() {
        let mut cfg = Config::default();
        cfg.bundled_website
            .insert("youtube_music".into(), website("music.youtube.com", Some("YT")));

        let resolved = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://music.youtube.com/watch?v=abc"),
        );
        assert_eq!(resolved.app_id, "YT");
        assert!(resolved.allow_streaming);
    }

    #[test]
    fn website_match_patterns_plural_any_entry_matches() {
        let mut cfg = Config::default();
        cfg.bundled_website.insert(
            "soundcloud".into(),
            WebsiteConfigLayer {
                match_patterns: Some(vec!["soundcloud.com".into(), "snd.sc".into()]),
                app_id: Some("SC".into()),
                allow_streaming: Some(true),
                ..Default::default()
            },
        );

        let resolved_long = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://soundcloud.com/discover/sets/x"),
        );
        assert_eq!(resolved_long.app_id, "SC");
        assert!(resolved_long.allow_streaming);

        let resolved_short = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://snd.sc/abc"),
        );
        assert_eq!(resolved_short.app_id, "SC");
        assert!(resolved_short.allow_streaming);
    }

    #[test]
    fn website_match_regex_on_host() {
        let mut cfg = Config::default();
        cfg.bundled_website.insert(
            "bandcamp".into(),
            website("re:.*\\.bandcamp\\.com$", Some("BC")),
        );

        let resolved = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://artist.bandcamp.com/track/y"),
        );
        assert_eq!(resolved.app_id, "BC");
    }

    #[test]
    fn website_unknown_http_url_forces_ignore() {
        let mut cfg = Config::default();
        cfg.bundled_website
            .insert("youtube_music".into(), website("music.youtube.com", Some("YT")));

        let resolved = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://github.com/lazykern/mprisence"),
        );
        assert!(
            resolved.ignore,
            "unknown http URL should auto-ignore so random browser audio stays hidden"
        );
    }

    #[test]
    fn website_non_http_scheme_falls_through_to_base() {
        let mut cfg = Config::default();
        cfg.bundled_website
            .insert("youtube_music".into(), website("music.youtube.com", Some("YT")));
        let baseline = cfg.get_player_config("Spotify", "spotify");

        let resolved = cfg.get_player_config_with_url(
            "Spotify",
            "spotify",
            Some("spotify:track:abc123"),
        );
        assert_eq!(resolved.app_id, baseline.app_id);
        assert_eq!(resolved.ignore, baseline.ignore);
    }

    #[test]
    fn website_file_url_falls_through_to_base() {
        let mut cfg = Config::default();
        cfg.bundled_website
            .insert("youtube_music".into(), website("music.youtube.com", Some("YT")));
        let baseline = cfg.get_player_config("VLC", "vlc");

        let resolved = cfg.get_player_config_with_url(
            "VLC",
            "vlc",
            Some("file:///home/user/track.flac"),
        );
        assert_eq!(resolved.ignore, baseline.ignore);
    }

    #[test]
    fn website_no_url_returns_base_player_config() {
        let mut cfg = Config::default();
        cfg.bundled_website
            .insert("youtube_music".into(), website("music.youtube.com", Some("YT")));
        let baseline = cfg.get_player_config("Firefox", "firefox");

        let resolved = cfg.get_player_config_with_url("Firefox", "firefox", None);
        assert_eq!(resolved.app_id, baseline.app_id);
    }

    #[test]
    fn website_user_overrides_bundled() {
        let mut cfg = Config::default();
        cfg.bundled_website
            .insert("youtube_music".into(), website("music.youtube.com", Some("BUNDLED")));
        cfg.user_website
            .insert("youtube_music".into(), website("music.youtube.com", Some("USER")));

        let resolved = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://music.youtube.com/watch?v=x"),
        );
        assert_eq!(resolved.app_id, "USER");
    }

    #[test]
    fn website_ignore_propagates_to_resolved_player_config() {
        let mut cfg = Config::default();
        cfg.bundled_website.insert(
            "spotify_web".into(),
            WebsiteConfigLayer {
                match_pattern: Some("open.spotify.com".into()),
                ignore: Some(true),
                ..Default::default()
            },
        );

        let resolved = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://open.spotify.com/track/abc"),
        );
        assert!(resolved.ignore);
    }

    #[test]
    fn website_pattern_more_specific_than_substring_wins() {
        let mut cfg = Config::default();
        // Both patterns would match the URL; exact host should win over substring.
        cfg.bundled_website
            .insert("youtube_dot_com".into(), website("youtube.com", Some("GENERIC")));
        cfg.bundled_website
            .insert("youtube_music".into(), website("music.youtube.com", Some("SPECIFIC")));

        let resolved = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://music.youtube.com/watch?v=x"),
        );
        assert_eq!(resolved.app_id, "SPECIFIC");
    }
}

/// Picks the most specific matching website layer from a single source map.
/// Priority: exact host > regex > wildcard > plain substring fallback.
fn find_matching_website_layer(
    source: &HashMap<String, WebsiteConfigLayer>,
    url_host: &str,
) -> Option<WebsiteConfigLayer> {
    let mut best: Option<(WebsiteConfigLayer, (u8, usize))> = None;

    for layer in source.values() {
        let patterns = layer.effective_patterns();
        if patterns.is_empty() {
            continue;
        }

        for pattern in patterns {
            let score: Option<(u8, usize)> = if pattern == url_host {
                Some((3, pattern.len()))
            } else if let Some(re) = regex_from_pattern(pattern) {
                re.is_match(url_host).then_some((2, pattern.len()))
            } else if is_wildcard_pattern(pattern) {
                wildcard_match(pattern, url_host).then(|| (1, pattern_specificity(pattern)))
            } else if url_host.contains(pattern) {
                Some((0, pattern.len()))
            } else {
                None
            };

            let Some(score) = score else { continue };

            match &best {
                Some((_, current)) if *current >= score => {}
                _ => best = Some((layer.clone(), score)),
            }
        }
    }

    best.map(|(layer, _)| layer)
}
