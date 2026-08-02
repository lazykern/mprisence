use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use url::Url;

use crate::utils::normalize_player_identity;

/// Pre-compiled player/web_player pattern.  Built once at config load time so
/// repeated matching avoids per-call `Regex::new()` overhead.
#[derive(Debug, Clone)]
pub enum CompiledPattern {
    Exact(String),
    Wildcard(Regex),
    Regex(Regex),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerMatchTarget {
    Identity,
    Bus,
    Any,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlayerConfigMatch {
    pub config_key: String,
    pub target: PlayerMatchTarget,
    pub pattern: String,
    pub matched_value: String,
    pub legacy: bool,
}

#[derive(Debug, Clone)]
pub struct PlayerResolution {
    pub config: PlayerConfig,
    pub matches: Vec<PlayerConfigMatch>,
}

#[derive(Debug, Clone)]
pub struct SourceResolution {
    pub config: PlayerConfig,
    pub player_matches: Vec<PlayerConfigMatch>,
    pub web_player_key: Option<String>,
    pub title_suffix: Option<String>,
}

#[derive(Debug, Clone)]
struct CompiledPlayerPattern {
    target: PlayerMatchTarget,
    raw: String,
    matcher: CompiledPattern,
    legacy: bool,
}

impl CompiledPattern {
    fn matches(&self, candidate: &str) -> bool {
        match self {
            CompiledPattern::Exact(key) => key == candidate,
            CompiledPattern::Wildcard(re) | CompiledPattern::Regex(re) => re.is_match(candidate),
        }
    }
}

pub const DEFAULT_INTERVAL: u64 = 2000;
pub const DEFAULT_EVENT_DRIVEN: bool = true;
pub const DEFAULT_FALLBACK_POLL_INTERVAL: u64 = 30000;
pub const DEFAULT_WEB_PLAYER_ENABLED: bool = true;
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
    "{{#if album includeZero=true}}{{{album}}}{{else}}{{{title}}}{{/if}}";
const DEFAULT_TEMPLATE_SMALL_TEXT: &str = "{{{player}}}";

const DEFAULT_COVER_FILE_NAMES: [&str; 5] = ["cover", "folder", "front", "album", "art"];
const DEFAULT_COVER_PROVIDERS: [&str; 2] = ["catbox", "musicbrainz"];
const DEFAULT_COVER_LOCAL_SEARCH_DEPTH: usize = 2;
const DEFAULT_COVER_CACHE_MAX_SIZE_MB: u64 = 32;
const DEFAULT_COVER_CACHE_MAX_ENTRIES: usize = 1024;
const DEFAULT_COVER_CACHE_TTL_HOURS: u64 = 24;
const DEFAULT_MUSICBRAINZ_MIN_SCORE: u8 = 95;
const DEFAULT_CATBOX_USE_LITTER: bool = true;
const DEFAULT_CATBOX_LITTER_HOURS: u8 = 24;

macro_rules! normalized_map_serde {
    ($mod_name:ident, $value_type:ident, $entity:literal) => {
        pub(crate) mod $mod_name {
            use crate::utils::normalize_player_identity;
            use serde::{Deserialize, Deserializer, Serialize, Serializer};
            use std::collections::HashMap;

            pub fn serialize<S>(
                map: &HashMap<String, super::$value_type>,
                serializer: S,
            ) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                map.serialize(serializer)
            }

            pub fn deserialize<'de, D>(
                deserializer: D,
            ) -> Result<HashMap<String, super::$value_type>, D::Error>
            where
                D: Deserializer<'de>,
            {
                let temp_map = HashMap::<String, super::$value_type>::deserialize(deserializer)?;
                let mut final_map: HashMap<String, super::$value_type> = HashMap::new();
                for (key, value) in temp_map {
                    let normalized_key = normalize_player_identity(&key);
                    if let Some(existing) = final_map.get_mut(&normalized_key) {
                        log::debug!(
                            "Merging duplicate {} config for '{}' (from '{}')",
                            $entity,
                            normalized_key,
                            key
                        );
                        existing.merge_from(value);
                    } else {
                        log::debug!(
                            "Normalizing {} config key from '{}' to '{}'",
                            $entity,
                            key,
                            normalized_key
                        );
                        final_map.insert(normalized_key, value);
                    }
                }
                Ok(final_map)
            }
        }
    };
}

normalized_map_serde!(normalized_string, PlayerConfigLayer, "player");
normalized_map_serde!(
    normalized_web_player_string,
    WebPlayerConfigLayer,
    "web_player"
);

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

    #[serde(
        default = "default_fallback_poll_interval",
        alias = "discovery_interval"
    )]
    pub fallback_poll_interval: u64,

    #[serde(default = "default_allowed_players")]
    pub allowed_players: Vec<String>,

    /// Master switch for web player detection. When false, `xesam:url` and
    /// title suffixes are never inspected: every source resolves through the
    /// normal `[player.*]` rules, so a browser is configured like any other
    /// player. Top-level (not a `[web_player.*]` field) because `web_player`
    /// is a map of site entries - a per-site `enabled` would silently no-op.
    #[serde(default = "default_web_player_enabled")]
    pub web_player_enabled: bool,

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

    /// Per-key merge used to inherit bundled match patterns in user overrides.
    #[serde(skip)]
    pub merged_player: HashMap<String, PlayerConfigLayer>,

    #[serde(default)]
    #[serde(with = "normalized_web_player_string")]
    pub web_player: HashMap<String, WebPlayerConfigLayer>,

    #[serde(skip)]
    pub bundled_web_player: HashMap<String, WebPlayerConfigLayer>,

    #[serde(skip)]
    pub user_web_player: HashMap<String, WebPlayerConfigLayer>,

    /// Per-key merge of `bundled_web_player` and `user_web_player`. User fields
    /// override bundled fields; unset user fields fall through to bundled
    /// (so a user `[web_player.youtube] ignore = false` entry without
    /// `match_patterns` still matches via the bundled patterns). Rebuilt
    /// after every load/reload by `rebuild_merged_web_player`.
    #[serde(skip)]
    pub merged_web_player: HashMap<String, WebPlayerConfigLayer>,

    /// Pre-compiled player patterns (key → compiled matcher).  Populated by
    /// `precompile_patterns()` and used by all player-config lookups.
    #[serde(skip)]
    compiled_player_patterns: HashMap<String, Vec<CompiledPlayerPattern>>,

    /// Pre-compiled web_player patterns (key → list of compiled matchers, one per
    /// match pattern).  Populated by `precompile_patterns()`.
    #[serde(skip)]
    pub compiled_web_player_patterns: HashMap<String, Vec<CompiledPattern>>,
}

fn default_interval() -> u64 {
    DEFAULT_INTERVAL
}

fn default_event_driven() -> bool {
    DEFAULT_EVENT_DRIVEN
}

fn default_fallback_poll_interval() -> u64 {
    DEFAULT_FALLBACK_POLL_INTERVAL
}

fn default_allowed_players() -> Vec<String> {
    Vec::new()
}

fn default_web_player_enabled() -> bool {
    DEFAULT_WEB_PLAYER_ENABLED
}

impl Default for Config {
    fn default() -> Self {
        Config {
            interval: default_interval(),
            event_driven: default_event_driven(),
            fallback_poll_interval: default_fallback_poll_interval(),
            allowed_players: default_allowed_players(),
            web_player_enabled: default_web_player_enabled(),
            template: TemplateConfig::default(),
            time: TimeConfig::default(),
            cover: CoverConfig::default(),
            activity_type: ActivityTypesConfig::default(),
            player: HashMap::default(),
            bundled_player: HashMap::default(),
            user_player: HashMap::default(),
            user_player_patterns: HashSet::new(),
            merged_player: HashMap::default(),
            web_player: HashMap::default(),
            bundled_web_player: HashMap::default(),
            user_web_player: HashMap::default(),
            merged_web_player: HashMap::default(),
            compiled_player_patterns: HashMap::default(),
            compiled_web_player_patterns: HashMap::default(),
        }
    }
}

impl Config {
    #[cfg(test)]
    pub fn is_player_allowed(&self, identity: &str, player_bus_name: &str) -> bool {
        let resolution = self.resolve_player_config(identity, player_bus_name);
        let keys: Vec<String> = resolution
            .matches
            .iter()
            .map(|matched| matched.config_key.clone())
            .collect();
        self.is_source_allowed(identity, player_bus_name, &keys, None)
    }

    pub fn rebuild_merged_player(&mut self) {
        let keys: HashSet<String> = self
            .bundled_player
            .keys()
            .chain(self.user_player.keys())
            .cloned()
            .collect();
        self.merged_player.clear();
        for key in keys {
            let mut layer = self.bundled_player.get(&key).cloned().unwrap_or_default();
            if let Some(user_layer) = self.user_player.get(&key) {
                layer.merge_from(user_layer.clone());
            }
            self.merged_player.insert(key, layer);
        }
    }

    pub fn is_source_allowed(
        &self,
        identity: &str,
        player_bus_name: &str,
        player_keys: &[String],
        web_player_key: Option<&str>,
    ) -> bool {
        if self.allowed_players.is_empty() {
            return true;
        }

        let normalized_identity = normalize_player_identity(identity);
        let normalized_player_bus_name = normalize_player_identity(player_bus_name);

        self.allowed_players.iter().any(|pattern| {
            let (target, body) = parse_match_target(pattern);
            match target {
                Some(PlayerMatchTarget::Identity) => {
                    matches_normalized_pattern(body, &normalized_identity)
                }
                Some(PlayerMatchTarget::Bus) => {
                    matches_normalized_pattern(body, &normalized_player_bus_name)
                }
                Some(PlayerMatchTarget::Any) | None if pattern.starts_with("player:") => {
                    let body = pattern.trim_start_matches("player:");
                    player_keys
                        .iter()
                        .any(|key| matches_normalized_pattern(body, key))
                }
                Some(PlayerMatchTarget::Any) | None if pattern.starts_with("web_player:") => {
                    let body = pattern.trim_start_matches("web_player:");
                    web_player_key.is_some_and(|key| matches_normalized_pattern(body, key))
                }
                _ => {
                    matches_normalized_pattern(pattern, &normalized_identity)
                        || matches_normalized_pattern(pattern, &normalized_player_bus_name)
                }
            }
        })
    }

    pub fn get_player_config(&self, identity: &str, player_bus_name: &str) -> PlayerConfig {
        self.resolve_player_config(identity, player_bus_name).config
    }

    pub fn resolve_player_config(&self, identity: &str, player_bus_name: &str) -> PlayerResolution {
        let normalized_identity = normalize_player_identity(identity);
        let normalized_player_bus_name = normalize_player_identity(player_bus_name);
        let mut matched_layers = self.collect_ordered_player_matches(
            &self.bundled_player,
            &normalized_identity,
            &normalized_player_bus_name,
        );
        matched_layers.extend(self.collect_ordered_player_matches(
            &self.user_player,
            &normalized_identity,
            &normalized_player_bus_name,
        ));

        let match_info = matched_layers.iter().map(|m| m.info.clone()).collect();
        let layers = matched_layers.into_iter().map(|m| m.layer).collect();
        PlayerResolution {
            config: self.resolve_player_layers(layers),
            matches: match_info,
        }
    }

    pub fn resolve_source(
        &self,
        identity: &str,
        player_bus_name: &str,
        url: Option<&str>,
        title: Option<&str>,
    ) -> SourceResolution {
        let native = self.resolve_player_config(identity, player_bus_name);
        let (config, web_player_key, title_suffix) = if let Some(url) = url {
            let matched = self.matched_web_player_for_url(Some(url));
            (
                self.get_player_config_with_url(identity, player_bus_name, Some(url)),
                matched.map(|(key, _)| key),
                None,
            )
        } else if let Some((key, web, suffix)) = self.matched_web_player_for_title(title) {
            (web.into_player_config(), Some(key), Some(suffix))
        } else {
            (native.config.clone(), None, None)
        };
        SourceResolution {
            config,
            player_matches: native.matches,
            web_player_key,
            title_suffix,
        }
    }

    /// Like `get_player_config` but additionally overlays any matching
    /// `[web_player.*]` layers on top when the current track's URL matches.
    pub fn get_player_config_with_url(
        &self,
        identity: &str,
        player_bus_name: &str,
        url: Option<&str>,
    ) -> PlayerConfig {
        let base = self.get_player_config(identity, player_bus_name);
        self.apply_web_player_overrides(base, url)
    }

    /// When the current track's URL matches a configured `[web_player.*]`
    /// entry, the web_player's resolved config **fully replaces** the browser's
    /// player config. This is the spec: the web_player override is the
    /// authoritative configuration for any web-based player, regardless of
    /// which browser is hosting it. Unmatched http URLs are always ignored so
    /// random browser audio doesn't leak into Discord.
    ///
    /// No-op when `web_player_enabled = false`: the URL is never inspected and
    /// the browser's own `[player.*]` config stands.
    fn apply_web_player_overrides(&self, base: PlayerConfig, url: Option<&str>) -> PlayerConfig {
        if !self.web_player_enabled {
            return base;
        }
        let Some(raw_url) = url else {
            return base;
        };
        if raw_url.is_empty() {
            return base;
        }

        let host_or_url = url_host_for_match(raw_url);

        if let Some((key, _)) = find_matching_web_player_entry(
            &self.compiled_web_player_patterns,
            &self.merged_web_player,
            &host_or_url,
        ) {
            return self.resolve_web_player_config(&key).into_player_config();
        }

        // Unknown web URL (http/https with no matching web_player entry) is
        // always hidden - an unrecognized site has no name, icon, or app_id
        // worth showing.
        if is_http_url(raw_url) {
            let mut hidden = base;
            hidden.ignore = true;
            return hidden;
        }

        base
    }

    /// Public accessor: returns the matched web_player key and its fully
    /// resolved config for a given URL. Used by the CLI to surface which
    /// `[web_player.*]` entry the runtime would apply.
    pub fn matched_web_player_for_url(
        &self,
        url: Option<&str>,
    ) -> Option<(String, WebPlayerConfig)> {
        if !self.web_player_enabled {
            return None;
        }
        let raw_url = url?;
        if raw_url.is_empty() {
            return None;
        }
        let host = url_host_for_match(raw_url);
        let (key, _) = find_matching_web_player_entry(
            &self.compiled_web_player_patterns,
            &self.merged_web_player,
            &host,
        )?;
        let resolved = self.resolve_web_player_config(&key);
        Some((key, resolved))
    }

    pub fn matched_web_player_for_title(
        &self,
        title: Option<&str>,
    ) -> Option<(String, WebPlayerConfig, String)> {
        if !self.web_player_enabled {
            return None;
        }
        let (key, _, suffix) = self.match_web_player_by_title_suffix(title?)?;
        let resolved = self.resolve_web_player_config(&key);
        Some((key, resolved, suffix))
    }

    fn resolve_web_player_config(&self, key: &str) -> WebPlayerConfig {
        let mut resolved = WebPlayerConfig::default();
        if let Some(layer) = self.bundled_web_player.get("default") {
            resolved = layer.default_base().apply_into_web_player(resolved);
        }
        if key != "default" {
            if let Some(layer) = self.bundled_web_player.get(key) {
                resolved = layer.apply_into_web_player(resolved);
            }
        }
        if let Some(layer) = self.user_web_player.get("default") {
            resolved = layer.default_base().apply_into_web_player(resolved);
        }
        if key != "default" {
            if let Some(layer) = self.user_web_player.get(key) {
                resolved = layer.apply_into_web_player(resolved);
            }
        }
        resolved
    }

    /// Rebuilds `merged_web_player` from `bundled_web_player` and `user_web_player`.
    /// Must be called after either map is mutated (load + reload paths in
    /// `config::mod` do this). User fields win on collisions; unset user
    /// fields fall through to the bundled value so a patternless user
    /// override (e.g. `[web_player.youtube] ignore = false`) still inherits
    /// `match_patterns` from the bundled entry.
    pub fn rebuild_merged_web_player(&mut self) {
        let mut merged: HashMap<String, WebPlayerConfigLayer> = HashMap::new();
        let keys: HashSet<String> = self
            .bundled_web_player
            .keys()
            .chain(self.user_web_player.keys())
            .cloned()
            .collect();
        for key in keys {
            let mut layer = self
                .bundled_web_player
                .get(&key)
                .cloned()
                .unwrap_or_default();
            if let Some(user_layer) = self.user_web_player.get(&key) {
                layer.merge_from(user_layer.clone());
            }
            merged.insert(key, layer);
        }
        self.merged_web_player = merged;
    }

    /// Pre-compile all player and web_player patterns so repeated matching
    /// avoids per-call `Regex::new()` overhead. Must be called after
    /// `rebuild_merged_web_player()` and every config reload.
    pub fn precompile_patterns(&mut self) {
        self.compiled_player_patterns.clear();
        self.compiled_web_player_patterns.clear();

        // --- player patterns ---
        for (key, layer) in &self.merged_player {
            if key == "default" {
                continue;
            }
            let patterns = layer.effective_patterns();
            let compiled = if patterns.is_empty() {
                vec![Self::compile_player_pattern(key, true)]
            } else {
                patterns
                    .iter()
                    .map(|pattern| Self::compile_player_pattern(pattern, false))
                    .collect()
            };
            self.compiled_player_patterns.insert(key.clone(), compiled);
        }

        // --- web_player patterns ---
        for (key, layer) in &self.merged_web_player {
            if key == "default" {
                continue;
            }
            let patterns = layer.effective_patterns();
            if patterns.is_empty() {
                continue;
            }
            let compiled: Vec<CompiledPattern> = patterns
                .iter()
                .map(|p| Self::compile_single_pattern(p))
                .collect();
            self.compiled_web_player_patterns
                .insert(key.clone(), compiled);
        }
    }

    fn compile_single_pattern(pattern: &str) -> CompiledPattern {
        if is_regex_pattern(pattern) {
            let raw = if let Some(stripped) = pattern.strip_prefix("re:") {
                stripped.to_string()
            } else {
                pattern
                    .trim_start_matches('/')
                    .trim_end_matches('/')
                    .to_string()
            };
            match RegexBuilder::new(&raw).case_insensitive(true).build() {
                Ok(re) => CompiledPattern::Regex(re),
                Err(err) => {
                    log::warn!("Invalid regex pattern '{}': {}", pattern, err);
                    CompiledPattern::Exact(pattern.to_string())
                }
            }
        } else if is_wildcard_pattern(pattern) {
            let mut regex_str = String::from("^");
            for ch in pattern.chars() {
                match ch {
                    '*' => regex_str.push_str(".*"),
                    '?' => regex_str.push('.'),
                    _ => regex_str.push_str(&regex::escape(&ch.to_string())),
                }
            }
            regex_str.push('$');

            match Regex::new(&regex_str) {
                Ok(re) => CompiledPattern::Wildcard(re),
                Err(err) => {
                    log::warn!("Invalid wildcard pattern '{}': {}", pattern, err);
                    CompiledPattern::Exact(pattern.to_string())
                }
            }
        } else {
            CompiledPattern::Exact(pattern.to_string())
        }
    }

    fn compile_player_pattern(pattern: &str, legacy: bool) -> CompiledPlayerPattern {
        let (target, body) = parse_match_target(pattern);
        let target = target.unwrap_or(PlayerMatchTarget::Any);
        let normalized = if is_regex_pattern(body) {
            body.to_string()
        } else {
            normalize_player_identity(body)
        };
        CompiledPlayerPattern {
            target,
            raw: pattern.to_string(),
            matcher: Self::compile_single_pattern(&normalized),
            legacy,
        }
    }

    /// Try to match a web_player config by checking if the title ends with a
    /// configured `title_suffix` (or ` | <name>` as fallback). Operates on
    /// the merged per-key map so user-only entries that lack a suffix still
    /// inherit the bundled one.  Returns the matching layer and the suffix
    /// that matched (so it can be stripped from the displayed title).
    pub fn match_web_player_by_title_suffix(
        &self,
        title: &str,
    ) -> Option<(String, WebPlayerConfigLayer, String)> {
        // First pass: explicit title_suffix field.
        for (key, layer) in &self.merged_web_player {
            if let Some(suffix) = &layer.title_suffix {
                if title.ends_with(suffix.as_str()) {
                    return Some((key.clone(), layer.clone(), suffix.clone()));
                }
            }
        }

        // Second pass: fallback to " | <name>" pattern.
        for (key, layer) in &self.merged_web_player {
            if let Some(name) = &layer.name {
                let suffix = format!(" | {}", name);
                if title.ends_with(&suffix) {
                    return Some((key.clone(), layer.clone(), suffix));
                }
            }
        }

        None
    }

    /// Resolved (non-Layer) view of every configured web_player, used by
    /// `mprisence config web_players` and for inspection.
    pub fn effective_web_player_configs(&self) -> HashMap<String, WebPlayerConfig> {
        let mut keys: HashSet<String> = HashSet::new();
        for key in self
            .bundled_web_player
            .keys()
            .chain(self.user_web_player.keys())
        {
            keys.insert(key.clone());
        }

        let mut result = HashMap::new();
        for key in keys {
            result.insert(key.clone(), self.resolve_web_player_config(&key));
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
            let (mut resolved, _) = self.resolve_default_player_base();
            resolved.ignore = false;

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

    pub fn effective_player_patterns(&self) -> HashMap<String, Vec<String>> {
        self.merged_player
            .iter()
            .filter(|(key, _)| key.as_str() != "default")
            .map(|(key, layer)| {
                let patterns = layer.effective_patterns();
                let patterns = if patterns.is_empty() {
                    vec![key.clone()]
                } else {
                    patterns.into_iter().map(str::to_string).collect()
                };
                (key.clone(), patterns)
            })
            .collect()
    }

    fn resolve_player_layers(&self, matches: Vec<PlayerConfigLayer>) -> PlayerConfig {
        let (mut resolved, ignore_unmatched) = self.resolve_default_player_base();

        if matches.is_empty() {
            resolved.ignore = ignore_unmatched;
            return resolved;
        }

        resolved.ignore = false;

        for layer in matches {
            resolved = layer.apply_over(resolved);
        }

        resolved
    }

    fn resolve_default_player_base(&self) -> (PlayerConfig, bool) {
        let mut resolved = PlayerConfig::default();
        let mut ignore_unmatched = resolved.ignore;

        if let Some(layer) = self.bundled_player.get("default") {
            ignore_unmatched = layer.ignore_unmatched.unwrap_or(ignore_unmatched);
            resolved = layer.apply_over(resolved);
        }

        if let Some(layer) = self.user_player.get("default") {
            ignore_unmatched = layer.ignore_unmatched.unwrap_or(ignore_unmatched);
            resolved = layer.apply_over(resolved);
        }

        (resolved, ignore_unmatched)
    }

    fn collect_ordered_player_matches(
        &self,
        source: &HashMap<String, PlayerConfigLayer>,
        identity: &str,
        bus: &str,
    ) -> Vec<OrderedPlayerMatch> {
        let mut matches = Vec::new();
        for (key, layer) in source {
            if key == "default" {
                continue;
            }
            let patterns = self
                .compiled_player_patterns
                .get(key)
                .cloned()
                .unwrap_or_else(|| {
                    let effective = self
                        .merged_player
                        .get(key)
                        .map(PlayerConfigLayer::effective_patterns)
                        .unwrap_or_default();
                    if effective.is_empty() {
                        vec![Config::compile_player_pattern(key, true)]
                    } else {
                        effective
                            .iter()
                            .map(|pattern| Config::compile_player_pattern(pattern, false))
                            .collect()
                    }
                });

            let best = patterns
                .iter()
                .filter_map(|pattern| match_compiled_player_pattern(pattern, identity, bus))
                .max_by_key(|matched| matched.rank);
            if let Some(matched) = best {
                matches.push(OrderedPlayerMatch {
                    layer: layer.clone(),
                    info: PlayerConfigMatch {
                        config_key: key.clone(),
                        target: matched.target,
                        pattern: matched.pattern,
                        matched_value: matched.value,
                        legacy: matched.legacy,
                    },
                    rank: matched.rank,
                });
            }
        }

        matches.sort_by(|a, b| {
            a.rank
                .cmp(&b.rank)
                .then_with(|| a.info.config_key.cmp(&b.info.config_key))
        });
        matches
    }
}

struct OrderedPlayerMatch {
    layer: PlayerConfigLayer,
    info: PlayerConfigMatch,
    rank: (u8, u8, usize),
}

struct MatchedPlayerPattern {
    target: PlayerMatchTarget,
    pattern: String,
    value: String,
    legacy: bool,
    rank: (u8, u8, usize),
}

fn parse_match_target(pattern: &str) -> (Option<PlayerMatchTarget>, &str) {
    if let Some(body) = pattern.strip_prefix("identity:") {
        (Some(PlayerMatchTarget::Identity), body)
    } else if let Some(body) = pattern.strip_prefix("bus:") {
        (Some(PlayerMatchTarget::Bus), body)
    } else {
        (None, pattern)
    }
}

fn matches_normalized_pattern(pattern: &str, candidate: &str) -> bool {
    let normalized = if is_regex_pattern(pattern) {
        pattern.to_string()
    } else {
        normalize_player_identity(pattern)
    };
    Config::compile_single_pattern(&normalized).matches(candidate)
}

fn match_compiled_player_pattern(
    pattern: &CompiledPlayerPattern,
    identity: &str,
    bus: &str,
) -> Option<MatchedPlayerPattern> {
    let (target, value, scope_rank) = match pattern.target {
        PlayerMatchTarget::Identity if pattern.matcher.matches(identity) => {
            (PlayerMatchTarget::Identity, identity, 1)
        }
        PlayerMatchTarget::Bus if pattern.matcher.matches(bus) => (PlayerMatchTarget::Bus, bus, 2),
        PlayerMatchTarget::Any if pattern.matcher.matches(bus) => (PlayerMatchTarget::Bus, bus, 0),
        PlayerMatchTarget::Any if pattern.matcher.matches(identity) => {
            (PlayerMatchTarget::Identity, identity, 0)
        }
        _ => return None,
    };
    let (kind_rank, specificity) = match &pattern.matcher {
        CompiledPattern::Wildcard(_) => (0, pattern_specificity(&pattern.raw)),
        CompiledPattern::Regex(_) => (1, pattern.raw.len()),
        CompiledPattern::Exact(_) => (2, pattern.raw.len()),
    };
    Some(MatchedPlayerPattern {
        target,
        pattern: pattern.raw.clone(),
        value: value.to_string(),
        legacy: pattern.legacy,
        rank: (kind_rank, scope_rank, specificity),
    })
}

fn is_wildcard_pattern(s: &str) -> bool {
    !is_regex_pattern(s) && (s.contains('*') || s.contains('?'))
}

fn is_regex_pattern(s: &str) -> bool {
    (s.starts_with("re:") && s.len() > 3) || (s.starts_with('/') && s.ends_with('/') && s.len() > 2)
}

fn pattern_specificity(s: &str) -> usize {
    s.chars().filter(|&c| c != '*' && c != '?').count()
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
        assert!(res.ignore); // bundled exact ignore persists unless user explicitly clears it
    }

    #[test]
    fn matched_player_entry_without_ignore_starts_enabled_from_default_policy() {
        let mut cfg = Config::default();
        cfg.bundled_player.insert(
            "default".to_string(),
            PlayerConfigLayer {
                show_icon: Some(false),
                ignore_unmatched: Some(true),
                app_id: Some("MPRISENCE".to_string()),
                ..Default::default()
            },
        );
        cfg.user_player.insert(
            "custom_player".to_string(),
            PlayerConfigLayer {
                app_id: Some("CUSTOM".to_string()),
                ..Default::default()
            },
        );

        let custom = cfg.get_player_config("Custom Player", "custom_player");
        assert_eq!(custom.app_id, "CUSTOM");
        assert!(!custom.ignore);

        let unknown = cfg.get_player_config("Unknown Player", "unknown_player");
        assert_eq!(unknown.app_id, "MPRISENCE");
        assert!(unknown.ignore);
    }

    #[test]
    fn explicit_ignore_true_on_matched_player_still_wins() {
        let mut cfg = Config::default();
        cfg.bundled_player.insert(
            "default".to_string(),
            layer(Some(false), Some(true), Some("MPRISENCE")),
        );
        cfg.user_player.insert(
            "custom_player".to_string(),
            PlayerConfigLayer {
                ignore: Some(true),
                app_id: Some("CUSTOM".to_string()),
                ..Default::default()
            },
        );

        let custom = cfg.get_player_config("Custom Player", "custom_player");
        assert_eq!(custom.app_id, "CUSTOM");
        assert!(custom.ignore);
    }

    #[test]
    fn effective_player_configs_show_matched_entries_as_enabled() {
        let mut cfg = Config::default();
        cfg.bundled_player.insert(
            "default".to_string(),
            layer(Some(false), Some(true), Some("MPRISENCE")),
        );
        cfg.user_player.insert(
            "custom_player".to_string(),
            PlayerConfigLayer {
                app_id: Some("CUSTOM".to_string()),
                ..Default::default()
            },
        );

        let effective = cfg.effective_player_configs();
        let custom = effective
            .get("custom_player")
            .expect("custom player config");
        assert_eq!(custom.app_id, "CUSTOM");
        assert!(!custom.ignore);
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
    fn stable_player_key_uses_explicit_unscoped_patterns() {
        let mut cfg = Config::default();
        cfg.bundled_player.insert(
            "vlc".to_string(),
            PlayerConfigLayer {
                match_patterns: Some(vec!["vlc_media_player".to_string()]),
                app_id: Some("VLC".to_string()),
                ..Default::default()
            },
        );
        cfg.rebuild_merged_player();
        cfg.precompile_patterns();

        let resolution = cfg.resolve_player_config("VLC Media Player", "vlc");
        assert_eq!(resolution.config.app_id, "VLC");
        assert_eq!(resolution.matches[0].config_key, "vlc");
        assert_eq!(resolution.matches[0].pattern, "vlc_media_player");
        assert!(!resolution.matches[0].legacy);
    }

    #[test]
    fn scoped_patterns_only_match_the_selected_field() {
        let mut cfg = Config::default();
        cfg.user_player.insert(
            "identity_only".to_string(),
            PlayerConfigLayer {
                match_patterns: Some(vec!["identity:shared".to_string()]),
                app_id: Some("IDENTITY".to_string()),
                ..Default::default()
            },
        );
        cfg.user_player.insert(
            "bus_only".to_string(),
            PlayerConfigLayer {
                match_patterns: Some(vec!["bus:shared".to_string()]),
                app_id: Some("BUS".to_string()),
                ..Default::default()
            },
        );
        cfg.rebuild_merged_player();
        cfg.precompile_patterns();

        assert_eq!(cfg.get_player_config("shared", "other").app_id, "IDENTITY");
        assert_eq!(cfg.get_player_config("other", "shared").app_id, "BUS");
    }

    #[test]
    fn patternless_user_override_inherits_bundled_player_patterns() {
        let mut cfg = Config::default();
        cfg.bundled_player.insert(
            "vlc".to_string(),
            PlayerConfigLayer {
                match_patterns: Some(vec!["vlc_media_player".to_string()]),
                app_id: Some("BUNDLED".to_string()),
                ..Default::default()
            },
        );
        cfg.user_player.insert(
            "vlc".to_string(),
            PlayerConfigLayer {
                show_icon: Some(true),
                ..Default::default()
            },
        );
        cfg.rebuild_merged_player();
        cfg.precompile_patterns();

        let resolved = cfg.get_player_config("VLC Media Player", "vlc");
        assert_eq!(resolved.app_id, "BUNDLED");
        assert!(resolved.show_icon);
    }

    #[test]
    fn allowed_players_supports_typed_source_selectors() {
        let mut cfg = Config {
            allowed_players: vec![
                "player:vlc".to_string(),
                "web_player:youtube_music".to_string(),
                "bus:special_bus".to_string(),
                "identity:special_identity".to_string(),
            ],
            ..Default::default()
        };
        cfg.user_player.insert(
            "vlc".to_string(),
            PlayerConfigLayer {
                match_patterns: Some(vec!["vlc_media_player".to_string()]),
                ..Default::default()
            },
        );
        cfg.rebuild_merged_player();
        cfg.precompile_patterns();
        let keys = vec!["vlc".to_string()];

        assert!(cfg.is_source_allowed("other", "other", &keys, None));
        assert!(cfg.is_source_allowed("other", "other", &[], Some("youtube_music")));
        assert!(cfg.is_source_allowed("other", "special_bus", &[], None));
        assert!(cfg.is_source_allowed("special identity", "other", &[], None));
        assert!(!cfg.is_source_allowed("other", "other", &[], None));
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

    #[serde(default)]
    pub cache: CoverCacheConfig,
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
            cache: CoverCacheConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct CoverCacheConfig {
    #[serde(
        default = "default_cover_cache_max_size_mb",
        deserialize_with = "deserialize_positive_u64"
    )]
    pub max_size_mb: u64,

    #[serde(
        default = "default_cover_cache_max_entries",
        deserialize_with = "deserialize_positive_usize"
    )]
    pub max_entries: usize,

    #[serde(
        default = "default_cover_cache_ttl_hours",
        deserialize_with = "deserialize_positive_u64"
    )]
    pub ttl_hours: u64,
}

fn default_cover_cache_max_size_mb() -> u64 {
    DEFAULT_COVER_CACHE_MAX_SIZE_MB
}

fn default_cover_cache_max_entries() -> usize {
    DEFAULT_COVER_CACHE_MAX_ENTRIES
}

fn default_cover_cache_ttl_hours() -> u64 {
    DEFAULT_COVER_CACHE_TTL_HOURS
}

fn deserialize_positive_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = u64::deserialize(deserializer)?;
    if value == 0 {
        return Err(serde::de::Error::custom("must be greater than zero"));
    }
    Ok(value)
}

fn deserialize_positive_usize<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = usize::deserialize(deserializer)?;
    if value == 0 {
        return Err(serde::de::Error::custom("must be greater than zero"));
    }
    Ok(value)
}

impl Default for CoverCacheConfig {
    fn default() -> Self {
        Self {
            max_size_mb: default_cover_cache_max_size_mb(),
            max_entries: default_cover_cache_max_entries(),
            ttl_hours: default_cover_cache_ttl_hours(),
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
    pub match_pattern: Option<String>,

    #[serde(default)]
    pub match_patterns: Option<Vec<String>>,

    #[serde(default)]
    pub name: Option<String>,

    #[serde(default)]
    pub ignore: Option<bool>,

    #[serde(default)]
    pub ignore_unmatched: Option<bool>,

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
    pub fn effective_patterns(&self) -> Vec<&str> {
        let mut patterns = Vec::new();
        if let Some(pattern) = self.match_pattern.as_deref().filter(|p| !p.is_empty()) {
            patterns.push(pattern);
        }
        if let Some(values) = self.match_patterns.as_deref() {
            patterns.extend(values.iter().filter(|p| !p.is_empty()).map(String::as_str));
        }
        patterns
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

    pub fn merge_from(&mut self, other: PlayerConfigLayer) {
        self.match_pattern = other.match_pattern.or(self.match_pattern.take());
        self.match_patterns = other.match_patterns.or(self.match_patterns.take());
        self.name = other.name.or(self.name.take());
        self.ignore = other.ignore.or(self.ignore);
        self.ignore_unmatched = other.ignore_unmatched.or(self.ignore_unmatched);
        self.app_id = other.app_id.or(self.app_id.take());
        self.icon = other.icon.or(self.icon.take());
        self.show_icon = other.show_icon.or(self.show_icon);
        self.allow_streaming = other.allow_streaming.or(self.allow_streaming);
        self.status_display_type = other.status_display_type.or(self.status_display_type);
        self.override_activity_type = other.override_activity_type.or(self.override_activity_type);
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
pub struct WebPlayerConfigLayer {
    #[serde(default)]
    pub match_pattern: Option<String>,

    #[serde(default)]
    pub match_patterns: Option<Vec<String>>,

    /// Optional title suffix used to infer this web_player when `xesam:url` is
    /// absent (e.g. Chrome native MPRIS).  Example: `" | YouTube Music"`.
    /// When matched, the suffix is stripped from the displayed title.
    #[serde(default)]
    pub title_suffix: Option<String>,

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
    pub status_display_type: Option<StatusDisplayType>,

    #[serde(default)]
    pub override_activity_type: Option<ActivityType>,
}

impl WebPlayerConfigLayer {
    /// This layer as the shared `[web_player.default]` inheritance base.
    /// `ignore` is dropped: hiding is a per-site decision, and unknown sites
    /// are always hidden regardless. Turning web players off entirely is
    /// `web_player_enabled`, not an inherited `ignore`.
    pub(crate) fn default_base(&self) -> Self {
        Self {
            ignore: None,
            ..self.clone()
        }
    }

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

    pub fn merge_from(&mut self, other: WebPlayerConfigLayer) {
        self.match_pattern = other.match_pattern.or(self.match_pattern.take());
        self.match_patterns = other.match_patterns.or(self.match_patterns.take());
        self.title_suffix = other.title_suffix.or(self.title_suffix.take());
        self.name = other.name.or(self.name.take());
        self.ignore = other.ignore.or(self.ignore);
        self.app_id = other.app_id.or(self.app_id.take());
        self.icon = other.icon.or(self.icon.take());
        self.show_icon = other.show_icon.or(self.show_icon);
        self.status_display_type = other.status_display_type.or(self.status_display_type);
        self.override_activity_type = other.override_activity_type.or(self.override_activity_type);
    }

    fn apply_into_web_player(&self, mut base: WebPlayerConfig) -> WebPlayerConfig {
        let patterns = self.effective_patterns();
        if !patterns.is_empty() {
            base.match_patterns = patterns.into_iter().map(|s| s.to_string()).collect();
        }
        if let Some(value) = &self.title_suffix {
            base.title_suffix = Some(value.clone());
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
        if let Some(value) = self.status_display_type {
            base.status_display_type = Some(value);
        }
        if let Some(value) = self.override_activity_type {
            base.override_activity_type = Some(value);
        }
        base
    }
}

/// Resolved, inspectable form of a web_player entry (used by CLI listing
/// and to project into a `PlayerConfig` at runtime via
/// `into_player_config`). Fields stay optional so callers can distinguish
/// "web_player explicitly set this" from "fall back to mprisence default".
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebPlayerConfig {
    #[serde(default)]
    pub match_patterns: Vec<String>,
    #[serde(default)]
    pub title_suffix: Option<String>,
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
    pub status_display_type: Option<StatusDisplayType>,
    #[serde(default)]
    pub override_activity_type: Option<ActivityType>,
}

impl WebPlayerConfig {
    /// Project the web_player's resolved fields onto a fresh `PlayerConfig`.
    /// This is the authoritative-replace operation: every policy field
    /// either takes the web_player's explicit value or falls back to the
    /// mprisence default. Streaming is always allowed: a web player is
    /// matched *because* its URL is http(s), so `allow_streaming = false`
    /// would just be `ignore = true` with a worse name. The browser's
    /// `[player.*]` config does NOT contribute, which is the whole point
    /// of the web_player override.
    pub fn into_player_config(self) -> PlayerConfig {
        let mut p = PlayerConfig::default();
        if let Some(name) = self.name {
            p.name = Some(name);
        }
        p.ignore = self.ignore;
        if let Some(app_id) = self.app_id {
            p.app_id = app_id;
        }
        if let Some(icon) = self.icon {
            p.icon = icon;
        }
        if let Some(show_icon) = self.show_icon {
            p.show_icon = show_icon;
        }
        p.allow_streaming = true;
        if let Some(sdt) = self.status_display_type {
            p.status_display_type = sdt;
        }
        if let Some(act) = self.override_activity_type {
            p.override_activity_type = Some(act);
        }
        p
    }
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
mod web_player_tests {
    use super::*;

    fn web_player(match_pattern: &str, app_id: Option<&str>) -> WebPlayerConfigLayer {
        WebPlayerConfigLayer {
            match_pattern: Some(match_pattern.to_string()),
            app_id: app_id.map(|s| s.to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn website_into_player_config_projects_fields_and_falls_back_to_defaults() {
        let layer = WebPlayerConfigLayer {
            match_pattern: Some("music.youtube.com".to_string()),
            name: Some("YouTube Music".into()),
            app_id: Some("WEB_PLAYER".into()),
            icon: Some("yt-icon".into()),
            ..Default::default()
        };
        let resolved = layer.apply_into_web_player(WebPlayerConfig::default());
        let player = resolved.into_player_config();

        assert_eq!(player.app_id, "WEB_PLAYER");
        assert_eq!(player.icon, "yt-icon");
        assert_eq!(player.name.as_deref(), Some("YouTube Music"));
        assert!(player.allow_streaming);
        // ignore wasn't set in the layer -> uses WebPlayerConfig::default
        // (false), matching the new authoritative-replace semantics.
        assert!(!player.ignore);
        // show_icon wasn't set -> falls back to mprisence default (false).
        assert!(!player.show_icon);
    }

    #[test]
    fn website_into_player_config_always_allows_streaming() {
        // A web player is matched because its URL is http(s), which is
        // exactly what `is_streaming_url` tests. Blocking streaming here
        // would hide the entry outright, which is what `ignore` is for.
        let layer = WebPlayerConfigLayer {
            match_pattern: Some("music.youtube.com".to_string()),
            ..Default::default()
        };
        let resolved = layer.apply_into_web_player(WebPlayerConfig::default());

        assert!(resolved.into_player_config().allow_streaming);
    }

    fn build_cfg(setup: impl FnOnce(&mut Config)) -> Config {
        let mut cfg = Config::default();
        setup(&mut cfg);
        cfg.rebuild_merged_web_player();
        cfg.precompile_patterns();
        cfg
    }

    #[test]
    fn web_player_match_host_swaps_app_id() {
        let cfg = build_cfg(|cfg| {
            cfg.bundled_web_player.insert(
                "youtube_music".into(),
                web_player("music.youtube.com", Some("YT")),
            );
        });

        let resolved = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://music.youtube.com/watch?v=abc"),
        );
        assert_eq!(resolved.app_id, "YT");
        assert!(resolved.allow_streaming);
    }

    #[test]
    fn web_player_match_patterns_plural_any_entry_matches() {
        let cfg = build_cfg(|cfg| {
            cfg.bundled_web_player.insert(
                "soundcloud".into(),
                WebPlayerConfigLayer {
                    match_patterns: Some(vec!["soundcloud.com".into(), "snd.sc".into()]),
                    app_id: Some("SC".into()),
                    ..Default::default()
                },
            );
        });

        let resolved_long = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://soundcloud.com/discover/sets/x"),
        );
        assert_eq!(resolved_long.app_id, "SC");
        assert!(resolved_long.allow_streaming);

        let resolved_short =
            cfg.get_player_config_with_url("Firefox", "firefox", Some("https://snd.sc/abc"));
        assert_eq!(resolved_short.app_id, "SC");
        assert!(resolved_short.allow_streaming);
    }

    #[test]
    fn web_player_match_regex_on_host() {
        let cfg = build_cfg(|cfg| {
            cfg.bundled_web_player.insert(
                "bandcamp".into(),
                web_player("re:.*\\.bandcamp\\.com$", Some("BC")),
            );
        });

        let resolved = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://artist.bandcamp.com/track/y"),
        );
        assert_eq!(resolved.app_id, "BC");
    }

    #[test]
    fn website_unknown_http_url_forces_ignore() {
        let cfg = build_cfg(|cfg| {
            cfg.bundled_web_player.insert(
                "youtube_music".into(),
                web_player("music.youtube.com", Some("YT")),
            );
        });

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
        let cfg = build_cfg(|cfg| {
            cfg.bundled_web_player.insert(
                "youtube_music".into(),
                web_player("music.youtube.com", Some("YT")),
            );
        });
        let baseline = cfg.get_player_config("Spotify", "spotify");

        let resolved =
            cfg.get_player_config_with_url("Spotify", "spotify", Some("spotify:track:abc123"));
        assert_eq!(resolved.app_id, baseline.app_id);
        assert_eq!(resolved.ignore, baseline.ignore);
    }

    #[test]
    fn website_file_url_falls_through_to_base() {
        let cfg = build_cfg(|cfg| {
            cfg.bundled_web_player.insert(
                "youtube_music".into(),
                web_player("music.youtube.com", Some("YT")),
            );
        });
        let baseline = cfg.get_player_config("VLC", "vlc");

        let resolved =
            cfg.get_player_config_with_url("VLC", "vlc", Some("file:///home/user/track.flac"));
        assert_eq!(resolved.ignore, baseline.ignore);
    }

    #[test]
    fn website_no_url_returns_base_player_config() {
        let cfg = build_cfg(|cfg| {
            cfg.bundled_web_player.insert(
                "youtube_music".into(),
                web_player("music.youtube.com", Some("YT")),
            );
        });
        let baseline = cfg.get_player_config("Firefox", "firefox");

        let resolved = cfg.get_player_config_with_url("Firefox", "firefox", None);
        assert_eq!(resolved.app_id, baseline.app_id);
    }

    #[test]
    fn website_user_overrides_bundled() {
        let cfg = build_cfg(|cfg| {
            cfg.bundled_web_player.insert(
                "youtube_music".into(),
                web_player("music.youtube.com", Some("BUNDLED")),
            );
            cfg.user_web_player.insert(
                "youtube_music".into(),
                web_player("music.youtube.com", Some("USER")),
            );
        });

        let resolved = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://music.youtube.com/watch?v=x"),
        );
        assert_eq!(resolved.app_id, "USER");
    }

    /// Per-site `ignore` remains load-bearing: bundled entries ship
    /// `ignore = true` as opt-in, and a user entry flips them on.
    #[test]
    fn per_site_ignore_still_controls_bundled_sites() {
        let mut cfg = Config::default();
        cfg.bundled_web_player.insert(
            "youtube".into(),
            WebPlayerConfigLayer {
                match_pattern: Some("youtube.com".into()),
                name: Some("YT".into()),
                ignore: Some(true),
                ..Default::default()
            },
        );
        cfg.rebuild_merged_web_player();
        cfg.precompile_patterns();

        let disabled = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://youtube.com/watch?v=x"),
        );
        assert!(disabled.ignore);

        cfg.user_web_player.insert(
            "youtube".into(),
            WebPlayerConfigLayer {
                ignore: Some(false),
                ..Default::default()
            },
        );
        cfg.rebuild_merged_web_player();
        cfg.precompile_patterns();
        let enabled = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://youtube.com/watch?v=x"),
        );
        assert!(!enabled.ignore);
    }

    /// `[web_player.default] ignore` is gone: it must not leak into site
    /// entries as an inheritance base, and unknown URLs stay hidden anyway.
    #[test]
    fn user_web_default_ignore_does_not_hide_matched_sites() {
        let mut cfg = Config::default();
        cfg.bundled_web_player.insert(
            "tidal".into(),
            WebPlayerConfigLayer {
                match_pattern: Some("tidal.com".into()),
                ignore: Some(false),
                ..Default::default()
            },
        );
        cfg.user_web_player.insert(
            "default".into(),
            WebPlayerConfigLayer {
                ignore: Some(true),
                ..Default::default()
            },
        );
        cfg.rebuild_merged_web_player();
        cfg.precompile_patterns();

        let matched =
            cfg.get_player_config_with_url("Firefox", "firefox", Some("https://tidal.com/track/1"));
        assert!(
            !matched.ignore,
            "default ignore must not hide a matched site"
        );

        let unmatched = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://unknown.example/track"),
        );
        assert!(unmatched.ignore, "unknown websites are always hidden");
    }

    /// Toggle off: neither detection channel fires. The URL channel must fall
    /// back to the browser's own `[player.*]` config, and the title-suffix
    /// channel must not rewrite the player or strip the suffix.
    #[test]
    fn disabled_web_players_fall_through_to_player_rules() {
        let mut cfg = Config {
            web_player_enabled: false,
            ..Default::default()
        };
        cfg.bundled_player.insert(
            "firefox".into(),
            PlayerConfigLayer {
                match_patterns: Some(vec!["firefox".into()]),
                ignore: Some(false),
                name: Some("Firefox".into()),
                ..Default::default()
            },
        );
        cfg.bundled_web_player.insert(
            "youtube_music".into(),
            WebPlayerConfigLayer {
                match_pattern: Some("music.youtube.com".into()),
                title_suffix: Some(" | YouTube Music".into()),
                name: Some("YouTube Music".into()),
                ignore: Some(false),
                ..Default::default()
            },
        );
        cfg.rebuild_merged_player();
        cfg.rebuild_merged_web_player();
        cfg.precompile_patterns();

        let by_url = cfg.resolve_source(
            "Firefox",
            "firefox",
            Some("https://music.youtube.com/watch?v=x"),
            None,
        );
        assert!(by_url.web_player_key.is_none());
        assert_eq!(by_url.config.name.as_deref(), Some("Firefox"));
        assert!(!by_url.config.ignore);

        let by_title = cfg.resolve_source("Firefox", "firefox", None, Some("Song | YouTube Music"));
        assert!(by_title.web_player_key.is_none());
        assert!(by_title.title_suffix.is_none());
        assert_eq!(by_title.config.name.as_deref(), Some("Firefox"));

        // Unknown http URLs are no longer force-hidden either - [player.*] rules govern.
        let unknown = cfg.resolve_source(
            "Firefox",
            "firefox",
            Some("https://unknown.example/x"),
            None,
        );
        assert!(!unknown.config.ignore);
    }

    /// The same URL, with the toggle on, still gets the website identity.
    #[test]
    fn enabled_web_players_still_override_browser() {
        let mut cfg = Config::default();
        cfg.bundled_web_player.insert(
            "youtube_music".into(),
            WebPlayerConfigLayer {
                match_pattern: Some("music.youtube.com".into()),
                name: Some("YouTube Music".into()),
                ignore: Some(false),
                ..Default::default()
            },
        );
        cfg.rebuild_merged_web_player();
        cfg.precompile_patterns();

        let resolved = cfg.resolve_source(
            "Firefox",
            "firefox",
            Some("https://music.youtube.com/watch?v=x"),
            None,
        );
        assert_eq!(resolved.web_player_key.as_deref(), Some("youtube_music"));
        assert_eq!(resolved.config.name.as_deref(), Some("YouTube Music"));
    }

    #[test]
    fn website_ignore_propagates_to_resolved_player_config() {
        let cfg = build_cfg(|cfg| {
            cfg.bundled_web_player.insert(
                "spotify_web".into(),
                WebPlayerConfigLayer {
                    match_pattern: Some("open.spotify.com".into()),
                    ignore: Some(true),
                    ..Default::default()
                },
            );
        });

        let resolved = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://open.spotify.com/track/abc"),
        );
        assert!(resolved.ignore);
    }

    #[test]
    fn website_pattern_more_specific_than_substring_wins() {
        let cfg = build_cfg(|cfg| {
            // Both patterns would match the URL; exact host should win over substring.
            cfg.bundled_web_player.insert(
                "youtube_dot_com".into(),
                web_player("youtube.com", Some("GENERIC")),
            );
            cfg.bundled_web_player.insert(
                "youtube_music".into(),
                web_player("music.youtube.com", Some("SPECIFIC")),
            );
        });

        let resolved = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://music.youtube.com/watch?v=x"),
        );
        assert_eq!(resolved.app_id, "SPECIFIC");
    }

    #[test]
    fn user_patternless_layer_inherits_bundled_patterns() {
        // The whole reason `merged_web_player` exists: a user entry like
        // `[web_player.youtube]\nignore = false` (no patterns) used to be
        // silently skipped by `find_matching_website_layer` because its
        // `effective_patterns()` returned empty. After per-key merge the
        // bundled patterns flow through, so the user's `ignore` flip
        // actually takes effect.
        let cfg = build_cfg(|cfg| {
            cfg.bundled_web_player.insert(
                "youtube".into(),
                WebPlayerConfigLayer {
                    match_patterns: Some(vec!["youtube.com".into(), "youtu.be".into()]),
                    app_id: Some("YT_BUNDLED".into()),
                    ignore: Some(true),
                    ..Default::default()
                },
            );
            cfg.user_web_player.insert(
                "youtube".into(),
                WebPlayerConfigLayer {
                    ignore: Some(false),
                    ..Default::default()
                },
            );
        });

        let resolved = cfg.get_player_config_with_url(
            "Firefox",
            "firefox",
            Some("https://www.youtube.com/watch?v=x"),
        );
        assert!(
            !resolved.ignore,
            "user override should flip ignore to false"
        );
        assert_eq!(
            resolved.app_id, "YT_BUNDLED",
            "bundled app_id should still apply since user didn't override it"
        );
        assert!(
            resolved.allow_streaming,
            "matched web_player should still allow streaming by default"
        );
    }

    #[test]
    fn website_fully_replaces_browser_player_config() {
        // The browser's [player.*] config (ignore=true, app_id=BROWSER) must
        // NOT bleed into the resolved config when the URL matches a web_player.
        // Only the web_player's fields plus mprisence defaults survive.
        let cfg = build_cfg(|cfg| {
            cfg.bundled_player.insert(
                "firefox".into(),
                PlayerConfigLayer {
                    ignore: Some(true),
                    app_id: Some("BROWSER".into()),
                    icon: Some("browser-icon".into()),
                    allow_streaming: Some(true),
                    ..Default::default()
                },
            );
            cfg.bundled_web_player
                .insert("youtube".into(), web_player("youtube.com", Some("YT_SITE")));
        });

        let resolved = cfg.get_player_config_with_url(
            "Mozilla Firefox",
            "firefox",
            Some("https://www.youtube.com/watch?v=x"),
        );
        assert_eq!(
            resolved.app_id, "YT_SITE",
            "web_player app_id must win, not browser's"
        );
        assert!(
            !resolved.ignore,
            "web_player's ignore=false (default) must replace browser's ignore=true"
        );
        assert_ne!(
            resolved.icon, "browser-icon",
            "browser icon must not leak through when web_player matches"
        );
    }

    #[test]
    fn matched_web_player_for_url_returns_key_and_config() {
        let cfg = build_cfg(|cfg| {
            cfg.bundled_web_player.insert(
                "youtube".into(),
                WebPlayerConfigLayer {
                    match_patterns: Some(vec!["youtube.com".into(), "youtu.be".into()]),
                    app_id: Some("YT".into()),
                    ..Default::default()
                },
            );
        });

        let (key, resolved) = cfg
            .matched_web_player_for_url(Some("https://www.youtube.com/watch?v=x"))
            .expect("youtube.com should match");
        assert_eq!(key, "youtube");
        assert_eq!(resolved.app_id.as_deref(), Some("YT"));

        assert!(cfg
            .matched_web_player_for_url(Some("https://unrelated.example/"))
            .is_none());
        assert!(cfg.matched_web_player_for_url(None).is_none());
    }
}

/// Returns the key and most specific matching web_player layer from a single source map.
/// Priority: exact host > regex > wildcard > plain substring fallback.
/// Use `.map(|(_, layer)| layer)` to discard the key when not needed.
fn find_matching_web_player_entry(
    compiled_patterns: &HashMap<String, Vec<CompiledPattern>>,
    source: &HashMap<String, WebPlayerConfigLayer>,
    url_host: &str,
) -> Option<(String, WebPlayerConfigLayer)> {
    let mut best: Option<(String, WebPlayerConfigLayer, (u8, usize))> = None;

    for (key, layer) in source.iter() {
        if key == "default" {
            continue;
        }
        let compiled_list: std::borrow::Cow<'_, [CompiledPattern]> =
            match compiled_patterns.get(key) {
                Some(c) => std::borrow::Cow::Borrowed(c.as_slice()),
                None => {
                    // Fallback: compile on-demand (tests that bypass precompile_patterns).
                    let list: Vec<CompiledPattern> = layer
                        .effective_patterns()
                        .iter()
                        .map(|p| Config::compile_single_pattern(p))
                        .collect();
                    std::borrow::Cow::Owned(list)
                }
            };

        for (idx, compiled) in compiled_list.iter().enumerate() {
            // For the score, we need the raw pattern string from effective_patterns.
            // Re-derive the raw pattern for specificity scoring.
            let raw_patterns = layer.effective_patterns();
            let raw = raw_patterns.get(idx).copied().unwrap_or("");

            let score: Option<(u8, usize)> = match compiled {
                CompiledPattern::Exact(p) if p == url_host => Some((3, raw.len())),
                CompiledPattern::Regex(_) if compiled.matches(url_host) => Some((2, raw.len())),
                CompiledPattern::Wildcard(_) if compiled.matches(url_host) => {
                    Some((1, pattern_specificity(raw)))
                }
                _ => {
                    // Fallback: contains match (only for non-regex, non-wildcard strings)
                    if !is_regex_pattern(raw) && !is_wildcard_pattern(raw) && url_host.contains(raw)
                    {
                        Some((0, raw.len()))
                    } else {
                        None
                    }
                }
            };

            let Some(score) = score else { continue };

            match &best {
                Some((_, _, current)) if *current >= score => {}
                _ => best = Some((key.clone(), layer.clone(), score)),
            }
        }
    }

    best.map(|(key, layer, _)| (key, layer))
}
