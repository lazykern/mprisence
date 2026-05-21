use crate::protocol::{SourceState, Status};
use log::trace;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Default heartbeat timeout — source must send updates at least this often.
pub const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(5);
/// Source is removed if no update received for this long.
pub const STALE_TIMEOUT: Duration = Duration::from_secs(10);

/// Manages source state and selects the active source.
pub struct SourceRegistry {
    sources: HashMap<String, SourceState>,
    active_source_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionReason {
    Playing,
    Paused,
    None,
}

impl SourceRegistry {
    pub fn new() -> Self {
        Self {
            sources: HashMap::new(),
            active_source_id: None,
        }
    }

    /// Insert or update a source from an extension update message.
    pub fn upsert(&mut self, state: SourceState) {
        let id = state.source_id.clone();
        trace!(
            "source {}: status={:?} title={:?}",
            id,
            state.playback.status,
            state.metadata.title.as_deref().unwrap_or("(no title)")
        );
        self.sources.insert(id, state);
    }

    /// Remove a source (tab closed, navigation away, etc.).
    pub fn remove(&mut self, source_id: &str) {
        self.sources.remove(source_id);
        if self.active_source_id.as_deref() == Some(source_id) {
            self.active_source_id = None;
        }
    }

    /// Prune stale sources. Returns removed IDs.
    pub fn prune_stale(&mut self) -> Vec<String> {
        let mut removed = Vec::new();
        self.sources.retain(|id, state| {
            if state.is_stale(STALE_TIMEOUT) {
                trace!("source {id}: stale ({}s), removing", STALE_TIMEOUT.as_secs());
                removed.push(id.clone());
                false
            } else {
                true
            }
        });
        if !removed.is_empty() {
            if let Some(active) = &self.active_source_id {
                if removed.contains(active) {
                    self.active_source_id = None;
                }
            }
        }
        removed
    }

    /// Select the best source given current state.
    /// Returns the selected source and the reason.
    pub fn select_active(&mut self) -> (Option<&SourceState>, SelectionReason) {
        // Clone the ID first to avoid borrow conflicts with self.active_source_id
        let selected_id = self
            .find_best_playing()
            .map(|(id, _)| id.clone())
            .or_else(|| self.find_paused().map(|(id, _)| id.clone()));

        match selected_id {
            Some(id) => {
                let reason = if self.sources.get(&id).map_or(false, |s| s.is_playing()) {
                    SelectionReason::Playing
                } else {
                    SelectionReason::Paused
                };
                self.active_source_id = Some(id.clone());
                (self.sources.get(&id), reason)
            }
            None => {
                self.active_source_id = None;
                (None, SelectionReason::None)
            }
        }
    }

    fn find_best_playing(&self) -> Option<(&String, &SourceState)> {
        let now = Instant::now();
        self.sources
            .iter()
            .filter(|(_, s)| s.is_playing())
            .max_by_key(|(_, s)| {
                // Prefer sources with recent heartbeats and updates
                let recent = s.last_seen;
                let staleness = now.duration_since(recent);
                // Negate staleness so "less stale" = "higher key"
                // We use saturating_sub to avoid overflow if called from test context with simulated time
                u64::MAX.saturating_sub(staleness.as_millis() as u64)
            })
    }

    fn find_paused(&self) -> Option<(&String, &SourceState)> {
        self.sources
            .iter()
            .filter(|(_, s)| matches!(s.playback.status, Status::Paused))
            .max_by_key(|(_, s)| s.last_seen)
    }

    pub fn active_source_id(&self) -> Option<&str> {
        self.active_source_id.as_deref()
    }

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
    use super::*;
    use crate::protocol::{Capabilities, MediaMetadata, PlaybackState};

    fn make_source(id: &str, status: Status) -> SourceState {
        SourceState {
            source_id: id.to_string(),
            url: format!("https://example.com/{id}"),
            origin: "https://example.com".into(),
            site: "generic".into(),
            playback: PlaybackState {
                status,
                position_ms: 0,
                duration_ms: 100000,
                rate: 1.0,
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
                raise: false,
            },
            confidence: crate::protocol::ConfidenceLevel::Fallback,
            last_seen: Instant::now(),
        }
    }

    #[test]
    fn test_select_playing_over_paused() {
        let mut reg = SourceRegistry::new();
        reg.upsert(make_source("tab:1", Status::Paused));
        reg.upsert(make_source("tab:2", Status::Playing));

        let (source, reason) = reg.select_active();
        assert_eq!(reason, SelectionReason::Playing);
        assert_eq!(source.unwrap().source_id, "tab:2");
    }

    #[test]
    fn test_select_paused_when_no_playing() {
        let mut reg = SourceRegistry::new();
        reg.upsert(make_source("tab:1", Status::Paused));
        reg.upsert(make_source("tab:2", Status::Stopped));

        let (source, reason) = reg.select_active();
        assert_eq!(reason, SelectionReason::Paused);
        assert_eq!(source.unwrap().source_id, "tab:1");
    }

    #[test]
    fn test_no_source_when_empty() {
        let mut reg = SourceRegistry::new();
        let (_, reason) = reg.select_active();
        assert_eq!(reason, SelectionReason::None);
    }

    #[test]
    fn test_prune_stale_removes_old_sources() {
        let mut reg = SourceRegistry::new();
        let mut state = make_source("tab:1", Status::Playing);
        // Set last_seen far in the past
        state.last_seen = Instant::now() - STALE_TIMEOUT - Duration::from_secs(1);
        reg.upsert(state);

        let removed = reg.prune_stale();
        assert_eq!(removed.len(), 1);
        assert!(reg.is_empty());
    }
}
