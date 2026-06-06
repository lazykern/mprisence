<!-- prettier-ignore -->
<div align="center">

<img src="assets/icon.png" alt="mprisence logo" width="96" />

# mprisence

**Discord Rich Presence for Linux media players, with optional web-player bridge**

[![crates.io](https://img.shields.io/crates/v/mprisence?style=flat-square)](https://crates.io/crates/mprisence)
[![AUR version](https://img.shields.io/aur/version/mprisence?style=flat-square)](https://aur.archlinux.org/packages/mprisence)
[![Nixpkgs](https://img.shields.io/badge/NixOS-nixpkgs-blue?logo=nixos&style=flat-square)](https://search.nixos.org/packages?query=mprisence)
[![MIT](https://img.shields.io/badge/license-MIT-4b5563?style=flat-square)](LICENSE)

[Overview](#overview) • [Quick start](#quick-start) • [Configuration](#configuration) • [Web players](#web-players) • [Development](#development) • [Troubleshooting](#troubleshooting)

</div>

`mprisence` reads MPRIS metadata over D-Bus, renders Discord Rich Presence from configurable templates, and keeps it in sync with your current media.

It works well for local players such as **VLC**, **MPV**, **Rhythmbox**, **Strawberry**, **CMUS**, **MPD**, and more. For browsers, it can use regular browser MPRIS support or an optional **native bridge + extension** for richer site-specific metadata and controls.

<p align="center">
  <img src="assets/example.gif" alt="mprisence demo" width="548" />
</p>

> [!IMPORTANT]
> Discord must allow activity sharing: **Settings → Activity Privacy → Display current activity as a status message**.

## Overview

### How it works

```text
local player or browser tab
  → MPRIS metadata on D-Bus
  → mprisence
  → Discord Rich Presence
```

Optional browser bridge path:

```text
supported website
  → browser extension
  → native messaging host
  → MPRIS bridge player
  → mprisence
  → Discord Rich Presence
```

### Highlights

- Event-driven MPRIS updates with polling fallback
- No config required for common local-player setups
- Handlebars templates for title, artist, album, player name, status, duration, IDs, and more
- Per-player and per-site overrides for app ID, icon, activity type, streaming policy, and status text
- Cover art from metadata, local files, Catbox/Litterbox, MusicBrainz, or ImgBB
- Hot reload for most config changes
- Optional browser bridge for better metadata, cover art, URLs, and controls on web players

## Quick start

> [!NOTE]
> Most users only need `mprisence`. Bridge and extension are optional.

### 1. Install

#### Arch Linux

```bash
yay -S mprisence
# or: yay -S mprisence-bin
```

#### Nix / NixOS

```bash
# without flakes
nix-env -iA nixpkgs.mprisence

# with flakes
nix profile install nixpkgs#mprisence
```

#### Debian / Ubuntu

Download `.deb` from [GitHub Releases](https://github.com/lazykern/mprisence/releases), then:

```bash
sudo dpkg -i /path/to/mprisence_*.deb
```

#### crates.io

```bash
cargo install mprisence
```

#### From source

```bash
git clone https://github.com/lazykern/mprisence.git
cd mprisence
cargo install --path .
```

### 2. Start it

Run foreground:

```bash
mprisence
```

Or enable user service:

```bash
systemctl --user enable --now mprisence.service
```

If your package did not install service file:

```bash
mkdir -p ~/.config/systemd/user
curl -o ~/.config/systemd/user/mprisence.service \
  https://raw.githubusercontent.com/lazykern/mprisence/main/mprisence.service
systemctl --user daemon-reload
systemctl --user enable --now mprisence.service
```

### 3. Verify

Start media playback, then check detected players:

```bash
mprisence players list
mprisence players list --detailed
```

Useful service checks:

```bash
systemctl --user status mprisence
journalctl --user -u mprisence -f
```

## Configuration

Config file path:

```text
~/.config/mprisence/config.toml
```

> [!TIP]
> No config file needed for first run. Create one only when you want overrides.

Start from example config:

```bash
mkdir -p ~/.config/mprisence
curl -o ~/.config/mprisence/config.toml \
  https://raw.githubusercontent.com/lazykern/mprisence/main/config/config.example.toml
```

Reference files:

- [`config/config.example.toml`](./config/config.example.toml) — documented example config
- [`config/config.default.toml`](./config/config.default.toml) — bundled player and web-player presets
- [`src/metadata.rs`](./src/metadata.rs) — template variable source of truth

Common knobs:

- `template.details`, `template.state`, `template.large_text`, `template.small_text`
- `[player.*]` for local-player overrides
- `[web_player.*]` for site-specific browser overrides
- `[activity_type]` and `[time]` for Discord display behavior
- `[cover.provider]` for cover-art sources

Example: show track details instead of player name in Discord status:

```toml
[player.default]
status_display_type = "details"
```

Example: enable YouTube video pages:

```toml
[web_player.youtube]
ignore = false
```

Inspect resolved config:

```bash
mprisence config
```

## Web players

`mprisence` supports browser media in two ways.

### Browser MPRIS

Some browsers expose active media tab as MPRIS player and include page URL metadata. When that metadata is good enough, `mprisence` can match it against `[web_player.*]` presets and apply site config directly.

Check what browser exposes:

```bash
playerctl -l
mprisence players list --detailed
```

### Optional bridge + extension

Use bridge when browser MPRIS misses title, cover art, canonical URL, duration, or reliable controls.

Bundled site support includes:

- YouTube Music
- YouTube
- SoundCloud
- Bandcamp
- TIDAL
- Apple Music
- plus bundled web-player presets for Deezer, Qobuz, Amazon Music, Yandex Music, and more

> [!IMPORTANT]
> No packaged bridge release yet. Web bridge now lives inside `mprisence` under [`src/web_bridge/`](./src/web_bridge), so there is no separate standalone bridge crate or directory to build.

> [!IMPORTANT]
> No packaged bridge release yet. Web bridge now lives inside `mprisence` under [`src/web_bridge/`](./src/web_bridge).

Build `mprisence` from repo root:

```bash
cargo build --release -p mprisence
./target/release/mprisence web install
./target/release/mprisence web doctor
```

Build extension:

```bash
cd extension
npm install
npm run build:firefox   # or: npm run build:chromium
```

Load extension temporarily:

- **Firefox:** `about:debugging#/runtime/this-firefox` → **Load Temporary Add-on** → `extension/dist/firefox/manifest.json`
- **Chromium browsers:** `chrome://extensions` → **Developer mode** → **Load unpacked** → `extension/dist/chromium/`

> [!NOTE]
> Reloading extension kills content scripts on existing tabs. Refresh media tabs after reload.

Bridge debugging:

```bash
playerctl -l | grep mprisence_web
tail -f /tmp/bridge-stderr.log
```

More detail:

- [`extension/README.md`](./extension/README.md)

## Development

### Build workspace

From repo root:

```bash
cargo build --release -p mprisence
./target/release/mprisence web install
cd extension && npm install && npm run build:firefox
```

### Repository layout

| Path | Purpose |
| --- | --- |
| [`src/`](./src) | core daemon: MPRIS discovery, metadata, cover art, config, Discord presence |
| [`src/web_bridge/`](./src/web_bridge) | native host mode that publishes browser sources as MPRIS players |
| [`extension/`](./extension) | browser extension for supported web players |
| [`config/`](./config) | bundled defaults and example config |
| [`tests/`](./tests) | integration and metadata tests |
| [`packaging/`](./packaging) | packaging scripts and distro assets |

### Useful commands

```bash
# run foreground
mprisence

# list players
mprisence players list
mprisence players list --detailed

# show resolved config
mprisence config

# validate version string logic
mprisence version validate 1.7.0-beta.3

# verbose logs
RUST_LOG=debug mprisence
RUST_LOG=trace mprisence
```

## Troubleshooting

### Discord activity not showing

Check:

1. Discord desktop client running
2. Activity-sharing setting enabled
3. `mprisence` process or service running
4. player visible in `mprisence players list`
5. logs show no errors

```bash
systemctl --user status mprisence
mprisence players list --detailed
journalctl --user -u mprisence -f
```

### Player detected, but presence still hidden

Player may be ignored by config or blocked as streaming source.

```bash
mprisence config
mprisence players list --detailed
```

Add matching `[player.*]` override or adjust `[web_player.*]` entry.

### Browser tab missing or wrong

- Browser MPRIS path: browser may not expose enough metadata
- Bridge path: extension may not be loaded, native host may not be installed, or tab may need refresh after extension reload

Useful checks:

```bash
playerctl -l
playerctl -l | grep mprisence_web
./target/release/mprisence web doctor
tail -f /tmp/bridge-stderr.log
```

### Cover art missing

```bash
RUST_LOG=debug mprisence
rm -rf ~/.cache/mprisence/cover_art
```

Then verify provider config and metadata quality.

### Flatpak Discord clients

If you use Vesktop Flatpak, native IPC may need extra setup. See Vesktop guide for native applications:
<https://github.com/flathub/dev.vencord.Vesktop?tab=readme-ov-file#native-applications>

If problem persists, open an issue with:

- player name and `mprisence players list --detailed` output
- relevant config snippet
- logs from `journalctl --user -u mprisence -f`
- bridge logs if browser path involved
