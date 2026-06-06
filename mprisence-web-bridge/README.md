# mprisence Web Bridge

Browser metadata bundle for mprisence.

This directory contains both halves of the bridge:

- `src/`: Rust native messaging host. It receives browser updates and publishes MPRIS players.
- `extension/`: browser extension. It reads media metadata from web players and sends it to the host.

Use this bridge only when browser MPRIS is not enough. Native browser MPRIS may miss cover art, expose generic URLs, or merge every tab into one player.

## How it works

```text
web page
  -> extension/content.ts + extension/page-world.ts
  -> extension/background.ts
  -> native messaging host: mprisence.web.bridge
  -> D-Bus MPRIS player: org.mpris.MediaPlayer2.mprisence_web.<site>.<hash>
  -> mprisence daemon
  -> Discord activity
```

The bridge publishes one MPRIS player per live browser source. It does not choose one active tab.

Source IDs look like this:

```text
<browser>:tab:<tab_id>:<frame_id>:frame
```

Example:

```text
firefox:tab:42:0:frame
```

Content scripts may start with tab ID `0`. `extension/src/background.ts` rewrites source IDs from `sender.tab.id` when the browser provides it.

## Build

From repo root:

```bash
cargo build --release -p mprisence-web-bridge
```

Build extension:

```bash
cd mprisence-web-bridge/extension
npm install
npm run build:firefox
npm run build:chromium
```

Output:

```text
mprisence-web-bridge/extension/dist/firefox/
mprisence-web-bridge/extension/dist/chromium/
```

## Install native host

From repo root:

```bash
./target/release/mprisence-web-bridge install
./target/release/mprisence-web-bridge doctor
```

Limit install to one browser family:

```bash
./target/release/mprisence-web-bridge install --browser firefox
./target/release/mprisence-web-bridge install --browser chromium
```

Remove manifests:

```bash
./target/release/mprisence-web-bridge uninstall
```

Native messaging host name:

```text
mprisence.web.bridge
```

Common manifest paths:

```text
~/.mozilla/native-messaging-hosts/mprisence.web.bridge.json
~/.config/chromium/NativeMessagingHosts/mprisence.web.bridge.json
~/.config/google-chrome/NativeMessagingHosts/mprisence.web.bridge.json
```

Firefox uses `allowed_extensions`. Chromium uses `allowed_origins` with the extension ID.

## Load extension

Firefox:

1. Open `about:debugging#/runtime/this-firefox`.
2. Click **Load Temporary Add-on**.
3. Select `mprisence-web-bridge/extension/dist/firefox/manifest.json`.

Chromium, Chrome, Edge, Brave:

1. Open `chrome://extensions`.
2. Enable **Developer mode**.
3. Click **Load unpacked**.
4. Select `mprisence-web-bridge/extension/dist/chromium/`.

After rebuilding or reloading the extension, refresh target media tabs. Reloading the extension kills content scripts on existing tabs.

## Config behavior

Bridge MPRIS players use bus names like this:

```text
org.mpris.MediaPlayer2.mprisence_web.<site>.<hash>
```

mprisence resolves all bridge players through one stable config key:

```toml
[player.mprisence_web]
```

Then it applies `[web_player.*]` based on the player URL. URL priority:

1. `canonical_url`
2. page `url`, unless it is `blob:`
3. `origin`

Bundled defaults enable most known music sites. YouTube video pages are bundled but ignored by default. Unmatched HTTP/HTTPS URLs are ignored by default.

The bridge also publishes:

```text
mprisence:browser = "firefox" | "chromium" | "brave" | "vivaldi" | "edge"
```

mprisence uses that key to suppress duplicate native browser MPRIS players when bridge data matches the same browser and URL.

## Protocol

Wire format uses standard native messaging framing:

```text
4-byte unsigned little-endian length
UTF-8 JSON payload
```

Protocol source files:

- Rust: `src/protocol.rs`
- TypeScript: `extension/src/types.ts`

Extension to host messages:

- `hello`
- `update`
- `remove`

Host to extension messages:

- `hello`
- `command`
- `heartbeat`

Supported commands:

- `play_pause`
- `play`
- `pause`
- `next`
- `previous`
- `seek`
- `set_position`

## Providers

Current provider registry lives in `extension/src/content.ts`:

- YouTube Music
- YouTube
- SoundCloud
- Bandcamp
- TIDAL
- Apple Music
- Generic media element fallback

Provider implementations live in `extension/src/providers/`.

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

Useful log searches:

```bash
grep 'Extension connected' /tmp/bridge-stderr.log | tail
grep 'Created MPRIS player' /tmp/bridge-stderr.log | tail
grep 'MPRIS player .* emitted' /tmp/bridge-stderr.log | tail
grep 'clamping position' /tmp/bridge-stderr.log | tail
```

Browser logs:

- background script: extension inspector
- content script: devtools on the target media page

## Source map

| Area | Files |
| --- | --- |
| native host CLI and loop | `src/main.rs` |
| native messaging framing | `src/native_messaging.rs` |
| protocol types | `src/protocol.rs`, `extension/src/types.ts` |
| source registry | `src/active_source.rs` |
| MPRIS publishing | `src/mpris.rs` |
| extension background | `extension/src/background.ts` |
| extension content pipeline | `extension/src/content.ts` |
| page-world script | `extension/src/page-world.ts` |
| providers | `extension/src/providers/` |
| browser detection/source IDs | `extension/src/utils/browser-detect.ts` |
| native messaging port | `extension/src/utils/native-messaging.ts` |
