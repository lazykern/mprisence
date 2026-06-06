use super::protocol::SourceState;
use log::{debug, trace};
use std::collections::HashMap;
use std::time::Duration;

/// A source is removed if no update has been received for this long. Must
/// comfortably exceed the extension's keepalive cadence, which the browser
/// throttles to ~once/minute in backgrounded tabs.
pub const STALE_TIMEOUT: Duration = Duration::from_secs(90);

/// Holds one `SourceState` per browser tab. No arbitration: every source is
/// published as its own MPRIS player.
pub struct SourceRegistry {
    sources: HashMap<String, SourceState>,
}

impl SourceRegistry {
    pub fn new() -> Self {
        Self {
            sources: HashMap::new(),
        }
    }

    /// Insert or update a source from an extension update message.
    pub fn upsert(&mut self, state: SourceState) {
        trace!(
            "source {}: status={:?} title={:?}",
            state.source_id,
            state.playback.status,
            state.metadata.title.as_deref().unwrap_or("(no title)")
        );
        self.sources.insert(state.source_id.clone(), state);
    }

    /// Remove a source (tab closed, navigation away, etc.).
    pub fn remove(&mut self, source_id: &str) {
        self.sources.remove(source_id);
    }

    /// Prune sources with no recent update. Returns removed IDs.
    pub fn prune_stale(&mut self) -> Vec<String> {
        let mut removed = Vec::new();
        self.sources.retain(|id, state| {
            if state.is_stale(STALE_TIMEOUT) {
                debug!(
                    "source {id}: stale ({}s), removing",
                    STALE_TIMEOUT.as_secs()
                );
                removed.push(id.clone());
                false
            } else {
                true
            }
        });
        removed
    }

    /// Get a specific source by ID.
    pub fn get(&self, source_id: &str) -> Option<&SourceState> {
        self.sources.get(source_id)
    }

    #[allow(dead_code)]
    pub fn source_count(&self) -> usize {
        self.sources.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::super::protocol::{Capabilities, MediaMetadata, PlaybackState, Status};
    use super::*;
    use std::time::Instant;

    fn make_source(id: &str, status: Status) -> SourceState {
        SourceState {
            source_id: id.to_string(),
            url: format!("https://example.com/{id}"),
            origin: "https://example.com".into(),
            site: "generic".into(),
            playback: PlaybackState {
                status,
                position_ms: 0,
                duration_ms: 100_000,
            },
            metadata: MediaMetadata {
                title: Some("Test".into()),
                artist: vec![],
                album: None,
                album_artist: vec![],
                art_url: None,
                track_id: None,
            },
            capabilities: Capabilities {
                play_pause: true,
                next: false,
                previous: false,
                seek: false,
                set_position: false,
            },
            last_seen: Instant::now(),
            canonical_url: None,
        }
    }

    #[test]
    fn upsert_then_get_returns_source() {
        let mut reg = SourceRegistry::new();
        reg.upsert(make_source("firefox:tab:1:0", Status::Playing));
        assert!(reg.get("firefox:tab:1:0").is_some());
        assert_eq!(reg.source_count(), 1);
    }

    #[test]
    fn upsert_same_id_replaces() {
        let mut reg = SourceRegistry::new();
        reg.upsert(make_source("firefox:tab:1:0", Status::Playing));
        reg.upsert(make_source("firefox:tab:1:0", Status::Paused));
        assert_eq!(reg.source_count(), 1);
    }

    #[test]
    fn remove_drops_source() {
        let mut reg = SourceRegistry::new();
        reg.upsert(make_source("firefox:tab:1:0", Status::Playing));
        reg.remove("firefox:tab:1:0");
        assert!(reg.is_empty());
    }

    #[test]
    fn prune_stale_removes_old_sources() {
        let mut reg = SourceRegistry::new();
        let mut state = make_source("firefox:tab:1:0", Status::Playing);
        state.last_seen = Instant::now() - STALE_TIMEOUT - Duration::from_secs(1);
        reg.upsert(state);

        let removed = reg.prune_stale();
        assert_eq!(removed, vec!["firefox:tab:1:0".to_string()]);
        assert!(reg.is_empty());
    }

    #[test]
    fn prune_stale_keeps_fresh_sources() {
        let mut reg = SourceRegistry::new();
        reg.upsert(make_source("firefox:tab:1:0", Status::Playing));
        assert!(reg.prune_stale().is_empty());
        assert_eq!(reg.source_count(), 1);
    }
}
