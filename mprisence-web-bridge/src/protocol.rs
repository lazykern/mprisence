use serde::{Deserialize, Serialize};

/// Protocol version
pub const PROTOCOL_VERSION: u32 = 1;

// ─── Extension → Bridge ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ExtMessage {
    #[serde(rename = "hello")]
    Hello {
        browser: BrowserKind,
        extension_version: String,
        protocol: u32,
        git_sha: Option<String>,
        #[serde(default)]
        extension_fingerprint: Option<String>,
    },
    #[serde(rename = "update")]
    Update {
        source_id: String,
        url: String,
        origin: String,
        site: String,
        playback: PlaybackState,
        metadata: MediaMetadata,
        capabilities: Capabilities,
        #[serde(default = "default_confidence")]
        confidence: ConfidenceLevel,
        /// Best canonical track/page URL from the provider.
        /// Takes priority over `url` for MPRIS `xesam:url` and website matching.
        #[serde(default)]
        canonical_url: Option<String>,
        /// Build fingerprint from extension content script.
        #[serde(default)]
        _ext_fingerprint: Option<String>,
    },
    #[serde(rename = "remove")]
    Remove {
        source_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserKind {
    Firefox,
    Chromium,
    Brave,
    Vivaldi,
    Edge,
    #[serde(untagged)]
    Other(String),
}

fn default_confidence() -> ConfidenceLevel {
    ConfidenceLevel::Fallback
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Playing,
    Paused,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaybackState {
    pub status: Status,
    pub position_ms: u64,
    pub duration_ms: u64,
    #[serde(default = "default_rate")]
    pub rate: f64,
}

fn default_rate() -> f64 {
    1.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaMetadata {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub artist: Vec<String>,
    #[serde(default)]
    pub album: Option<String>,
    #[serde(default)]
    pub album_artist: Vec<String>,
    #[serde(default)]
    pub art_url: Option<String>,
    #[serde(default)]
    pub track_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capabilities {
    #[serde(default = "default_true")]
    pub play_pause: bool,
    #[serde(default)]
    pub next: bool,
    #[serde(default)]
    pub previous: bool,
    #[serde(default)]
    pub seek: bool,
    #[serde(default)]
    pub set_position: bool,
    #[serde(default)]
    pub raise: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfidenceLevel {
    Provider,
    Dom,
    Fallback,
}

// ─── Bridge → Extension ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum BridgeMessage {
    #[serde(rename = "hello")]
    Hello {
        bridge_version: String,
        protocol: u32,
        git_sha: Option<String>,
    },
    #[serde(rename = "command")]
    Command {
        source_id: String,
        command: CommandKind,
        #[serde(default)]
        position_ms: Option<u64>,
    },
    #[serde(rename = "heartbeat")]
    Heartbeat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandKind {
    PlayPause,
    Play,
    Pause,
    Next,
    Previous,
    Seek,
    SetPosition,
    Raise,
}

// ─── Bridge Internal State ────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct SourceState {
    pub source_id: String,
    pub url: String,
    #[allow(dead_code)]
    pub origin: String,
    pub site: String,
    pub playback: PlaybackState,
    pub metadata: MediaMetadata,
    pub capabilities: Capabilities,
    #[allow(dead_code)]
    pub confidence: ConfidenceLevel,
    pub last_seen: std::time::Instant,
    /// Best canonical URL from the provider (track page, not mini-player).
    /// Falls back to page URL if provider doesn't supply one.
    pub canonical_url: Option<String>,
}

impl SourceState {
    pub fn is_stale(&self, timeout: std::time::Duration) -> bool {
        self.last_seen.elapsed() > timeout
    }

    pub fn is_playing(&self) -> bool {
        matches!(self.playback.status, Status::Playing)
    }
}


