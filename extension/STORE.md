# Store submission notes — mprisence bridge 1.1.0

Checklist and copy for AMO + Chrome Web Store submissions. Not shipped in the package.

## Release checklist

1. Working tree clean and committed (`build.mjs` stamps `-dirty` into the bundle otherwise).
2. Version bumped in all three: `manifest.firefox.json`, `manifest.chromium.json`, `package.json`.
3. `npm test`
4. `npm ci && npm run build:store` → `dist/mprisence-firefox-store.zip`, `dist/mprisence-chrome-store.zip`
5. `npm run package:source` → `dist/mprisence-extension-source.zip` (AMO only)
6. Verify each store zip: `manifest.json` at root, no `.map`, no `node_modules`,
   and `manifest.json` in the Chrome zip has **no** `key` field.
7. Upload. AMO: attach the source archive. CWS: fill permission justifications below.

## Extension IDs (do not change without a matching `mprisence` release)

| Build | ID |
|---|---|
| Firefox (AMO) | `mprisence-bridge@lazykern.foo` |
| Chrome Web Store | `pnkkjbdopihogobhhjbgapbpfccinjjo` |
| Chrome dev sideload (keyed) | `pphdmbejbipjlocngoefnmjoijcbdejf` |

All three are whitelisted by `mprisence web install` (`src/web_bridge/mod.rs`).
Changing an ID breaks native messaging for every installed user and requires a new
`mprisence` release plus a re-run of `mprisence web install`.

## Reviewer notes (paste into both stores)

> This extension is the browser half of **mprisence**, an open-source Linux desktop
> program (https://github.com/lazykern/mprisence). It reads media metadata from the
> current page and forwards it over native messaging to the local `mprisence` binary,
> which publishes it as an MPRIS D-Bus media player so Linux desktops and `playerctl`
> can see and control browser playback.
>
> **The extension does nothing visible on its own.** Without the native host installed
> it connects, fails, and stays idle — there is no UI to exercise. To test it you would
> need a Linux machine with `mprisence` installed and `mprisence web install` run once.
> Install instructions: https://github.com/lazykern/mprisence#readme
>
> No network requests are made by the extension. No analytics, no remote code, no eval.
> All executed code is bundled in the package. Source: https://github.com/lazykern/mprisence
>
> New in 1.1.0: an **opt-in** generic fallback for sites that have no dedicated
> provider. It is off by default. Turning it on from the options page requests the
> optional `<all_urls>` host permission and registers the extension's own two bundled
> content scripts on `<all_urls>`; turning it off unregisters them and drops the
> permission.

## CWS permission justifications

- **nativeMessaging** — Sole purpose of the extension: hand media metadata to the local
  `mprisence` host binary, which republishes it on Linux D-Bus (MPRIS).
- **storage** — Stores one boolean local setting (`genericEnabled`) for the opt-in
  generic fallback. No user content stored.
- **scripting** — Used only to `registerContentScripts` the extension's own bundled
  `content.js` / `page-world.js` when the user enables the generic fallback. No remote
  or generated code.
- **Host permissions (music sites)** — Content scripts must read the player DOM
  (title, artist, artwork, position) on the supported music sites.
- **`<all_urls>` (optional, not required)** — Requested at runtime only if the user
  explicitly enables the generic fallback, which is intended to work on arbitrary sites
  the user chooses. It is deliberately optional rather than a required permission, so a
  default install never has broad host access.
- **Remote code** — No.
- **Data collection disclosure** — Nothing is collected or transmitted. All data stays
  on the user's machine and goes only to a local process over native messaging.
  Privacy policy: `extension/PRIVACY.md` in the repository.

## AMO notes

- `data_collection_permissions.required: ["none"]` is already declared in the manifest.
- Source archive is required (esbuild-bundled output). Build steps are in
  `README.md` → "Reproducing the store builds (AMO reviewers)".
