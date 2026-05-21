# mprisence Web Bridge — Design

## Overview

Browser MPRIS export is unreliable — often missing album art, exposing generic URLs (miniplayer returns `https://music.youtube.com/`), or providing no metadata at all. The web bridge solves this by injecting a browser extension that extracts high-quality metadata directly from the page DOM and sends it to a native host that publishes a clean virtual MPRIS player. mprisence consumes it like any other MPRIS source.

## Architecture

```
┌─────────────────────────────────────────────────┐
│ Browser (Firefox / Chromium)                     │
│                                                  │
│  ┌──────────────────────────────────────────┐   │
│  │ Extension                                 │   │
│  │                                           │   │
│  │  content.ts (page world)                  │   │
│  │    └─ provider: YouTubeMusicProvider       │   │
│  │       └─ extracts metadata, art, playback  │   │
│  │                                           │   │
│  │  background.ts (service worker)           │   │
│  │    └─ native messaging port               │   │
│  │       └─ forwards updates → bridge        │   │
│  │       └─ receives commands ← bridge       │   │
│  └──────────────────────────────────────────┘   │
│                                                  │
│  ←── native messaging (stdin/stdout) ──────────→ │
└──────────────────────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────┐
│ mprisence-web-bridge (per-browser process)        │
│                                                    │
│  native_messaging.rs  ←  JSON from extension      │
│       │                                            │
│  active_source.rs  →  arbitrates between tabs     │
│       │                                            │
│  mpris.rs  →  publishes org.mpris.MediaPlayer2.*  │
│                                                    │
│  ←── D-Bus session bus ──────────────────────────→ │
└──────────────────────────────────────────────────┘
                       │
                       ▼
┌──────────────────────────────────────────────────┐
│ mprisence                                         │
│  unchanged — consumes MPRIS as always              │
│  player config: [player.mprisence_web_firefox]    │
│  website override: same as other browser players  │
└──────────────────────────────────────────────────┘
```

## Workspace Layout

```
mprisence/
├── Cargo.toml                  # workspace root [workspace] + existing mprisence crate
├── src/                        # existing mprisence source (unchanged)
├── mprisence-web-bridge/       # new bridge crate
│   ├── Cargo.toml
│   └── src/
│       ├── main.rs             # entry: CLI (install/uninstall/doctor/run)
│       ├── native_messaging.rs # stdin/stdout JSON reader/writer
│       ├── mpris.rs            # D-Bus MPRIS player interface
│       ├── active_source.rs    # source arbitration logic
│       └── protocol.rs         # shared message types
├── extension/                  # WebExtension
│   ├── package.json
│   ├── tsconfig.json
│   ├── build.mjs               # esbuild script
│   ├── manifest.firefox.json
│   ├── manifest.chromium.json
│   └── src/
│       ├── background.ts
│       ├── content.ts
│       ├── types.ts
│       ├── providers/
│       │   ├── base.ts
│       │   ├── generic-media.ts
│       │   └── youtube-music.ts
│       └── utils/
│           ├── native-messaging.ts
│           └── browser-detect.ts
└── docs/
    └── web-bridge-design.md    # this file
```

## Protocol

### Extension → Bridge

```typescript
// Connection init
interface Hello {
  type: "hello";
  browser: "firefox" | "chromium";
  extension_version: string;
}

// Full source state update (sent on any change)
interface Update {
  type: "update";
  source_id: string;              // "firefox:tab:123"
  url: string;                    // page URL
  origin: string;                 // "https://music.youtube.com"
  site: string;                   // "youtube_music" | "generic" | ...
  playback: {
    status: "playing" | "paused" | "stopped";
    position_ms: number;
    duration_ms: number;
    rate: number;                 // playback rate (1.0 = normal)
  };
  metadata: {
    title?: string;
    artist?: string[];            // first = primary
    album?: string;
    album_artist?: string[];
    art_url?: string;             // resolved HTTPS URL or data:base64
    track_id?: string;            // provider-specific stable ID
  };
  capabilities: {
    play_pause: boolean;
    next: boolean;
    previous: boolean;
    seek: boolean;
    set_position: boolean;
    raise: boolean;               // focus tab
  };
  confidence: "provider" | "dom" | "fallback";  // metadata quality
}

// Source removed (tab closed)
interface Remove {
  type: "remove";
  source_id: string;
}
```

### Bridge → Extension

```typescript
// Command from bridge to extension (user clicked MPRIS control)
interface Command {
  type: "command";
  source_id: string;
  command: "play_pause" | "next" | "previous" | "seek" | "set_position" | "raise";
  position_ms?: number;         // for seek/set_position
}

// Bridge hello (sent after receiving extension hello)
interface BridgeHello {
  type: "hello";
  bridge_version: string;
  protocol: 1;
}

// Heartbeat probe
interface Heartbeat {
  type: "heartbeat";
}
```

### Wire Format

Native messaging wraps each message as a 4-byte length prefix (uint32 LE) followed by UTF-8 JSON. Standard native messaging framing for both Firefox and Chromium.

## Bridge Design

### State

```rust
struct Bridge {
    sources: HashMap<String, SourceState>,
    active_source_id: Option<String>,
    mpris_player: MprisPlayer,
    heartbeat_deadline: Instant,
}

struct SourceState {
    source_id: String,
    url: String,
    origin: String,
    site: String,
    playback: PlaybackState,
    metadata: MediaMetadata,
    capabilities: Capabilities,
    confidence: ConfidenceLevel,
    last_seen: Instant,
}
```

### Active Source Selection

Priority (highest wins):
1. Playing source with recent heartbeat (< 5s)
2. Source whose position is actively advancing
3. Most recently user-controlled source (tab focus or command)
4. Most recently audible tab (last update received)
5. Paused source if no playing source exists
6. No source → publish Stopped

Staleness:
- No update for 5s → treat as stale, re-evaluate selection
- No update for 10s → remove source entirely
- All sources removed → publish Stopped or unpublish

### MPRIS Mapping

| MPRIS Property | Source Field |
|---|---|
| `PlaybackStatus` | `playback.status` |
| `Position` | `playback.position_ms * 1000` (microseconds) |
| `mpris:length` | `playback.duration_ms * 1000` |
| `mpris:trackid` | hash of `track_id` or `source_id + metadata.title + metadata.artist` |
| `mpris:artUrl` | `metadata.art_url` (passed through; mprisence fetches it) |
| `xesam:title` | `metadata.title` |
| `xesam:artist` | `metadata.artist` |
| `xesam:album` | `metadata.album` |
| `xesam:albumArtist` | `metadata.album_artist` |
| `xesam:url` | `url` (page URL) |
| `xesam:sourceUrl` | `origin` |
| `Identity` | formatted from `site`, e.g. "YouTube Music" |
| `DesktopEntry` | (empty — browser already owns this) |
| `CanRaise` | `capabilities.raise` |
| `CanGoNext` | `capabilities.next` |
| `CanGoPrevious` | `capabilities.previous` |
| `CanSeek` | `capabilities.seek` |
| `CanPlay` | `capabilities.play_pause` (if paused) |
| `CanPause` | `capabilities.play_pause` (if playing) |

### MPRIS Bus Names

Per-browser instance to avoid collisions:

| Browser | MPRIS Bus Name |
|---|---|
| Firefox | `org.mpris.MediaPlayer2.mprisence_web_firefox` |
| Chromium | `org.mpris.MediaPlayer2.mprisence_web_chromium` |
| Brave | `org.mpris.MediaPlayer2.mprisence_web_brave` |

Override via `--mpris-name` flag.

## Extension Design

### Build System

- **esbuild** for TypeScript compilation
- Build script (`build.mjs`) reads `manifest.{browser}.json`, generates output in `dist/{browser}/`
- One npm script: `build:firefox`, `build:chromium`, or `build:all`

### Manifest Strategy

Shared fields in a base config. Per-browser variants override:
- `browser_specific_settings` / `minimum_chrome_version`
- `background` (service_worker for Chromium, scripts/type:module for Firefox)
- Permissions (host_permissions vs permissions)

### Provider Interface

```typescript
interface Provider {
  /** Check if this provider handles the given URL */
  matches(url: URL): boolean;

  /** Extract metadata from page */
  getMetadata(): Metadata | null;

  /** Get current playback state */
  getPlayback(): PlaybackState | null;

  /** Get current album art URL (prefer HTTPS, resolve blobs) */
  getArtUrl(): string | null;

  /** Execute a media control command */
  command(cmd: Command): Promise<void>;
}
```

### Content Script Architecture

```
content.ts (main world via injection)
  └─ provider dispatcher
       ├─ YouTubeMusicProvider  (matches music.youtube.com)
       ├─ GenericMediaProvider  (matches any page with <audio>/<video>)
       └─ ... (future providers)

Provider sends updates via CustomEvent to content script wrapper
Content script wrapper forwards to background via chrome.runtime.sendMessage
Background forwards to native host via connectNative port
```

CSP-safe approach (static script + CustomEvent, not dynamic eval):
- Content script injects a static `<script>` element into page world
- Page-world script uses DOM + MediaSession API directly
- Sends data back to content script via `window.postMessage` or CustomEvent
- Content script relay → background → bridge

### Passive Page-Side Detection

The page-world script uses `MutationObserver` + `timeupdate` event on media elements + `navigator.mediaSession` metadata changes. It does NOT poll aggressively. For YouTube Music specifically, it observes DOM changes in the player bar area.

## Data Flow: Track Change

```
1. User clicks next track on YouTube Music
2. Page-world script detects DOM change (new title element)
3. Page-world script reads title, artist, album, art URL from DOM
4. Page-world posts data to content script via CustomEvent
5. Content script forwards to background script
6. Background script sends { type: "update", ... } JSON to bridge
7. Bridge receives update, stores in source registry
8. Bridge re-evaluates active source (this source is playing → winner)
9. Bridge updates MPRIS properties on D-Bus
10. mprisence detects mpris:trackid change via D-Bus PropertiesChanged
11. mprisence fetches art_url, builds Discord presence
```

## Development Phases

### Phase 0: Workspace + Design (this doc)
- [x] Write design doc
- [ ] Set up Cargo workspace
- [ ] Create bridge crate scaffold

### Phase 1: Bridge MVP (fake source → MPRIS)
- [ ] Bridge reads JSON from stdin (native messaging framing)
- [ ] Bridge publishes static MPRIS player
- [ ] `playerctl metadata` sees the fake player
- [ ] mprisence detects it

### Phase 2: Extension MVP (generic media)
- [ ] Extension build system (esbuild, manifests)
- [ ] Background script connects via native messaging
- [ ] Content script detects `<audio>/<video>` elements
- [ ] Generic media provider sends updates
- [ ] End-to-end: browser → extension → bridge → MPRIS

### Phase 3: YouTube Music Provider
- [ ] Page-world script for YTM
- [ ] Extract title, artist, album, album art (resolve to HTTPS)
- [ ] Handle miniplayer/navigation
- [ ] Album art works in Discord

### Phase 4: Active Source + Cleanup
- [ ] Multiple tabs tracked internally
- [ ] Active source selection logic
- [ ] Heartbeat timeout / stale removal
- [ ] Tab close → source_removed → MPRIS Stopped

### Phase 5: Installer + Doctor
- [ ] `mprisence-web-bridge install` — detect browsers, write manifests
- [ ] `mprisence-web-bridge uninstall` — remove manifests
- [ ] `mprisence-web-bridge doctor` — validate setup
- [ ] Support Firefox + Chromium + Brave

### Phase 6: mprisence Integration
- [ ] Default `[player.mprisence_web_*]` config entries
- [ ] `ignore_when = "mprisence_web_active"` for raw browser MPRIS
- [ ] Website overrides still work (same pattern as existing)

### Phase 7: Polish
- [ ] Per-tab mode (optional)
- [ ] Extension popup UI (status display)
- [ ] Error handling + logging

## Rationale

### Why native messaging, not WebSocket/HTTP
- Browser-approved local IPC
- No port discovery, no auth, no sandbox escape risk
- Browser manages process lifecycle
- Works in both Firefox and Chromium identically (wire protocol)

### Why one bridge per browser, not one daemon
- Browser manages the native host lifecycle
- No stale daemon if browser closes
- Simpler: no lock files, socket paths, primary election
- Later optimization: broker mode if multi-browser users complain

### Why per-browser bus names, not shared
- Two browsers = two MPRIS players = mprisence handles both
- Avoids bus name conflicts
- User can configure per-browser in mprisence config
- mprisence already handles multiple players well

### Why resolve blob: URLs in content script, not bridge
- Content script has access to fetch API in page context
- Bridge has no rendering context — can't resolve blob URLs
- Content script fetches blob → gets ArrayBuffer → converts to base64
- Passes as `data:image/...;base64,...` in `art_url`
- mprisence already handles data: URLs (it HTTP-fetches art_url; data: works in browsers but mprisence's reqwest won't fetch it)
- **Better**: content script extracts real HTTPS URL from page/SDK when possible. Only use blob→base64 as fallback.

### Why static page-world injection, not runtime eval
- CSP on music.youtube.com and other sites blocks inline scripts
- Static `<script>` element injected at document_start runs before CSP applies (for code that doesn't need DOM) or injected after DOM ready with a nonce-less approach using message passing
- CustomEvent messages cross the world boundary safely
- Plasma Browser Integration uses this pattern for the same reasons

## Future Considerations

- **Broker mode**: Single daemon, multiple browser shims → one MPRIS player
- **Per-tab mode**: One MPRIS player per tab (for advanced users)
- **Custom providers**: User-provided page scripts
- **Non-browser media**: Electron apps, webview-based players
