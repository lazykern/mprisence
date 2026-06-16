# Store listing — mprisence bridge

## Published URLs

- **Firefox (AMO):** https://addons.mozilla.org/en-US/firefox/addon/mprisence-bridge/
- **Chrome Web Store:** https://chromewebstore.google.com/detail/pnkkjbdopihogobhhjbgapbpfccinjjo
- **Project / docs:** https://github.com/lazykern/mprisence

## Short description (Chrome, ≤132 chars)

Bridge web music players to desktop MPRIS via the mprisence native app. Works on YouTube Music, SoundCloud, and more.

## Long description

Companion extension for [mprisence](https://github.com/lazykern/mprisence). Reads playback metadata from supported music sites (title, artist, album, cover art, position, duration, play/pause) and sends it to the `mprisence` native host over native messaging. The host publishes MPRIS players (`mprisence_web.*`) on D-Bus.

You need `mprisence` installed and `mprisence web install` run first. Without that, the extension connects to nothing.

**Sites (v1):** YouTube Music, YouTube, SoundCloud (+ subdomains), Bandcamp (+ subdomains), TIDAL (+ subdomains), Apple Music.

**Out of scope for v1:** remote servers (extension sends nothing over the network), Discord (handled by the daemon), arbitrary-site HTML5 fallback.

**Requirements:** Linux + D-Bus, `mprisence` binary, `mprisence web install`, Firefox 128+ or Chromium with native messaging.

## Privacy policy URL

https://mprisence.lazykern.foo/privacy

Cloudflare Pages site on lazykern.foo. Must resolve before submitting.

## Reviewer notes

Paste into AMO/CWS additional information:

---

Companion app required: **mprisence** on Linux. Not a standalone player or Discord client.

The extension uses `nativeMessaging` to send media metadata from supported music sites to local host `mprisence.web.bridge`. The host publishes MPRIS on the session D-Bus. Traffic stays on the machine (stdin/stdout to the native host). No remote servers.

**Reviewer setup (Linux):**

1. `cargo build --release -p mprisence` from https://github.com/lazykern/mprisence
2. `./target/release/mprisence web install`
3. Load extension from `extension/dist/firefox` or `extension/dist/chromium`
4. Play a track on https://music.youtube.com
5. `playerctl -l | grep mprisence_web`
6. Optional: `./target/release/mprisence web doctor`

**Permissions:**

- `nativeMessaging` — talk to local bridge host
- `tabs` — route playback commands to the right tab; remove sources on tab close
- `host_permissions` (nine music-site patterns) — inject content scripts on supported domains to read page-local media state. Extension makes no network requests.

Content scripts run on listed music domains only, not `<all_urls>`.

A MAIN-world content script reads `navigator.mediaSession` and related page media state, then dispatches to the isolated content script. No `eval`, no dynamic injection.

Generic `<video>`/`<audio>` fallback on arbitrary sites is omitted in v1 to keep scope narrow.

---

## Screenshots

1280×800 or 640×400 PNG/JPEG:

| # | Shot |
|---|------|
| 1 | YTM playing, extension toolbar badge visible |
| 2 | Terminal: `playerctl -l \| grep mprisence_web` + metadata |
| 3 | Terminal: `./target/release/mprisence web doctor` passing |

Optional: Discord RP via daemon.

## Category

Music (primary), Productivity (secondary).

## Single purpose

Forward web playback metadata to desktop MPRIS through a local native host.

## Build artifacts

```bash
cd extension && npm run build:store
```

- `dist/mprisence-firefox-store.zip` — AMO
- `dist/mprisence-chrome-store.zip` — CWS

Chrome `key` must stay in store builds. See [extension/README.md](../extension/README.md).
