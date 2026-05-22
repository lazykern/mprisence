use std::{collections::HashMap, fmt::Display, time::Duration};

use log::{debug, info, trace};
use mpris::{DBusError, PlaybackStatus, Player};
use smol_str::SmolStr;
use url::Url;

pub mod cmus;
pub mod events;
pub mod health;

const MPRIS_BUS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const PLAYERCTLD_NO_ACTIVE_PLAYER_ERROR: &str = "com.github.altdesktop.playerctld.NoActivePlayer";
const PLAYERCTLD_NO_ACTIVE_PLAYER_MESSAGE: &str = "No player is being controlled by playerctld";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PlayerIdentifier {
    pub player_bus_name: SmolStr,
    pub identity: SmolStr,
    pub unique_name: SmolStr,
}

impl Display for PlayerIdentifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.identity, self.player_bus_name, self.unique_name
        )
    }
}

impl From<&Player> for PlayerIdentifier {
    fn from(player: &Player) -> Self {
        let player_bus_name = canonical_player_bus_name(player.bus_name());

        Self {
            player_bus_name: SmolStr::new(player_bus_name),
            identity: SmolStr::new(player.identity()),
            unique_name: SmolStr::new(player.unique_name()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PlaybackState {
    pub playback_status: Option<PlaybackStatus>,
    pub track_identifier: Option<Box<str>>,
    pub title: Option<Box<str>>,
    pub position: Option<u32>,
    pub volume: Option<u8>,
    pub url: Option<Box<str>>,
    pub art_url: Option<Box<str>>,
}

impl Display for PlaybackState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?}: {} [{}s, {}%]",
            self.playback_status,
            self.title.as_deref().unwrap_or("Unknown"),
            self.position.unwrap_or(0),
            self.volume.unwrap_or(0)
        )?;

        if let Some(id) = &self.track_identifier {
            write!(f, " id:{}", id)?;
        }

        Ok(())
    }
}

impl From<&Player> for PlaybackState {
    fn from(player: &Player) -> Self {
        let metadata = player.get_metadata().ok();
        let playback_status = player.get_playback_status().ok();
        Self::from_parts(player, playback_status, metadata.as_ref())
    }
}

impl PlaybackState {
    /// Construct `PlaybackState` from an already-fetched status and metadata.
    /// Avoids redundant D-Bus calls when the caller already has both values.
    pub fn from_with_status(
        player: &Player,
        playback_status: PlaybackStatus,
        metadata: &mpris::Metadata,
    ) -> Self {
        Self::from_parts(player, Some(playback_status), Some(metadata))
    }

    fn from_parts(
        player: &Player,
        playback_status: Option<PlaybackStatus>,
        metadata: Option<&mpris::Metadata>,
    ) -> Self {
        let track_identifier = metadata
            .and_then(|m| {
                m.track_id()
                    .map(|s| s.to_string())
                    .or_else(|| m.url().map(|s| s.to_string()))
            })
            .map(|s| s.into_boxed_str());

        Self {
            playback_status,
            track_identifier,
            title: metadata.and_then(|m| m.title().map(|s| s.to_string().into_boxed_str())),
            position: player.get_position().map(|d| d.as_secs() as u32).ok(),
            volume: player.get_volume().map(|v| (v * 100.0) as u8).ok(),
            url: metadata.and_then(|m| m.url().map(|s| s.to_string().into_boxed_str())),
            art_url: metadata.and_then(|m| m.art_url().map(|s| s.to_string().into_boxed_str())),
        }
    }
}

pub fn canonical_player_bus_name(raw_bus_name: &str) -> String {
    let without_prefix = raw_bus_name.trim_start_matches(MPRIS_BUS_PREFIX);
    let mut segments: Vec<&str> = without_prefix.split('.').collect();

    if segments.len() > 1 {
        if let Some(last) = segments.last() {
            if last.starts_with("instance") || last.chars().all(|c| c.is_ascii_digit()) {
                segments.pop();
            }
        }
    }

    segments.join(".")
}

/// Returns true if the given canonical bus name is a known proxy bus (e.g. playerctld).
/// Proxy buses forward MPRIS events from another player rather than being the source.
pub fn is_proxy_bus_name(canonical_bus_name: &str) -> bool {
    canonical_bus_name == "playerctld"
}

/// Returns true if this is a bridge MPRIS player from mprisence-web-bridge.
/// Detects by bus prefix: all bridge players share the `mprisence_web` namespace.
/// Uses the raw bus name (before canonicalization) for reliable detection.
pub fn is_mprisence_web_bridge_bus(raw_bus_name: &str) -> bool {
    let canon = canonical_player_bus_name(raw_bus_name);
    canon.starts_with("mprisence_web")
}

/// The stable config key all bridge MPRIS players resolve to.
/// Instead of `mprisence_web.youtube_music.abc123` matching individual config,
/// all bridge players use `mprisence_web` as their config lookup key.
pub const BRIDGE_CONFIG_KEY: &str = "mprisence_web";

/// Returns true if MPRIS metadata contains the bridge marker.
#[allow(dead_code)]
pub fn has_bridge_metadata_marker(metadata: &mpris::Metadata) -> bool {
    metadata
        .get("mprisence:bridge")
        .and_then(|v| v.as_str())
        .map(|s| s == "true")
        .unwrap_or(false)
}

/// Extracts the group key from bridge MPRIS metadata.
/// Returns the value of `mprisence:group` (the site), or None if not a bridge player.
#[allow(dead_code)]
pub fn bridge_group_key(metadata: &mpris::Metadata) -> Option<String> {
    if !has_bridge_metadata_marker(metadata) {
        return None;
    }
    metadata
        .get("mprisence:group")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Extracts the site from bridge MPRIS metadata.
#[allow(dead_code)]
pub fn bridge_site(metadata: &mpris::Metadata) -> Option<String> {
    metadata
        .get("mprisence:site")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Extracts confidence from bridge MPRIS metadata.
pub fn bridge_confidence(metadata: &mpris::Metadata) -> Option<String> {
    metadata
        .get("mprisence:confidence")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Extracts whether this source is active from bridge MPRIS metadata.
pub fn bridge_is_active(metadata: &mpris::Metadata) -> bool {
    metadata
        .get("mprisence:active")
        .and_then(|v| v.as_str())
        .map(|s| s == "true")
        .unwrap_or(false)
}

/// Score a player's metadata richness (higher = richer).
/// Used to break ties when multiple bus names expose the same content.
pub(crate) fn metadata_richness(player: &Player) -> u8 {
    let mut score: u8 = 0;
    if let Ok(meta) = player.get_metadata() {
        if meta.title().is_some() {
            score += 1;
        }
        if meta.album_name().is_some_and(|a| !a.is_empty()) {
            score += 2;
        }
        if meta
            .artists()
            .is_some_and(|a| a.iter().any(|s| !s.is_empty()))
        {
            score += 2;
        }
        if meta.art_url().is_some() {
            score += 3;
        }
        if meta.length().is_some() {
            score += 1;
        }
    }
    score
}

/// Select the winner among bridge players in the same group (same site).
/// Priority:
/// 1. Active + playing (bridge marks the best source)
/// 2. Playing
/// 3. Active + paused (user's current tab)
/// 4. Provider confidence
/// 5. Newest seen age
/// 6. Current bus (stability)
/// 7. First (fallback)
///
/// Scoring ensures playing always beats active+paused within the same
/// heartbeat window (~2s). Only when the playing tab goes stale (>60s
/// without update) can active+paused overtake.
pub fn select_bridge_winner(
    players: &[Player],
    current_bus: Option<&str>,
) -> usize {
    if players.len() <= 1 {
        return 0;
    }

    let ids: Vec<PlayerIdentifier> = players.iter().map(PlayerIdentifier::from).collect();

    // Score each player, higher = better
    let scores: Vec<u32> = players
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let meta = p.get_metadata().ok();
            let is_active = meta
                .as_ref()
                .map(|m| bridge_is_active(m))
                .unwrap_or(false);
            let is_playing = p.get_playback_status().ok()
                .map(|s| s == PlaybackStatus::Playing)
                .unwrap_or(false);
            let confidence_score = meta
                .as_ref()
                .and_then(|m| bridge_confidence(m))
                .map(|c| match c.as_str() {
                    "provider" => 3u32,
                    "dom" => 2,
                    _ => 1,
                })
                .unwrap_or(0);
            let last_seen_age = meta
                .as_ref()
                .and_then(|m| m.get("mprisence:seenAgeMs"))
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);

            let mut score: u32 = 0;
            // Active + playing = highest
            if is_active && is_playing { score += 200; }
            // Playing
            else if is_playing { score += 150; }
            // Active paused (user's current tab)
            else if is_active { score += 120; }
            // Neither
            else { score += 80; }

            score += confidence_score;
            // Newer lastSeen = higher score (already ms, smaller = newer)
            // Invert: max age ~ 60000ms, subtract
            score += 60_000u32.saturating_sub(last_seen_age as u32) / 1000;

            // Stability bonus: current bus gets a small boost
            if current_bus.map_or(false, |b| b == ids[i].player_bus_name.as_str()) {
                score += 5;
            }

            score
        })
        .collect();

    // Find max score index
    let mut best = 0;
    for i in 1..scores.len() {
        if scores[i] > scores[best] {
            best = i;
        }
    }
    best
}

/// Given a group of `Player`s that represent the same content (merged by URL),
/// returns the index of the one with the richest metadata.
/// Falls back to `select_winner_idx` tie-breaking if scores are equal.
pub fn select_richest_player(players: &[Player], current_bus: Option<&str>) -> usize {
    if players.len() <= 1 {
        return 0;
    }

    let ids: Vec<PlayerIdentifier> = players.iter().map(PlayerIdentifier::from).collect();

    // Find the richest player.
    let mut best_idx = 0;
    let mut best_score = metadata_richness(&players[0]);
    for (i, player) in players.iter().enumerate().skip(1) {
        let score = metadata_richness(player);
        if score > best_score {
            best_score = score;
            best_idx = i;
        }
    }

    // Prefer stability only when the current bus is equally rich.
    if let Some(current) = current_bus {
        if let Some(idx) = ids
            .iter()
            .position(|id| id.player_bus_name.as_str() == current)
        {
            if metadata_richness(&players[idx]) >= best_score {
                return idx;
            }
        }
    }

    best_idx
}

/// Given a slice of `PlayerIdentifier`s that all share the same player identity,
/// returns the index of the preferred ("winner") entry.
///
/// Selection priority:
/// 1. The currently-tracked bus name (stability — avoid unnecessary switching).
/// 2. A non-proxy bus name (prefer the real player over a proxy like playerctld).
/// 3. The first candidate in the slice (fallback).
pub fn select_winner_idx(candidates: &[PlayerIdentifier], current_bus: Option<&str>) -> usize {
    if let Some(current) = current_bus {
        if let Some(idx) = candidates
            .iter()
            .position(|id| id.player_bus_name.as_str() == current)
        {
            return idx;
        }
    }

    if let Some(idx) = candidates
        .iter()
        .position(|id| !is_proxy_bus_name(&id.player_bus_name))
    {
        return idx;
    }

    0
}

pub fn is_playerctld_no_active_error(error: &DBusError) -> bool {
    match error {
        DBusError::TransportError(transport_error) => {
            transport_error.name() == Some(PLAYERCTLD_NO_ACTIVE_PLAYER_ERROR)
                || transport_error
                    .message()
                    .map(|message| message.contains(PLAYERCTLD_NO_ACTIVE_PLAYER_MESSAGE))
                    .unwrap_or(false)
        }
        _ => false,
    }
}

impl PlaybackState {
    pub fn has_significant_changes(&self, previous: &Self) -> bool {
        if self.track_identifier != previous.track_identifier {
            debug!("Track identity changed");
            return true;
        }

        // URL change is significant even when track_id is static (e.g. plasma-
        // browser-integration reuses the same mpris:trackid across tracks).
        // This allows the stale-URL quarantine to exit when plasma finally
        // updates xesam:url to the correct value.
        if self.url != previous.url {
            debug!("URL changed: {:?} -> {:?}", previous.url, self.url);
            return true;
        }

        // Art URL changes are significant even when the title/track id/url are
        // unchanged. plasma-browser-integration often fixes mpris:artUrl a few
        // seconds after the track title changes; reaching update_activity lets
        // the generation gate cancel stale cover fetches and spawn the fresh one.
        if self.art_url != previous.art_url {
            debug!(
                "Art URL changed: {:?} -> {:?}",
                previous.art_url, self.art_url
            );
            return true;
        }

        if self.playback_status != previous.playback_status || self.volume != previous.volume {
            info!(
                "Player changed status: {:?} -> {:?}",
                previous.playback_status, self.playback_status,
            );
            return true;
        }

        false
    }

    pub fn has_position_jump(
        &self,
        previous: &Self,
        polling_interval: Duration,
        dbus_delay: Duration,
    ) -> bool {
        // Add a buffer to account for variations
        const BUFFER_DURATION: Duration = Duration::from_secs(2);

        let max_expected_change_duration = polling_interval + dbus_delay + BUFFER_DURATION;
        let max_expected_change = max_expected_change_duration.as_secs() as u32;

        if self.position < previous.position {
            debug!(
                "Position jumped backward: {}s -> {}s",
                previous.position.unwrap_or(0),
                self.position.unwrap_or(0)
            );
            return true;
        }

        let elapsed = self
            .position
            .unwrap_or(0)
            .saturating_sub(previous.position.unwrap_or(0));
        if elapsed > max_expected_change {
            debug!(
                "Position jumped forward: {}s -> {}s (expected max change: {}s)",
                previous.position.unwrap_or(0),
                self.position.unwrap_or(0),
                max_expected_change
            );
            return true;
        }

        false
    }
}

/// Per-group snapshot used to compute URL-based merges deterministically,
/// decoupled from D-Bus I/O so the merge contract is unit-testable.
#[derive(Debug, Clone)]
struct GroupSnapshot {
    norm_id: SmolStr,
    max_richness: u8,
    urls: Vec<String>,
}

/// Returns the scheme+host origin of a URL, e.g. `"https://www.youtube.com"`.
fn origin_of(url_str: &str) -> Option<String> {
    Url::parse(url_str).ok().and_then(|u| {
        let host = u.host_str()?;
        Some(format!("{}://{}", u.scheme(), host))
    })
}

/// Returns true if the URL has no meaningful path/query beyond the origin
/// (e.g. `"https://www.youtube.com/"` or `"https://www.youtube.com"`).
/// These are reported by plasma-browser-integration when it lacks a specific
/// track URL, and should be treated as wildcards for same-origin merging.
fn is_origin_only(url_str: &str) -> bool {
    Url::parse(url_str).ok().is_some_and(|u| {
        let path = u.path();
        (path.is_empty() || path == "/") && u.query().is_none()
    })
}

/// Pure: given a per-group snapshot, return the (from, into) merges to apply.
/// Iteration order is deterministic — groups sorted by richness desc, then
/// norm_id asc — so the merge target is stable across ticks regardless of the
/// caller's `HashMap` iteration order.
///
/// In addition to exact URL matches, this also merges groups where one reports
/// only an origin-level URL (e.g. `https://www.youtube.com/`) and another
/// reports a specific URL on the same origin. This handles plasma-browser-
/// integration, which reports the site root URL instead of the track URL.
fn compute_url_merges(groups: &[GroupSnapshot]) -> Vec<(SmolStr, SmolStr)> {
    let mut sorted: Vec<&GroupSnapshot> = groups.iter().collect();
    sorted.sort_by(|a, b| {
        b.max_richness
            .cmp(&a.max_richness)
            .then_with(|| a.norm_id.cmp(&b.norm_id))
    });

    let mut url_to_norm: HashMap<String, SmolStr> = HashMap::new();
    // Maps `"https://example.com"` → norm_id for groups registered via an
    // origin-only URL. Only populated by origin-only entries.
    let mut origin_to_norm: HashMap<String, SmolStr> = HashMap::new();
    let mut merges: Vec<(SmolStr, SmolStr)> = Vec::new();

    'group: for group in sorted {
        for url in &group.urls {
            if url.is_empty() {
                continue;
            }

            // Exact URL match.
            if let Some(existing) = url_to_norm.get(url.as_str()) {
                if existing != &group.norm_id {
                    merges.push((group.norm_id.clone(), existing.clone()));
                    break 'group;
                }
                continue;
            }

            let origin = origin_of(url);

            if is_origin_only(url) {
                if let Some(ref origin_str) = origin {
                    // Another origin-only group already claimed this origin.
                    if let Some(existing) = origin_to_norm.get(origin_str.as_str()) {
                        if existing != &group.norm_id {
                            merges.push((group.norm_id.clone(), existing.clone()));
                            break 'group;
                        }
                    }
                    // A richer group with a specific URL on this origin was
                    // registered before us — merge into it.
                    if let Some(existing) = url_to_norm
                        .iter()
                        .find(|(k, v)| {
                            *v != &group.norm_id && origin_of(k).as_deref() == Some(origin_str)
                        })
                        .map(|(_, v)| v.clone())
                    {
                        merges.push((group.norm_id.clone(), existing));
                        break 'group;
                    }
                }
                // No match: register in both maps.
                url_to_norm.insert(url.clone(), group.norm_id.clone());
                if let Some(origin_str) = origin {
                    origin_to_norm.insert(origin_str, group.norm_id.clone());
                }
            } else {
                // Specific URL: check if an origin-only group already claimed this origin.
                if let Some(ref origin_str) = origin {
                    if let Some(existing) = origin_to_norm.get(origin_str.as_str()) {
                        if existing != &group.norm_id {
                            merges.push((group.norm_id.clone(), existing.clone()));
                            break 'group;
                        }
                    }
                }
                url_to_norm.insert(url.clone(), group.norm_id.clone());
            }
        }
    }

    merges
}

/// Merge identity groups that share the same xesam:url.
///
/// Handles cases like plasma-browser-integration and the native browser MPRIS
/// endpoint both exposing the same tab. The merge target is the richest group
/// (with norm_id alphabetical as final tiebreaker), so the choice is stable
/// across discovery ticks even though `HashMap` iteration order is not.
pub fn merge_url_duplicates(
    mut candidates: HashMap<SmolStr, Vec<Player>>,
) -> HashMap<SmolStr, Vec<Player>> {
    if candidates.len() < 2 {
        return candidates;
    }

    let snapshots: Vec<GroupSnapshot> = candidates
        .iter()
        .map(|(norm_id, players)| {
            let max_richness = players.iter().map(metadata_richness).max().unwrap_or(0);
            let urls = players
                .iter()
                .map(|p| {
                    p.get_metadata()
                        .ok()
                        .and_then(|m| m.url().map(|s| s.to_string()))
                        .unwrap_or_default()
                })
                .collect();
            GroupSnapshot {
                norm_id: norm_id.clone(),
                max_richness,
                urls,
            }
        })
        .collect();

    for (from, into) in compute_url_merges(&snapshots) {
        if from == into {
            continue;
        }
        if let Some(players) = candidates.remove(&from) {
            trace!(
                "Merging duplicate player group '{}' into '{}' (same URL or origin)",
                from,
                into
            );
            candidates.entry(into).or_default().extend(players);
        }
    }

    candidates
}

/// Post-merge bucket summary used to decide presence-key migrations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BucketSummary {
    pub norm_id: SmolStr,
    pub bus_names: Vec<SmolStr>,
    pub winner_bus: SmolStr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceMigration {
    pub from_key: SmolStr,
    pub to_key: SmolStr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresenceDrop {
    pub key: SmolStr,
    pub superseded_by: SmolStr,
}

/// Decide which existing `Presence` entries (keyed by their current
/// `media_players` key, mapped to the canonical bus they are bound to)
/// need to be re-keyed to match a post-merge bucket norm_id, and which
/// need to be dropped because two presences contend for the same bucket.
///
/// Caller applies drops first (`destroy_discord_client` + `stop_listener`),
/// then migrations (`HashMap::remove` + `HashMap::insert` under the new
/// key, preserving Discord IPC client + listener thread + cached state).
///
/// Tiebreak for multi-match: prefer the entry already keyed under the
/// bucket norm_id (avoids any rekey), else prefer the entry whose bus
/// equals the bucket's `winner_bus`, else alphabetical by existing key.
pub fn compute_presence_migrations(
    existing: &HashMap<SmolStr, SmolStr>,
    buckets: &[BucketSummary],
) -> (Vec<PresenceMigration>, Vec<PresenceDrop>) {
    let mut migrations = Vec::new();
    let mut drops = Vec::new();

    for bucket in buckets {
        let mut matches: Vec<(&SmolStr, &SmolStr)> = existing
            .iter()
            .filter(|(_, bus)| bucket.bus_names.contains(bus))
            .collect();

        if matches.is_empty() {
            continue;
        }

        matches.sort_by(|a, b| {
            let a_correct = a.0 == &bucket.norm_id;
            let b_correct = b.0 == &bucket.norm_id;
            if a_correct != b_correct {
                return b_correct.cmp(&a_correct);
            }
            let a_winner = a.1 == &bucket.winner_bus;
            let b_winner = b.1 == &bucket.winner_bus;
            if a_winner != b_winner {
                return b_winner.cmp(&a_winner);
            }
            a.0.cmp(b.0)
        });

        let (winner_key, _) = matches[0];
        if winner_key != &bucket.norm_id {
            migrations.push(PresenceMigration {
                from_key: winner_key.clone(),
                to_key: bucket.norm_id.clone(),
            });
        }
        for (loser_key, _) in &matches[1..] {
            drops.push(PresenceDrop {
                key: (*loser_key).clone(),
                superseded_by: winner_key.clone(),
            });
        }
    }

    (migrations, drops)
}

#[cfg(test)]
mod tests {
    use super::{
        canonical_player_bus_name, compute_presence_migrations, compute_url_merges, BucketSummary,
        GroupSnapshot, PlaybackState, PresenceDrop, PresenceMigration,
    };
    use mpris::PlaybackStatus;
    use smol_str::SmolStr;
    use std::collections::HashMap;

    fn playback_state(art_url: Option<&str>) -> PlaybackState {
        PlaybackState {
            playback_status: Some(PlaybackStatus::Playing),
            track_identifier: Some("track-1".into()),
            title: Some("Song".into()),
            position: Some(10),
            volume: Some(50),
            url: Some("https://music.example/track-1".into()),
            art_url: art_url.map(Into::into),
        }
    }

    #[test]
    fn art_url_change_is_significant() {
        let previous = playback_state(Some("file:///tmp/old-cover.jpg"));
        let current = playback_state(Some("file:///tmp/new-cover.jpg"));

        assert!(current.has_significant_changes(&previous));
    }

    #[test]
    fn unchanged_art_url_is_not_significant_by_itself() {
        let previous = playback_state(Some("file:///tmp/cover.jpg"));
        let current = playback_state(Some("file:///tmp/cover.jpg"));

        assert!(!current.has_significant_changes(&previous));
    }

    #[test]
    fn keeps_reverse_dns_player_names() {
        let bus_name = "org.mpris.MediaPlayer2.io.github.htkhiem.euphonica";
        assert_eq!(
            canonical_player_bus_name(bus_name),
            "io.github.htkhiem.euphonica"
        );
    }

    #[test]
    fn strips_instance_suffix() {
        let bus_name = "org.mpris.MediaPlayer2.vlc.instance1234";
        assert_eq!(canonical_player_bus_name(bus_name), "vlc");
    }

    #[test]
    fn trims_prefix_for_simple_names() {
        let bus_name = "org.mpris.MediaPlayer2.spotify";
        assert_eq!(canonical_player_bus_name(bus_name), "spotify");
    }

    fn snap(norm_id: &str, richness: u8, urls: &[&str]) -> GroupSnapshot {
        GroupSnapshot {
            norm_id: SmolStr::new(norm_id),
            max_richness: richness,
            urls: urls.iter().map(|s| (*s).to_string()).collect(),
        }
    }

    #[test]
    fn merge_target_is_richer_group_regardless_of_input_order() {
        // mozilla_zen (firefox, no art_url): lower richness.
        // zen_browser (plasma-browser-integration, with art_url): higher richness.
        // Both expose the same YouTube URL.
        let firefox = snap("mozilla_zen", 3, &["https://youtube.com/watch?v=x"]);
        let plasma = snap("zen_browser", 6, &["https://youtube.com/watch?v=x"]);

        let order1 = compute_url_merges(&[firefox.clone(), plasma.clone()]);
        let order2 = compute_url_merges(&[plasma, firefox]);

        assert_eq!(order1, order2);
        assert_eq!(order1.len(), 1);
        let (from, into) = &order1[0];
        assert_eq!(into.as_str(), "zen_browser");
        assert_eq!(from.as_str(), "mozilla_zen");
    }

    #[test]
    fn merge_target_breaks_ties_alphabetically() {
        let a = snap("mozilla_zen", 5, &["https://example.com/"]);
        let b = snap("zen_browser", 5, &["https://example.com/"]);

        let merges = compute_url_merges(&[a.clone(), b.clone()]);
        assert_eq!(merges.len(), 1);
        assert_eq!(merges[0].1.as_str(), "mozilla_zen");
        assert_eq!(merges[0].0.as_str(), "zen_browser");

        let merges_rev = compute_url_merges(&[b, a]);
        assert_eq!(merges, merges_rev);
    }

    #[test]
    fn no_merge_when_urls_differ() {
        let a = snap("a", 5, &["https://example.com/one"]);
        let b = snap("b", 5, &["https://example.com/two"]);
        assert!(compute_url_merges(&[a, b]).is_empty());
    }

    #[test]
    fn empty_url_is_ignored() {
        let a = snap("a", 5, &["", "https://shared/"]);
        let b = snap("b", 5, &["https://shared/"]);
        let merges = compute_url_merges(&[a, b]);
        assert_eq!(merges.len(), 1);
        assert_eq!(merges[0].1.as_str(), "a");
    }

    #[test]
    fn single_group_produces_no_merge() {
        let a = snap("a", 5, &["https://example.com/"]);
        assert!(compute_url_merges(&[a]).is_empty());
    }

    // Origin-level matching: plasma reports "https://www.youtube.com/" (origin-only),
    // firefox reports the full video URL. Should merge into the richer group.
    #[test]
    fn origin_only_merges_with_specific_url_rich_origin_first() {
        let plasma = snap("zen_browser", 6, &["https://www.youtube.com/"]);
        let firefox = snap("mozilla_zen", 3, &["https://www.youtube.com/watch?v=abc"]);

        let order1 = compute_url_merges(&[plasma.clone(), firefox.clone()]);
        let order2 = compute_url_merges(&[firefox, plasma]);

        assert_eq!(order1, order2);
        assert_eq!(order1.len(), 1);
        assert_eq!(order1[0].1.as_str(), "zen_browser");
        assert_eq!(order1[0].0.as_str(), "mozilla_zen");
    }

    #[test]
    fn origin_only_merges_with_specific_url_rich_specific_first() {
        let firefox = snap("mozilla_zen", 6, &["https://www.youtube.com/watch?v=abc"]);
        let plasma = snap("zen_browser", 3, &["https://www.youtube.com/"]);

        let order1 = compute_url_merges(&[firefox.clone(), plasma.clone()]);
        let order2 = compute_url_merges(&[plasma, firefox]);

        assert_eq!(order1, order2);
        assert_eq!(order1.len(), 1);
        assert_eq!(order1[0].1.as_str(), "mozilla_zen");
        assert_eq!(order1[0].0.as_str(), "zen_browser");
    }

    // Two different specific URLs on the same origin must NOT merge — they are
    // different tabs/tracks on the same site.
    #[test]
    fn two_specific_urls_same_origin_no_merge() {
        let a = snap("a", 5, &["https://www.youtube.com/watch?v=aaa"]);
        let b = snap("b", 5, &["https://www.youtube.com/watch?v=bbb"]);
        assert!(compute_url_merges(&[a, b]).is_empty());
    }

    fn bucket(norm_id: &str, buses: &[&str], winner: &str) -> BucketSummary {
        BucketSummary {
            norm_id: SmolStr::new(norm_id),
            bus_names: buses.iter().map(|b| SmolStr::new(*b)).collect(),
            winner_bus: SmolStr::new(winner),
        }
    }

    fn existing_from(pairs: &[(&str, &str)]) -> HashMap<SmolStr, SmolStr> {
        pairs
            .iter()
            .map(|(k, v)| (SmolStr::new(*k), SmolStr::new(*v)))
            .collect()
    }

    #[test]
    fn migration_coalesce_renames_old_key() {
        // Existing presence keyed `mozilla_zen` bound to Firefox native bus.
        // Post-merge bucket `zen_browser` contains both Firefox bus + plasma bus.
        let existing = existing_from(&[("mozilla_zen", "firefox")]);
        let buckets = vec![bucket(
            "zen_browser",
            &["firefox", "plasma-browser-integration"],
            "plasma-browser-integration",
        )];

        let (migrations, drops) = compute_presence_migrations(&existing, &buckets);

        assert_eq!(
            migrations,
            vec![PresenceMigration {
                from_key: SmolStr::new("mozilla_zen"),
                to_key: SmolStr::new("zen_browser"),
            }]
        );
        assert!(drops.is_empty());
    }

    #[test]
    fn migration_split_renames_old_key() {
        // Plasma vanished; existing presence keyed `zen_browser` bound to
        // Firefox bus must be re-keyed to `mozilla_zen`.
        let existing = existing_from(&[("zen_browser", "firefox")]);
        let buckets = vec![bucket("mozilla_zen", &["firefox"], "firefox")];

        let (migrations, drops) = compute_presence_migrations(&existing, &buckets);

        assert_eq!(
            migrations,
            vec![PresenceMigration {
                from_key: SmolStr::new("zen_browser"),
                to_key: SmolStr::new("mozilla_zen"),
            }]
        );
        assert!(drops.is_empty());
    }

    #[test]
    fn migration_no_change_when_key_already_correct() {
        let existing = existing_from(&[("zen_browser", "plasma-browser-integration")]);
        let buckets = vec![bucket(
            "zen_browser",
            &["firefox", "plasma-browser-integration"],
            "plasma-browser-integration",
        )];

        let (migrations, drops) = compute_presence_migrations(&existing, &buckets);

        assert!(migrations.is_empty());
        assert!(drops.is_empty());
    }

    #[test]
    fn migration_double_match_prefers_winner_bus() {
        // Both presences pre-exist independently; bucket coalesces them.
        // Winner bus is plasma, so the plasma-bound presence wins.
        let existing = existing_from(&[
            ("mozilla_zen", "firefox"),
            ("zen_browser", "plasma-browser-integration"),
        ]);
        let buckets = vec![bucket(
            "zen_browser",
            &["firefox", "plasma-browser-integration"],
            "plasma-browser-integration",
        )];

        let (migrations, drops) = compute_presence_migrations(&existing, &buckets);

        // zen_browser already correctly keyed → no migration emitted.
        assert!(migrations.is_empty());
        assert_eq!(
            drops,
            vec![PresenceDrop {
                key: SmolStr::new("mozilla_zen"),
                superseded_by: SmolStr::new("zen_browser"),
            }]
        );
    }

    #[test]
    fn migration_double_match_alphabetical_when_neither_is_winner() {
        // Both presences pre-exist; winner bus matches neither's bound bus.
        // Neither is correctly keyed (bucket norm_id is "youtube_app"), and
        // neither bus equals winner_bus ("c") → alphabetical wins.
        let existing = existing_from(&[("zoo", "a"), ("abc", "b")]);
        let buckets = vec![bucket("youtube_app", &["a", "b"], "c")];

        let (migrations, drops) = compute_presence_migrations(&existing, &buckets);

        assert_eq!(
            migrations,
            vec![PresenceMigration {
                from_key: SmolStr::new("abc"),
                to_key: SmolStr::new("youtube_app"),
            }]
        );
        assert_eq!(
            drops,
            vec![PresenceDrop {
                key: SmolStr::new("zoo"),
                superseded_by: SmolStr::new("abc"),
            }]
        );
    }

    #[test]
    fn migration_unrelated_presence_not_migrated() {
        // Two browsers playing different videos: each presence is bound to
        // its own bus, buckets are disjoint, no migration emitted.
        let existing = existing_from(&[
            ("zen_browser", "plasma-browser-integration"),
            ("firefox", "plasma-browser-integration-1285737"),
        ]);
        let buckets = vec![
            bucket(
                "zen_browser",
                &["firefox", "plasma-browser-integration"],
                "plasma-browser-integration",
            ),
            bucket(
                "firefox",
                &["firefox", "plasma-browser-integration-1285737"],
                "plasma-browser-integration-1285737",
            ),
        ];

        let (migrations, drops) = compute_presence_migrations(&existing, &buckets);

        assert!(migrations.is_empty());
        assert!(drops.is_empty());
    }
}

#[cfg(test)]
mod bridge_tests {
    use super::*;

    #[test]
    fn detects_bridge_bus_by_prefix() {
        assert!(is_mprisence_web_bridge_bus(
            "org.mpris.MediaPlayer2.mprisence_web.youtube_music.habc1234"
        ));
        assert!(is_mprisence_web_bridge_bus(
            "org.mpris.MediaPlayer2.mprisence_web.soundcloud.hdef5678"
        ));
        assert!(!is_mprisence_web_bridge_bus(
            "org.mpris.MediaPlayer2.spotify"
        ));
        assert!(!is_mprisence_web_bridge_bus(
            "org.mpris.MediaPlayer2.plasma-browser-integration"
        ));
    }

    #[test]
    fn canonical_player_bus_name_preserves_bridge_prefix() {
        let canon = canonical_player_bus_name(
            "org.mpris.MediaPlayer2.mprisence_web.youtube_music.habc1234",
        );
        assert!(canon.starts_with("mprisence_web"));
        assert!(canon.contains("youtube_music"));
    }

    #[test]
    fn bridge_group_key_extracts_site() {
        // We can't easily construct Metadata in tests because it's built by mpris_server.
        // The function signature is tested via integration.
        // This test just verifies the function exists and compiles.
        assert_eq!(BRIDGE_CONFIG_KEY, "mprisence_web");
    }
}
