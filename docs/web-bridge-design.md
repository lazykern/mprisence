# mprisence Web Bridge

Implementation notes for the browser extension and native bridge.

The bridge exists because native browser MPRIS is often incomplete. Common failures: missing cover art, generic URLs such as `https://music.youtube.com/`, weak controls, or no metadata. The extension reads site metadata from the page, sends it to a native messaging host, and the host publishes MPRIS players that the normal mprisence daemon consumes.

## Current status

Implemented:

- native messaging host: `mprisence-web-bridge/`
- browser extension: `extension/`
- Firefox and Chromium manifests
- one MPRIS player per browser source/tab
- site providers for YouTube Music, YouTube, SoundCloud, Bandcamp, TIDAL, Apple Music, plus generic media
- MPRIS controls back to the page: play, pause, play-pause, next, previous, seek, set-position
- native messaging install, uninstall, and doctor commands
- bridge-player config routing through `[player.mprisence_web]` and `[web_player.*]`

## Runtime architecture

```text
Browser
  extension/background.ts
    connectNative("mprisence.web.bridge")
    forwards page updates to native host
    forwards MPRIS commands back to the right tab

  extension/content.ts
    runs in isolated world
    listens for media events, DOM mutations, and keepalive ticks
    runs provider extraction
    sends update/remove messages to background

  extension/page-world.ts
    runs in MAIN world from the manifest
    reads page-only media state where needed
    dispatches CustomEvent("mprisence-media-state")

Native host
  mprisence-web-bridge/src/main.rs
    reads native messaging frames from stdin
    writes bridge messages to stdout
    creates/removes MPRIS players

  active_source.rs
    stores one SourceState per browser source
    prunes stale sources after 90 seconds

  mpris.rs
    publishes one MPRIS player per source
    maps updates to org.mpris.MediaPlayer2 metadata/properties
    forwards MPRIS method calls to the extension

mprisence daemon
  consumes bridge MPRIS players like normal players
  resolves all bridge players through config key mprisence_web
  applies [web_player.*] overrides from xesam:url or title suffix
```

## Source model

The bridge does not pick one active browser tab. Each live browser source gets its own MPRIS player.

Source ID format:

```text
<browser>:tab:<tab_id>:<frame_id>:frame
```

Example:

```text
firefox:tab:42:0:frame
```

Content scripts cannot reliably know their own tab ID, so they may send tab ID `0`. `background.ts` rewrites source IDs using `sender.tab.id` when available and keeps a reverse map from tab ID to source ID for command routing.

A source is pruned if it sends no update for 90 seconds. The extension sends a forced keepalive every 30 seconds. Browsers can throttle background tabs to about once per minute, so the timeout must stay above that.

## MPRIS player naming

Each source publishes a D-Bus player under the shared `mprisence_web` namespace:

```text
org.mpris.MediaPlayer2.mprisence_web.<site>.<hash>
```

Examples:

```text
org.mpris.MediaPlayer2.mprisence_web.youtube_music.p00ef924695567065
org.mpris.MediaPlayer2.mprisence_web.soundcloud.p9f3a4d6c8e1b2a00
```

The suffix is generated in `mprisence-web-bridge/src/mpris.rs`:

```text
mprisence_web.<site>.<hash(source_id)>
```

The `mpris_server::Player::builder()` call receives only the suffix. The library prepends `org.mpris.MediaPlayer2.`.

## mprisence config resolution

All bridge players resolve to the stable player config key:

```toml
[player.mprisence_web]
```

That key is internal routing behavior, not a required default config block. The daemon rewrites bridge player identity and bus name to `mprisence_web` before config lookup.

Then mprisence applies `[web_player.*]` using the bridge-provided URL:

1. `canonical_url`, if present
2. page `url`, if not `blob:`
3. `origin`

Matched web-player config fully replaces the generic browser/player config. This lets one bridge bus namespace support different Discord app IDs and icons per site.

Important default behavior:

- bundled web-player entries enable known sites such as YouTube Music, SoundCloud, Apple Music, Bandcamp, and TIDAL
- YouTube video pages are bundled but ignored by default
- unmatched HTTP/HTTPS URLs are ignored by `[web_player.default]`
- native browser MPRIS can be suppressed when a bridge player exposes the same browser and URL

The bridge adds a custom metadata key:

```text
mprisence:browser = "firefox" | "chromium" | "brave" | "vivaldi" | "edge"
```

mprisence uses it to suppress duplicate native browser MPRIS players when the bridge is active for the same content.

## Protocol v1

Wire format uses standard native messaging framing:

```text
4-byte unsigned little-endian length
UTF-8 JSON payload
```

Host name:

```text
mprisence.web.bridge
```

Protocol source of truth:

- Rust: `mprisence-web-bridge/src/protocol.rs`
- TypeScript: `extension/src/types.ts`

### Extension to bridge

```ts
type ExtMessage =
  | {
      type: "hello";
      browser: "firefox" | "chromium" | "brave" | "vivaldi" | "edge";
      extension_version: string;
      protocol: number;
      git_sha?: string;
      extension_fingerprint?: string;
    }
  | {
      type: "update";
      source_id: string;
      url: string;
      origin: string;
      site: string;
      playback: PlaybackState;
      metadata: MediaMetadata;
      capabilities: Capabilities;
      canonical_url?: string;
    }
  | {
      type: "remove";
      source_id: string;
    };
```

```ts
interface PlaybackState {
  status: "playing" | "paused" | "stopped";
  position_ms: number;
  duration_ms: number;
}

interface MediaMetadata {
  title?: string;
  artist: string[];
  album?: string;
  album_artist: string[];
  art_url?: string;
  track_id?: string;
}

interface Capabilities {
  play_pause: boolean;
  next: boolean;
  previous: boolean;
  seek: boolean;
  set_position: boolean;
}
```

### Bridge to extension

```ts
type BridgeMessage =
  | {
      type: "hello";
      bridge_version: string;
      protocol: number;
      git_sha?: string;
    }
  | {
      type: "command";
      source_id: string;
      command:
        | "play_pause"
        | "play"
        | "pause"
        | "next"
        | "previous"
        | "seek"
        | "set_position";
      position_ms?: number;
    }
  | { type: "heartbeat" };
```

Notes:

- `seek` is relative in MPRIS, but the extension protocol only carries absolute `position_ms`. Current bridge does not send relative seek offsets as `position_ms`.
- `set_position` sends absolute `position_ms`.
- `heartbeat` is accepted by the extension, but current bridge liveness mainly depends on reconnect and source keepalives.

## MPRIS mapping

| MPRIS field | Bridge source |
| --- | --- |
| `PlaybackStatus` | `playback.status` |
| `Position` | `playback.position_ms * 1000` |
| `mpris:length` | `playback.duration_ms * 1000`, only when finite and under 24h |
| `mpris:trackid` | `metadata.track_id`, else hash of canonical URL, page URL, or source/title |
| `mpris:artUrl` | `metadata.art_url`, only if `http://` or `https://` |
| `xesam:title` | `metadata.title` |
| `xesam:artist` | `metadata.artist` |
| `xesam:album` | `metadata.album` |
| `xesam:albumArtist` | `metadata.album_artist` |
| `xesam:url` | `canonical_url`, else page URL, else origin |
| `mprisence:browser` | browser extracted from `source_id` |
| `Identity` | formatted site name, e.g. `YouTube Music` |
| capabilities | `capabilities.*` |

Position is set every publish. Other MPRIS properties are diffed and only emitted when changed.

The bridge clamps position to duration. YouTube Music can briefly report old `<video>.currentTime` for a new track while the progress bar already reports the new duration. Without clamping, Discord can show nonsense elapsed time.

## Extension provider pipeline

Provider registry lives in `extension/src/content.ts`:

1. `YouTubeMusicProvider`
2. `YouTubeProvider`
3. `SoundCloudProvider`
4. `BandcampProvider`
5. `TidalProvider`
6. `AppleMusicProvider`
7. `GenericMediaProvider`

Each provider implements the interface from `extension/src/providers/base.ts`:

```ts
interface Provider {
  siteKey: string;
  matches(url: URL): boolean;
  extract(): ProviderResult | null;
  command(command: string, positionMs?: number): Promise<void>;
}
```

Updates come from three paths:

- media element events: `play`, `pause`, `ended`, `seeked`, `loadedmetadata`, `durationchange`, etc.
- throttled `timeupdate`, about once per second
- debounced `MutationObserver`, mainly for SPAs that change track UI without media events

Page-world events provide extra metadata when isolated-world DOM access is not enough. YouTube Music can also send art-only updates from page-world. The content script merges art-only updates into the last provider metadata to avoid blanking title/artist.

Dedup rules in `sendUpdate()` avoid sending unchanged state. Forced keepalives bypass dedup only to refresh `last_seen` in the bridge.

URL stability matters. If an update lacks a page URL and the track identity did not change, the content script reuses the last known page URL. This avoids URL flapping between canonical track URLs and `window.location.href` with playlist parameters.

## Build and install

Build native bridge:

```bash
cargo build --release -p mprisence-web-bridge
```

Install native messaging manifests:

```bash
./target/release/mprisence-web-bridge install
```

Limit install to one browser family:

```bash
./target/release/mprisence-web-bridge install --browser firefox
./target/release/mprisence-web-bridge install --browser chromium
```

Check install:

```bash
./target/release/mprisence-web-bridge doctor
```

Remove manifests:

```bash
./target/release/mprisence-web-bridge uninstall
```

Build extension:

```bash
cd extension
npm install
npm run build:firefox
npm run build:chromium
```

Store builds:

```bash
npm run build:firefox:store
npm run build:chromium:store
npm run build:store
```

Firefox temporary load:

```text
about:debugging#/runtime/this-firefox
Load Temporary Add-on
extension/dist/firefox/manifest.json
```

Chromium temporary load:

```text
chrome://extensions
Developer mode
Load unpacked
extension/dist/chromium/
```

## Native messaging manifests

Firefox path:

```text
~/.mozilla/native-messaging-hosts/mprisence.web.bridge.json
```

Chromium and Chrome paths:

```text
~/.config/chromium/NativeMessagingHosts/mprisence.web.bridge.json
~/.config/google-chrome/NativeMessagingHosts/mprisence.web.bridge.json
```

Installer also scans Chromium/Chrome profile-root variants under `~/.config`.

Native host requirements:

- Firefox manifest uses `allowed_extensions` with `mprisence-bridge@lazykern.github.io`
- Chromium manifest uses `allowed_origins` with the extension ID from `CHROME_EXTENSION_ID`
- Firefox and Chromium pass extra CLI args to native hosts. Clap must allow trailing args or the bridge exits before logging useful output.

## Debugging

Bridge log:

```bash
tail -f /tmp/bridge-stderr.log
```

List bridge players:

```bash
playerctl -l | grep mprisence_web
```

Inspect one player:

```bash
playerctl -p mprisence_web.youtube_music.p00ef924695567065 metadata
playerctl -p mprisence_web.youtube_music.p00ef924695567065 status
```

Test controls:

```bash
playerctl -p mprisence_web.youtube_music.p00ef924695567065 play-pause
playerctl -p mprisence_web.youtube_music.p00ef924695567065 next
playerctl -p mprisence_web.youtube_music.p00ef924695567065 previous
```

Inspect how mprisence resolves the player:

```bash
mprisence players list --detailed
mprisence config
```

Useful bridge log patterns:

```bash
grep 'Extension connected' /tmp/bridge-stderr.log | tail
grep 'Created MPRIS player' /tmp/bridge-stderr.log | tail
grep 'MPRIS player .* emitted' /tmp/bridge-stderr.log | tail
grep 'clamping position' /tmp/bridge-stderr.log | tail
```

Browser-side logs:

- background logs: extension inspector
- content logs: page devtools for the target site
- extension reload kills content scripts, so refresh target pages after reload

## Known limitations

- Extension is currently temporary-load/dev oriented unless packaged through store build scripts.
- Chromium native messaging allowed origin depends on the fixed extension key/ID.
- Relative MPRIS seek is not represented as a relative offset in the extension protocol.
- Bridge art URLs must be HTTP/HTTPS. Data and blob URLs are not published as `mpris:artUrl`.
- Content scripts cannot directly know their tab ID; background rewrites source IDs from sender metadata.
- Browser throttling affects background tabs, so keepalive and stale timeout values must stay conservative.

## Source map

| Area | Files |
| --- | --- |
| bridge CLI and event loop | `mprisence-web-bridge/src/main.rs` |
| native messaging framing | `mprisence-web-bridge/src/native_messaging.rs` |
| protocol types | `mprisence-web-bridge/src/protocol.rs`, `extension/src/types.ts` |
| source registry | `mprisence-web-bridge/src/active_source.rs` |
| MPRIS publishing | `mprisence-web-bridge/src/mpris.rs` |
| extension background | `extension/src/background.ts` |
| extension content pipeline | `extension/src/content.ts` |
| page-world script | `extension/src/page-world.ts` |
| providers | `extension/src/providers/` |
| browser detection/source IDs | `extension/src/utils/browser-detect.ts` |
| extension native port | `extension/src/utils/native-messaging.ts` |
| mprisence bridge config routing | `src/config/mod.rs`, `src/player/mod.rs` |
| web-player defaults | `config/config.default.toml` |
