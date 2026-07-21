# mprisence Web Extension

Browser extension that sends rich media metadata from supported websites to the `mprisence` native host via Chrome-native messaging.

The extension reads media metadata (title, artist, cover art, position, duration, playback state) from supported websites, aggregates it in the background script, and forwards it to the native host `mprisence.web.bridge`, which publishes each tab as its own MPRIS D-Bus player.

## Install

1. Install [mprisence](https://github.com/lazykern/mprisence) and register the native host: `mprisence web install`
2. Install the extension:
   - **Firefox:** [mprisence bridge on AMO](https://addons.mozilla.org/en-US/firefox/addon/mprisence-bridge/)
   - **Chrome / Chromium:** [mprisence bridge on Chrome Web Store](https://chromewebstore.google.com/detail/pnkkjbdopihogobhhjbgapbpfccinjjo)
3. Verify (optional): `mprisence web doctor`

## Build

```bash
npm install
npm run build:firefox
npm run build:chromium
```

Watch builds:

```bash
npm run watch:firefox
npm run watch:chromium
```

Store builds:

```bash
npm run build:firefox:store
npm run build:chromium:store
npm run build:store
```

Outputs:

```text
dist/firefox/
dist/chromium/
```

## Chrome extension IDs

- **Chrome Web Store:** `pnkkjbdopihogobhhjbgapbpfccinjjo` — matches `CHROME_EXTENSION_ID` in `src/web_bridge/mod.rs`
- **Dev sideload (keyed build):** `pphdmbejbipjlocngoefnmjoijcbdejf` — pinned by the `key` field in `manifest.chromium.json`

Store builds strip `key` so CWS assigns the store ID. `mprisence web install` allows both origins. If either ID changes, update the Rust constants and release a new `mprisence` build; users must re-run `mprisence web install`.

## Temporary load (development)

Firefox:

1. Open `about:debugging#/runtime/this-firefox`.
2. Click **Load Temporary Add-on**.
3. Select `dist/firefox/manifest.json`.

Chromium, Chrome, Edge, Brave:

1. Open `chrome://extensions`.
2. Enable **Developer mode**.
3. Click **Load unpacked**.
4. Select `dist/chromium/`.

After reload, refresh target media tabs. Existing content scripts die when the extension reloads.

## Architecture

```text
page-world.ts
  runs in MAIN world
  can see page-owned media/session state
  dispatches CustomEvent("mprisence-media-state")

content.ts
  runs in isolated world
  watches media events, DOM mutations, and keepalive ticks
  runs provider extraction
  sends update/remove messages with chrome.runtime.sendMessage

background.ts
  owns native messaging port
  rewrites source IDs with sender.tab.id when available
  forwards updates to native host
  routes host commands back to tabs
```

Both `content.js` and `page-world.js` are manifest-declared content scripts. No runtime script injection or dynamic eval is used.

## Providers

Provider registry lives in `src/content.ts`:

- YouTube Music
- YouTube
- SoundCloud
- Bandcamp
- TIDAL
- Apple Music

Provider files live in `src/providers/`.

### Generic fallback (unsupported sites)

Opt-in, off by default. Enable it on the extension's options page. It publishes
a player for any site with `<audio>`/`<video>`, from the page's own Media
Session (title/artist/album/artwork) plus `og:`/favicon/title fallbacks, with
play/pause/seek from the element and Next/Previous only when the page
registered those Media Session action handlers.

Enabling requests the optional `<all_urls>` host permission and dynamically
registers `content.js` + `page-world.js` on `<all_urls>` minus the supported
sites above (background `syncGenericRegistration`); disabling unregisters them
and drops the permission. Collection runs in `page-world.ts`
(`startGenericMode`) because the isolated world can't read the page's
`navigator.mediaSession`.

> Enable this only if you have disabled your browser's built-in MPRIS/media
> integration — otherwise you get duplicate players. Firefox:
> `media.hardwaremediakeys.enabled = false`. Chromium:
> `--disable-features=HardwareMediaKeyHandling,MediaSessionService`.
> `mprisence web doctor` warns when a competing browser player is on the bus.

Provider interface:

```ts
interface Provider {
  siteKey: string;
  matches(url: URL): boolean;
  extract(): ProviderResult | null;
  command(command: string, positionMs?: number): Promise<void>;
}
```

## Messages

Protocol types live in `src/types.ts` and must match Rust types in `../src/web_bridge/protocol.rs`.

Extension sends:

- `hello`
- `update`
- `remove`

Host sends:

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

## Debugging

Background logs:

1. Open extension management page.
2. Inspect the extension service worker or background script.

Content logs:

1. Open devtools on the target media page.
2. Use the Console tab.

Native host log:

```bash
tail -f /tmp/bridge-stderr.log
```

Check bridge player:

```bash
playerctl -l | grep mprisence_web
```
