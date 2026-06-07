# Browser store listing — mprisence bridge

Copy and collateral for Firefox Add-ons (AMO) and Chrome Web Store (CWS) submissions.

## Short description

**Text (132 characters — fits Chrome Web Store limit):**

> Bridge web music players to desktop MPRIS via the mprisence native app. Works on YouTube Music, SoundCloud, and more.

**Character count:** 108

## Long description

mprisence bridge is a companion browser extension for [mprisence](https://github.com/lazykern/mprisence), a Linux application that turns MPRIS media metadata into Discord Rich Presence and desktop integrations.

The extension reads playback metadata from supported music websites in your browser—title, artist, album, cover art URL, position, duration, and play/pause state—and forwards it over **native messaging** to the `mprisence` native host on your machine. The host publishes each active tab as an MPRIS D-Bus player (`mprisence_web.*`), so media keys, `playerctl`, and `mprisence` itself can control and display what you are listening to in the browser.

**This extension does not work on its own.** You must install and run `mprisence`, then register the native messaging host with `mprisence web install`. Without the companion app, the extension has nothing to connect to and no MPRIS player is created.

### Supported sites (v1)

Content scripts run only on these domains:

- YouTube Music (`music.youtube.com`)
- YouTube (`www.youtube.com`)
- SoundCloud (`soundcloud.com` and subdomains)
- Bandcamp (`bandcamp.com` and subdomains)
- TIDAL (`tidal.com` and subdomains)
- Apple Music (`music.apple.com`)

### What it does not do

- Does not send data to remote servers—all communication stays on your machine via native messaging.
- Does not connect to Discord directly; that is handled by the separate `mprisence` daemon if you use it.
- Does not inject on arbitrary websites. A generic HTML5 media fallback (`GenericMediaProvider`) is **deferred for store v1** and is not included in the published build.

### Requirements

- Linux with D-Bus session bus
- `mprisence` binary built or installed from the project repository
- Native host registered: `mprisence web install`
- A supported browser (Firefox 128+, or Chromium-based browser with native messaging)

## Privacy policy URL

**URL for store forms:**

https://lazykern.github.io/mprisence/

This URL serves the privacy policy from `docs/index.html` via GitHub Pages. Confirm it loads in a browser before submitting the store form.

## Reviewer notes

Paste the following into the AMO or CWS reviewer notes / additional information field:

---

This extension requires a companion native application (**mprisence**) installed separately on the user's Linux machine. It is not a standalone media player or Discord client.

**How it works:** The extension uses the browser's `nativeMessaging` API to send media metadata (title, artist, album, cover art URL, playback position, duration, play/pause state) from supported music websites to a local native host (`mprisence.web.bridge`). The host publishes MPRIS players on the session D-Bus. **No data is sent to remote servers**—all communication is local stdin/stdout between the extension background script and the native host process.

**Install steps for reviewers (Linux):**

1. Clone and build mprisence from https://github.com/lazykern/mprisence  
   `cargo build --release -p mprisence`
2. Register the native messaging host:  
   `./target/release/mprisence web install`
3. Install this extension (temporary load or unpacked from `extension/dist/firefox` or `extension/dist/chromium`).
4. Open https://music.youtube.com and play any track.
5. Verify an MPRIS player appears:  
   `playerctl -l | grep mprisence_web`  
   Optional: `playerctl -p mprisence_web.youtube_music.<id> metadata` to inspect forwarded metadata.
6. Optional sanity check: `./target/release/mprisence web doctor` should report manifest and extension ID matches.

**Permissions justification:**

- `nativeMessaging` — required to talk to the local `mprisence` bridge host.
- `tabs` — required to route host playback commands to the correct tab and clean up when tabs close.
- `host_permissions` (nine music-site patterns) — required so content scripts may inject on supported streaming domains and read page-local media metadata. No network requests are made by the extension; permissions scope DOM access only.

**Content script scope:** Scripts inject **only** on listed music domains (YouTube Music, YouTube, SoundCloud, Bandcamp, TIDAL, Apple Music)—not on `<all_urls>` or unrelated sites.

**MAIN-world script:** A second content script runs in the page's MAIN world (not an isolated extension world) solely to read the page's `navigator.mediaSession` API and related media state that is already exposed to the page. It dispatches events to the isolated content script; it does not use `eval` or dynamic code injection.

**Deferred for v1:** Generic HTML5 `<video>`/`<audio>` fallback on arbitrary sites is intentionally omitted from this store release to keep injection scope narrow.

---

## Screenshots checklist

Capture at **1280×800** or **640×400** (PNG or JPEG). Suggested shots:

| # | Screenshot | What to show |
|---|------------|--------------|
| 1 | YouTube Music playing | A track playing on `music.youtube.com` with the extension toolbar badge visible (site label, e.g. "YTM") |
| 2 | `playerctl` metadata | Terminal showing `playerctl -l \| grep mprisence_web` and `playerctl -p mprisence_web.youtube_music.* metadata` with title/artist from the playing tab |
| 3 | `mprisence web doctor` | Terminal showing `./target/release/mprisence web doctor` success output (Firefox/Chromium manifests found, extension IDs match) |

Optional fourth screenshot: Discord Rich Presence showing the bridged track (requires `mprisence` daemon running with Discord configured).

## Category

- **Primary:** Music
- **Secondary:** Productivity

(AMO: Music & Audio. CWS: pick the closest match—Music or Productivity.)

## Single purpose

**Bridge web media playback metadata to the desktop MPRIS standard** via a local native host, enabling system media controls and desktop integrations (e.g. `mprisence` Discord Rich Presence) for supported music websites.

## Store build artifacts

From `extension/`:

```bash
npm run build:store
```

Produces:

- `dist/mprisence-firefox-store.zip` — AMO upload
- `dist/mprisence-chrome-store.zip` — CWS upload

See [extension/README.md](../extension/README.md) for Chrome extension ID coupling and load instructions.
