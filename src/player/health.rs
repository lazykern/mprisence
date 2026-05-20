use std::time::{Duration, Instant};

use log::{debug, info, trace};
use mpris::{Metadata as MprisMetadata, PlaybackStatus};

use crate::metadata;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// How long position must be frozen before declaring a stall (general/native).
const STALLED_PLAYING_THRESHOLD_GENERAL: Duration = Duration::from_secs(8);

/// How long position must be frozen before declaring a stall (browser sources).
const STALLED_PLAYING_THRESHOLD_BROWSER: Duration = Duration::from_secs(4);

/// How long without any position movement before we declare a stall
/// for browser sources (catches tab-backgrounding where position never updates).
const BROWSER_SILENCE_TIMEOUT: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Track fingerprint (mirrors presence.rs so health.rs stays independent)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackFingerprint {
    pub track_id: Option<String>,
    pub url: Option<String>,
    pub art_url: Option<String>,
    pub title: Option<String>,
    pub artists: Vec<String>,
    pub length: Option<Duration>,
}

impl TrackFingerprint {
    pub fn from_mpris(metadata: &MprisMetadata) -> Self {
        Self {
            track_id: metadata.track_id().map(|id| id.to_string()),
            url: metadata.url().map(|url| url.to_string()),
            art_url: metadata.art_url().map(|url| url.to_string()),
            title: metadata.title().map(|title| title.to_string()),
            artists: metadata
                .artists()
                .map(|artists| artists.iter().map(|artist| artist.to_string()).collect())
                .unwrap_or_default(),
            length: metadata.length(),
        }
    }
}

// ---------------------------------------------------------------------------
// Art decision — returned alongside push outcomes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtDecision {
    pub source_options: metadata::ArtSourceOptions,
    pub read_cache: bool,
}

impl Default for ArtDecision {
    fn default() -> Self {
        Self {
            source_options: metadata::ArtSourceOptions::default(),
            read_cache: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Transition outcome
// ---------------------------------------------------------------------------

/// What action the caller should take after a transition.
#[derive(Debug)]
pub enum TransitionOutcome {
    /// Push activity to Discord with the given art decision.
    Push { art_decision: ArtDecision },
    /// Clear Discord activity (stall detected or stopped/paused).
    Clear,
    /// Nothing to do — state machine says skip this tick.
    Noop,
}

// ---------------------------------------------------------------------------
// Health check input
// ---------------------------------------------------------------------------

/// All the information the state machine needs to make a decision.
#[derive(Debug)]
pub struct HealthCheckInput<'a> {
    pub playback_status: PlaybackStatus,
    pub position: Duration,
    /// Track metadata (used by tests; track_length is the derived field for runtime checks).
    #[allow(dead_code)]
    pub track: &'a TrackFingerprint,
    pub track_length: Option<Duration>,
    pub is_browser_source: bool,
    /// The current update generation (monotonically increasing counter).
    pub generation: u64,
    /// Wall-clock time of this check.
    pub now: Instant,
    /// When the last MPRIS event was received (for silence detection).
    pub last_event: Instant,
}

// ---------------------------------------------------------------------------
// PlayerHealth state machine
// ---------------------------------------------------------------------------

/// Unified staleness state for one logical player.
///
/// Replaces the 6 scattered fields that previously tracked stale-Youtube-art,
/// stalled-playing-position, startup-playback-confirmation, etc.
#[derive(Debug, Clone)]
pub enum PlayerHealth {
    /// Awaiting first position move (browser sources).  Prevents the stale
    /// position from a previous session being pushed on initial discovery.
    Confirming {
        generation: u64,
        since: Instant,
    },

    /// Fresh data — normal operation.
    Healthy {
        generation: u64,
        last_event: Instant,
        /// Last known position, used to detect genuine position freeze
        /// (not just time-since-last-transition which can falsely fire
        /// when the poll interval exceeds the stall threshold).
        last_position: Duration,
    },

    /// Player reports `Playing` but position hasn't advanced for ≥ threshold,
    /// OR position ≥ track length.  Clears Discord activity until position
    /// moves, status changes, or a new track arrives.
    Stalled {
        generation: u64,
        since: Instant,
        last_position: Duration,
    },
}

impl PlayerHealth {
    // ------------------------------------------------------------------
    // Public helpers
    // ------------------------------------------------------------------

    /// Create a fresh `Healthy` state.
    pub fn healthy(generation: u64) -> Self {
        Self::Healthy {
            generation,
            last_event: Instant::now(),
            last_position: Duration::ZERO,
        }
    }

    /// Create a `Confirming` state (browser sources at startup).
    pub fn confirming(generation: u64) -> Self {
        Self::Confirming {
            generation,
            since: Instant::now(),
        }
    }

    /// Get the last known position stored in this state.
    fn last_position(&self) -> Duration {
        match self {
            Self::Healthy { last_position, .. }
            | Self::Stalled { last_position, .. } => *last_position,
            Self::Confirming { .. } => Duration::ZERO,
        }
    }

    /// Refresh liveness from a skipped poll tick without producing a Discord
    /// action. Normal position progress should keep browser sources from being
    /// considered "silent" just because nothing warranted a rich-presence push.
    pub fn observe_progress(&mut self, input: &HealthCheckInput) {
        if input.playback_status != PlaybackStatus::Playing {
            return;
        }

        match self {
            Self::Healthy {
                last_event,
                last_position,
                ..
            } => {
                if input.position != *last_position {
                    let previous_position = *last_position;
                    let age = input.now.saturating_duration_since(*last_event);
                    *last_event = input.now;
                    *last_position = input.position;
                    if age >= Duration::from_secs(10) {
                        debug!(
                            "health observe_progress: state=Healthy pos={:?}->{:?} refreshed_liveness=true age_ms={} generation={}",
                            previous_position,
                            input.position,
                            age.as_millis(),
                            input.generation,
                        );
                    } else {
                        trace!(
                            "health observe_progress: state=Healthy pos={:?}->{:?} refreshed_liveness=true age_ms={} generation={}",
                            previous_position,
                            input.position,
                            age.as_millis(),
                            input.generation,
                        );
                    }
                }
            }
            Self::Confirming { .. } | Self::Stalled { .. } => {}
        }
    }

    fn state_name(&self) -> &'static str {
        match self {
            Self::Confirming { .. } => "Confirming",
            Self::Healthy { .. } => "Healthy",
            Self::Stalled { .. } => "Stalled",
        }
    }

    fn log_transition(
        old: &Self,
        new: &Self,
        outcome: &TransitionOutcome,
        reason: &str,
        input: &HealthCheckInput,
        previous_position: Option<Duration>,
        stall_threshold: Duration,
    ) {
        let last_event_age = input.now.saturating_duration_since(input.last_event);
        let state_changed = old.state_name() != new.state_name();
        let level_is_info = matches!(outcome, TransitionOutcome::Clear) && state_changed;

        let msg = format!(
            "health transition: {} -> {} outcome={:?} reason={} pos={:?} prev_pos={:?} last_event_age_ms={} stall_threshold_ms={} browser_silence_timeout_ms={} generation={}",
            old.state_name(),
            new.state_name(),
            outcome,
            reason,
            input.position,
            previous_position,
            last_event_age.as_millis(),
            stall_threshold.as_millis(),
            BROWSER_SILENCE_TIMEOUT.as_millis(),
            input.generation,
        );

        if level_is_info {
            info!("{}", msg);
        } else if matches!(outcome, TransitionOutcome::Noop)
            || reason == "healthy_progress"
            || reason == "confirming_wait"
            || reason == "stalled_wait"
            || (!state_changed && reason == "playback_not_playing")
        {
            trace!("{}", msg);
        } else {
            debug!("{}", msg);
        }
    }

    /// Returns the `ArtDecision` for the current state (used when re-pushing
    /// without transitioning, e.g. on a background cover-fetch completion).
    pub fn art_decision(&self, _track: &TrackFingerprint) -> ArtDecision {
        ArtDecision::default()
    }

    // ------------------------------------------------------------------
    // Core transition
    // ------------------------------------------------------------------

    /// Run one transition step.  Caller passes current player state + event
    /// timestamp; the machine returns the action to take and updates itself.
    pub fn transition(&mut self, input: &HealthCheckInput) -> TransitionOutcome {
        let now = input.now;
        let stall_threshold = if input.is_browser_source {
            STALLED_PLAYING_THRESHOLD_BROWSER
        } else {
            STALLED_PLAYING_THRESHOLD_GENERAL
        };

        if input.playback_status != PlaybackStatus::Playing {
            let old = self.clone();
            let new = Self::Healthy {
                generation: input.generation,
                last_event: now,
                last_position: input.position,
            };
            let outcome = TransitionOutcome::Clear;
            Self::log_transition(
                &old,
                &new,
                &outcome,
                "playback_not_playing",
                input,
                Some(old.last_position()),
                stall_threshold,
            );
            *self = new;
            return outcome;
        }

        let old = std::mem::replace(
            self,
            Self::Healthy {
                generation: input.generation,
                last_event: now,
                last_position: input.position,
            },
        );
        let previous_position = old.last_position();
        let old_snapshot = old.clone();

        let (new_self, outcome, reason) = match old {
            Self::Confirming { generation, since } => {
                let elapsed = now.saturating_duration_since(since);
                let is_stalled = (!input.is_browser_source
                    && elapsed >= STALLED_PLAYING_THRESHOLD_GENERAL)
                    || (input.is_browser_source
                        && elapsed >= STALLED_PLAYING_THRESHOLD_BROWSER);

                if input.position > Duration::ZERO {
                    (
                        Self::Healthy {
                            generation,
                            last_event: now,
                            last_position: input.position,
                        },
                        TransitionOutcome::Push {
                            art_decision: ArtDecision::default(),
                        },
                        "confirming_position_moved",
                    )
                } else if is_stalled {
                    (
                        Self::Stalled {
                            generation,
                            since: now,
                            last_position: input.position,
                        },
                        TransitionOutcome::Clear,
                        "confirming_timeout",
                    )
                } else {
                    (
                        Self::Confirming { generation, since },
                        TransitionOutcome::Noop,
                        "confirming_wait",
                    )
                }
            }

            Self::Healthy {
                generation,
                last_event,
                last_position,
            } => {
                if Self::is_ended(input) {
                    (
                        Self::Stalled {
                            generation,
                            since: now,
                            last_position: input.position,
                        },
                        TransitionOutcome::Clear,
                        "track_ended",
                    )
                } else if input.is_browser_source && Self::is_silent(last_event, now) {
                    (
                        Self::Stalled {
                            generation,
                            since: now,
                            last_position: input.position,
                        },
                        TransitionOutcome::Clear,
                        "silent_timeout",
                    )
                } else if Self::is_frozen(
                    last_position,
                    input.position,
                    last_event,
                    now,
                    stall_threshold,
                ) {
                    (
                        Self::Stalled {
                            generation,
                            since: now,
                            last_position: input.position,
                        },
                        TransitionOutcome::Clear,
                        "position_frozen",
                    )
                } else {
                    (
                        Self::Healthy {
                            generation,
                            last_event: now,
                            last_position: input.position,
                        },
                        TransitionOutcome::Push {
                            art_decision: ArtDecision::default(),
                        },
                        "healthy_progress",
                    )
                }
            }

            Self::Stalled {
                generation,
                since,
                last_position,
            } => {
                if input.position > Duration::ZERO && input.position != last_position {
                    (
                        Self::Healthy {
                            generation,
                            last_event: now,
                            last_position: input.position,
                        },
                        TransitionOutcome::Push {
                            art_decision: ArtDecision::default(),
                        },
                        "stalled_recover_position_moved",
                    )
                } else {
                    (
                        Self::Stalled {
                            generation,
                            since,
                            last_position: input.position,
                        },
                        TransitionOutcome::Noop,
                        "stalled_wait",
                    )
                }
            }
        };

        Self::log_transition(
            &old_snapshot,
            &new_self,
            &outcome,
            reason,
            input,
            Some(previous_position),
            stall_threshold,
        );
        *self = new_self;
        outcome
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    /// True if position ≥ track length (song ended but player didn't signal it).
    fn is_ended(input: &HealthCheckInput) -> bool {
        input.track_length.is_some_and(|len| {
            input.position + Duration::from_secs(2) >= len
        })
    }

    /// True if the player has been silent (no events) for the browser
    /// silence timeout — catches backgrounded/hibernated tabs.
    fn is_silent(last_event: Instant, now: Instant) -> bool {
        now.saturating_duration_since(last_event) >= BROWSER_SILENCE_TIMEOUT
    }

    /// Returns true when the position hasn't changed AND the time since
    /// the last event exceeds the threshold. This requires BOTH conditions:
    /// a genuine position freeze, not just a poll interval > threshold.
    fn is_frozen(
        last_position: Duration,
        current_position: Duration,
        last_event: Instant,
        now: Instant,
        threshold: Duration,
    ) -> bool {
        let position_stuck = current_position == last_position;
        let time_exceeded = now.saturating_duration_since(last_event) >= threshold;
        position_stuck && time_exceeded
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn track(
        title: &str,
        artist: &str,
        url: &str,
        art_url: &str,
        track_id: &str,
    ) -> TrackFingerprint {
        TrackFingerprint {
            track_id: Some(track_id.to_string()),
            url: Some(url.to_string()),
            art_url: Some(art_url.to_string()),
            title: Some(title.to_string()),
            artists: vec![artist.to_string()],
            length: Some(Duration::from_secs(180)),
        }
    }

    fn make_input<'a>(
        status: PlaybackStatus,
        track: &'a TrackFingerprint,
        position: Duration,
        is_browser: bool,
        gen: u64,
        now: Instant,
        last_event: Instant,
    ) -> HealthCheckInput<'a> {
        HealthCheckInput {
            playback_status: status,
            position,
            track,
            track_length: track.length,
            is_browser_source: is_browser,
            generation: gen,
            now,
            last_event,
        }
    }

    // ------------------------------------------------------------------
    // Confirming → Healthy (first position move)
    // ------------------------------------------------------------------
    #[test]
    fn confirming_transitions_to_healthy_on_position_move() {
        let mut health = PlayerHealth::Confirming {
            generation: 1,
            since: Instant::now(),
        };
        let t = track("Song", "Artist", "https://youtube.com/watch?v=abc", "", "id1");
        let now = Instant::now();
        let inp = make_input(
            PlaybackStatus::Playing,
            &t,
            Duration::from_secs(5),
            true,
            1,
            now,
            now,
        );

        let outcome = health.transition(&inp);
        assert!(matches!(outcome, TransitionOutcome::Push { .. }));
        assert!(matches!(health, PlayerHealth::Healthy { .. }));
    }

    #[test]
    fn confirming_stays_confirming_when_position_zero() {
        let mut health = PlayerHealth::Confirming {
            generation: 1,
            since: Instant::now(),
        };
        let t = track("Song", "Artist", "https://youtube.com/watch?v=abc", "", "id1");
        let now = Instant::now();
        let inp = make_input(
            PlaybackStatus::Playing,
            &t,
            Duration::ZERO,
            true,
            1,
            now,
            now,
        );

        let outcome = health.transition(&inp);
        assert!(matches!(outcome, TransitionOutcome::Noop));
        assert!(matches!(health, PlayerHealth::Confirming { .. }));
    }

    #[test]
    fn confirming_transitions_to_stalled_on_timeout() {
        let mut health = PlayerHealth::Confirming {
            generation: 1,
            since: Instant::now() - STALLED_PLAYING_THRESHOLD_BROWSER - Duration::from_secs(1),
        };
        let t = track("Song", "Artist", "https://youtube.com/watch?v=abc", "", "id1");
        let now = Instant::now();
        let inp = make_input(
            PlaybackStatus::Playing,
            &t,
            Duration::ZERO,
            true,
            1,
            now,
            now,
        );

        let outcome = health.transition(&inp);
        assert!(matches!(outcome, TransitionOutcome::Clear));
        assert!(matches!(health, PlayerHealth::Stalled { .. }));
    }

    // ------------------------------------------------------------------
    // Healthy → Stalled (position frozen)
    // ------------------------------------------------------------------
    #[test]
    fn healthy_native_stalls_after_threshold() {
        let mut health = PlayerHealth::Healthy {
            generation: 1,
            last_event: Instant::now() - STALLED_PLAYING_THRESHOLD_GENERAL
                - Duration::from_secs(1),
            last_position: Duration::from_secs(22), // same as current → frozen
        };
        let t = track("Song", "Artist", "https://example.com/stream", "", "id1");
        let now = Instant::now();
        let inp = make_input(
            PlaybackStatus::Playing,
            &t,
            Duration::from_secs(22),
            false,
            1,
            now,
            now,
        );

        let outcome = health.transition(&inp);
        assert!(matches!(outcome, TransitionOutcome::Clear));
        assert!(matches!(health, PlayerHealth::Stalled { .. }));
    }

    #[test]
    fn healthy_does_not_stall_before_threshold() {
        let mut health = PlayerHealth::Healthy {
            generation: 1,
            last_event: Instant::now(),
            last_position: Duration::from_secs(22), // same as current but time hasn't elapsed
        };
        let t = track("Song", "Artist", "https://example.com/stream", "", "id1");
        let now = Instant::now();
        let inp = make_input(
            PlaybackStatus::Playing,
            &t,
            Duration::from_secs(22),
            false,
            1,
            now,
            now,
        );

        let outcome = health.transition(&inp);
        assert!(matches!(outcome, TransitionOutcome::Push { .. }));
        assert!(matches!(health, PlayerHealth::Healthy { .. }));
    }

    #[test]
    fn healthy_browser_stalls_after_4s() {
        let mut health = PlayerHealth::Healthy {
            generation: 1,
            last_event: Instant::now() - STALLED_PLAYING_THRESHOLD_BROWSER
                - Duration::from_secs(1),
            last_position: Duration::from_secs(22), // same as current → frozen
        };
        let t = track("Song", "Artist", "https://youtube.com/watch?v=abc", "", "id1");
        let now = Instant::now();
        let inp = make_input(
            PlaybackStatus::Playing,
            &t,
            Duration::from_secs(22),
            true,
            1,
            now,
            now,
        );

        let outcome = health.transition(&inp);
        assert!(matches!(outcome, TransitionOutcome::Clear));
        assert!(matches!(health, PlayerHealth::Stalled { .. }));
    }

    // ------------------------------------------------------------------
    // Healthy → Stalled immediately (position ≥ length)
    // ------------------------------------------------------------------
    #[test]
    fn healthy_stalls_on_ended_song() {
        let mut health = PlayerHealth::Healthy {
            generation: 1,
            last_event: Instant::now(),
            last_position: Duration::ZERO,
        };
        let mut t = track("Song", "Artist", "https://youtube.com/watch?v=abc", "", "id1");
        t.length = Some(Duration::from_secs(180));
        let now = Instant::now();
        let inp = make_input(
            PlaybackStatus::Playing,
            &t,
            Duration::from_secs(179), // within 2s of end
            false,
            1,
            now,
            now,
        );

        let outcome = health.transition(&inp);
        assert!(matches!(outcome, TransitionOutcome::Clear));
        assert!(matches!(health, PlayerHealth::Stalled { .. }));
    }

    #[test]
    fn healthy_not_stalled_when_far_from_end() {
        let mut health = PlayerHealth::Healthy {
            generation: 1,
            last_event: Instant::now(),
            last_position: Duration::ZERO,
        };
        let mut t = track("Song", "Artist", "https://youtube.com/watch?v=abc", "", "id1");
        t.length = Some(Duration::from_secs(180));
        let now = Instant::now();
        let inp = make_input(
            PlaybackStatus::Playing,
            &t,
            Duration::from_secs(100),
            false,
            1,
            now,
            now,
        );

        let outcome = health.transition(&inp);
        assert!(matches!(outcome, TransitionOutcome::Push { .. }));
        assert!(matches!(health, PlayerHealth::Healthy { .. }));
    }

    // ------------------------------------------------------------------
    // Stalled → Healthy (position advances)
    // ------------------------------------------------------------------
    #[test]
    fn stalled_recovers_on_position_change() {
        let mut health = PlayerHealth::Stalled {
            generation: 1,
            since: Instant::now(),
            last_position: Duration::from_secs(10), // stalled at 10s
        };
        let t = track("Song", "Artist", "https://example.com/stream", "", "id1");
        let now = Instant::now();
        let inp = make_input(
            PlaybackStatus::Playing,
            &t,
            Duration::from_secs(23), // advanced to 23s
            false,
            1,
            now,
            now,
        );

        let outcome = health.transition(&inp);
        assert!(matches!(outcome, TransitionOutcome::Push { .. }));
        assert!(matches!(health, PlayerHealth::Healthy { .. }));
    }

    #[test]
    fn stalled_stays_stalled_when_position_unchanged() {
        let mut health = PlayerHealth::Stalled {
            generation: 1,
            since: Instant::now(),
            last_position: Duration::from_secs(42), // stalled at 42s
        };
        let t = track("Song", "Artist", "https://example.com/stream", "", "id1");
        let now = Instant::now();
        let inp = make_input(
            PlaybackStatus::Playing,
            &t,
            Duration::from_secs(42), // same position — no recovery
            false,
            1,
            now,
            now,
        );

        let outcome = health.transition(&inp);
        assert!(matches!(outcome, TransitionOutcome::Noop));
        assert!(matches!(health, PlayerHealth::Stalled { .. }));
    }

    // ------------------------------------------------------------------
    // All states → Healthy when playback is not Playing
    // ------------------------------------------------------------------
    #[test]
    fn any_state_goes_healthy_on_pause() {
        for mut health in vec![
            PlayerHealth::Healthy {
                generation: 1,
                last_event: Instant::now(),
                last_position: Duration::ZERO,
            },
            PlayerHealth::Stalled {
                generation: 1,
                since: Instant::now(),
                last_position: Duration::ZERO,
            },
        ] {
            let t = track("Song", "Artist", "https://youtube.com/watch?v=abc", "", "id1");
            let now = Instant::now();
            let inp = make_input(
                PlaybackStatus::Paused,
                &t,
                Duration::from_secs(22),
                false,
                1,
                now,
                now,
            );
            let outcome = health.transition(&inp);
            assert!(
                matches!(outcome, TransitionOutcome::Clear),
                "expected Clear, got {:?}",
                outcome
            );
            assert!(matches!(health, PlayerHealth::Healthy { .. }));
        }
    }

    #[test]
    fn any_state_goes_healthy_on_stop() {
        for mut health in vec![
            PlayerHealth::Healthy {
                generation: 1,
                last_event: Instant::now(),
                last_position: Duration::ZERO,
            },
            PlayerHealth::Stalled {
                generation: 1,
                since: Instant::now(),
                last_position: Duration::ZERO,
            },
        ] {
            let t = track("Song", "Artist", "https://youtube.com/watch?v=abc", "", "id1");
            let now = Instant::now();
            let inp = make_input(
                PlaybackStatus::Stopped,
                &t,
                Duration::from_secs(22),
                false,
                1,
                now,
                now,
            );
            let outcome = health.transition(&inp);
            assert!(
                matches!(outcome, TransitionOutcome::Clear),
                "expected Clear, got {:?}",
                outcome
            );
            assert!(matches!(health, PlayerHealth::Healthy { .. }));
        }
    }

    // ------------------------------------------------------------------
    // Browser silence timeout
    // ------------------------------------------------------------------
    #[test]
    fn browser_silence_triggers_stall() {
        let mut health = PlayerHealth::Healthy {
            generation: 1,
            last_event: Instant::now() - BROWSER_SILENCE_TIMEOUT - Duration::from_secs(1),
            last_position: Duration::ZERO,
        };
        let t = track("Song", "Artist", "https://youtube.com/watch?v=abc", "", "id1");
        let now = Instant::now();
        let inp = make_input(
            PlaybackStatus::Playing,
            &t,
            Duration::from_secs(50),
            true,
            1,
            now,
            now,
        );

        let outcome = health.transition(&inp);
        assert!(matches!(outcome, TransitionOutcome::Clear));
        assert!(matches!(health, PlayerHealth::Stalled { .. }));
    }

    #[test]
    fn native_does_not_stall_on_silence() {
        let mut health = PlayerHealth::Healthy {
            generation: 1,
            last_event: Instant::now() - BROWSER_SILENCE_TIMEOUT - Duration::from_secs(1),
            last_position: Duration::from_secs(50), // same as current → frozen by is_frozen, not silence
        };
        let t = track("Song", "Artist", "https://example.com/stream", "", "id1");
        let now = Instant::now();
        let inp = make_input(
            PlaybackStatus::Playing,
            &t,
            Duration::from_secs(50),
            false, // not a browser source
            1,
            now,
            now,
        );

        let outcome = health.transition(&inp);
        // Native sources don't have silence timeout, but the is_frozen check
        // still applies with the 8s threshold. Since last_event is old, it should stall.
        assert!(matches!(outcome, TransitionOutcome::Clear));
        assert!(matches!(health, PlayerHealth::Stalled { .. }));
    }

    // ------------------------------------------------------------------
    // ArtDecision behaviour
    // ------------------------------------------------------------------
    #[test]
    fn art_decision_default_is_normal() {
        let decision = ArtDecision::default();
        assert!(decision.read_cache);
        assert!(decision.source_options.allow_mpris_art_url);
    }

    #[test]
    fn art_decision_healthy_returns_default() {
        let health = PlayerHealth::Healthy {
            generation: 1,
            last_event: Instant::now(),
            last_position: Duration::ZERO,
        };
        let t = track("Song", "Artist", "https://example.com", "", "id1");
        let decision = health.art_decision(&t);
        assert!(decision.read_cache);
        assert!(decision.source_options.allow_mpris_art_url);
    }

}
