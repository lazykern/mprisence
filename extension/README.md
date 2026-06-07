# mprisence Web Extension

Browser extension that sends rich media metadata from supported websites to the `mprisence` native host via Chrome-native messaging.

The extension reads media metadata (title, artist, cover art, position, duration, playback state) from supported websites, aggregates it in the background script, and forwards it to the native host `mprisence.web.bridge`, which publishes each tab as its own MPRIS D-Bus player.

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

## Chrome extension ID

The Chromium manifest `key` field pins extension ID `pphdmbejbipjlocngoefnmjoijcbdejf`.
This must match `CHROME_EXTENSION_ID` in `src/web_bridge/mod.rs`.
Store uploads must include `key` — do not strip it.

If the ID ever changes, update the Rust constant and release a new `mprisence` build.
Users must re-run `mprisence web install`.

## Temporary load

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

Generic HTML5 media fallback is deferred for store v1 (see `docs/store-listing.md`).

Provider files live in `src/providers/`.

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

## Store submission

Store listing copy, reviewer notes, screenshot checklist, and privacy policy URL:

[docs/store-listing.md](../docs/store-listing.md)

Build submission zips with `npm run build:store` (see **Store builds** above). Upload `dist/mprisence-firefox-store.zip` to AMO and `dist/mprisence-chrome-store.zip` to CWS. Keep the Chromium manifest `key` field in store builds so the extension ID stays aligned with `CHROME_EXTENSION_ID` in `src/web_bridge/mod.rs`.
