# mprisence

[![AUR version](https://img.shields.io/aur/version/mprisence)](https://aur.archlinux.org/packages/mprisence)
[![Nixpkgs](https://img.shields.io/badge/NixOS-nixpkgs-blue?logo=nixos)](https://search.nixos.org/packages?query=mprisence)

Discord Rich Presence for Linux media players.

mprisence reads MPRIS metadata from your player, renders it with your config, and updates Discord. It works with local players such as VLC, MPV, Rhythmbox, and browser/web players through browser MPRIS or the optional web bridge.

<img src="/assets/example.gif" width="548" height="548"/>

_(Appearance depends on your config and media player.)_

## Choose your setup

1. **Using VLC, MPV, Rhythmbox, Audacious, or another local player?**
   Start with [Quick start](#quick-start). You usually do not need config.

2. **Using browser media that already appears in `playerctl -l`?**
   Start with [Web players](#web-players). You may need config if the site is not bundled.

3. **Using YouTube Music, SoundCloud, Apple Music, or another site with poor browser metadata?**
   Use [Extension bridge setup](#extension-bridge-setup) for better metadata, cover art, and controls.

4. **Player appears in mprisence, but Discord does not update?**
   See [Common recipes](#common-recipes).

## What you get

- Discord Rich Presence from MPRIS players
- Templates for title, artist, album, player name, and technical metadata
- Per-player overrides for app ID, icon, display text, streaming, and activity type
- Cover art from player metadata, local files, MusicBrainz, Catbox/Litterbox, or ImgBB
- Hot reload for most config changes
- Web-player matching for supported browser URLs
- Optional browser extension bridge for richer web-player metadata and controls

## Before you start

You need:

- Linux desktop session with D-Bus
- Discord desktop client or compatible client with Rich Presence support
- Discord setting enabled: **Settings → Activity Privacy → Display current activity as a status message**
- For service mode: systemd user session

Important defaults:

- **You do not need a config file for first run.**
- **Unknown local players are hidden by default.** Add a small config entry if your player is not bundled.
- **Unknown browser HTTP/HTTPS URLs are hidden by default**, so random tabs do not appear in Discord.
- **Spotify is bundled but disabled by default.**
- **YouTube video pages are bundled but ignored by default.**

## Quick start

### 1. Install

#### Arch Linux

```bash
# Stable
yay -S mprisence

# Prebuilt binary
yay -S mprisence-bin
```

Enable and start the service:

```bash
systemctl --user enable --now mprisence.service
```

#### Nix, NixOS, Linux

```bash
# without flakes
nix-env -iA nixpkgs.mprisence

# with flakes
nix profile install nixpkgs#mprisence
```

NixOS:

```nix
environment.systemPackages = [ pkgs.mprisence ];
```

Then run mprisence from a terminal or configure a user service for your setup.

#### Debian, Ubuntu, derivatives

Download the `.deb` package from [GitHub Releases](https://github.com/lazykern/mprisence/releases), then install it:

```bash
sudo dpkg -i /path/to/mprisence_*.deb
```

Enable and start the service:

```bash
systemctl --user enable --now mprisence.service
```

#### From crates.io

```bash
cargo install mprisence
```

#### From source

```bash
git clone https://github.com/lazykern/mprisence.git
cd mprisence
cargo install --path .
```

### 2. Verify

Start a supported player and play media.

If you started the service, check it:

```bash
systemctl --user status mprisence
```

If you did not start the service, run mprisence in a terminal:

```bash
mprisence
```

Then check player detection:

```bash
mprisence players list
```

If your player appears and Discord shows activity, core setup works.

### 3. Run as service

If your package did not install a service file, create one manually:

```bash
mkdir -p ~/.config/systemd/user
curl -o ~/.config/systemd/user/mprisence.service \
  https://raw.githubusercontent.com/lazykern/mprisence/main/mprisence.service
systemctl --user daemon-reload
systemctl --user enable --now mprisence.service
```

If your `mprisence` binary is not in `~/.cargo/bin/`, edit `~/.config/systemd/user/mprisence.service` and change `ExecStart`.

Service commands:

```bash
systemctl --user status mprisence
systemctl --user restart mprisence
journalctl --user -u mprisence -f
systemctl --user disable --now mprisence
```

## Configuration

Config path:

```text
~/.config/mprisence/config.toml
```

or:

```text
$XDG_CONFIG_HOME/mprisence/config.toml
```

**You do not need a config file for first run.** Create one only when you want to customize behavior.

Start from the example config:

```bash
mkdir -p ~/.config/mprisence
curl -o ~/.config/mprisence/config.toml \
  https://raw.githubusercontent.com/lazykern/mprisence/main/config/config.example.toml
```

Most config changes hot-reload.

Reference files:

- [`config.example.toml`](./config/config.example.toml): documented example config
- [`config.default.toml`](./config/config.default.toml): bundled player and web-player defaults
- [`src/metadata.rs`](./src/metadata.rs): template variables source of truth

## Common recipes

### Show details instead of player name in Discord status

```toml
[player.default]
status_display_type = "details" # name | state | details
```

### Override one local player

```toml
[player.vlc_media_player]
app_id = "1124968989538402334"
icon = "https://upload.wikimedia.org/wikipedia/commons/thumb/e/e6/VLC_Icon.svg/1200px-VLC_Icon.svg.png"
show_icon = true
status_display_type = "details"
```

### Enable an unsupported local player

If `mprisence players list --detailed` shows your player but Discord does not update, add a matching entry. Replace `my_player` with the detected player identity.

```toml
[player.my_player]
name = "My Player"
ignore = false
# allow_streaming = true # only if the media URL is http/https
```

For bus names or multiple variants, use regex:

```toml
[player."re:.*mpdris2.*"]
name = "MPD"
ignore = false
```

### Ignore a player

```toml
[player.spotify]
ignore = true
```

### Enable YouTube video pages

YouTube Music is enabled by default. YouTube video pages are ignored by default.

```toml
[web_player.youtube]
ignore = false
```

### Add a custom web player

```toml
[web_player.last_fm]
match_pattern = "last.fm"
name = "Last.fm"
app_id = "YOUR_DISCORD_APP_ID"
icon = "https://example.com/last-fm.png"
allow_streaming = true
ignore = false
```

Inspect resolved config:

```bash
mprisence config
mprisence players list --detailed
```

## Cover art

mprisence can use cover art from metadata, local files, and online providers.

Default provider behavior prefers Catbox/Litterbox for local or embedded art, then falls back to other configured providers such as MusicBrainz or ImgBB.

### Catbox/Litterbox, no API key

```toml
[cover.provider]
provider = ["catbox"]

[cover.provider.catbox]
use_litter = true
litter_hours = 24
# user_hash = "your_user_hash" # optional
```

### ImgBB, API key required

```toml
[cover.provider]
provider = ["imgbb"]

[cover.provider.imgbb]
api_key = "YOUR_API_KEY_HERE"
```

Clear cover cache:

```bash
rm -rf ~/.cache/mprisence/cover_art
```

Authenticated or self-hosted art URLs from Feishin, Navidrome, Jellyfin, and similar players are treated as source images. mprisence re-hosts them through your configured providers before sending them to Discord.

## Web players

mprisence supports web players in two ways.

### Browser MPRIS

Some browsers publish the active media tab as an MPRIS player and include the page URL in `xesam:url`. mprisence matches that URL against `[web_player.*]` entries and applies the site config.

Use this path first if your browser already exposes good metadata.

### When to use the bridge

Use the extension bridge when browser MPRIS metadata is missing title, cover art, URL, duration, or reliable controls.

Bridge support currently covers:

- YouTube Music
- YouTube
- SoundCloud
- Bandcamp
- TIDAL
- Apple Music

Bundled web-player entries include:

- YouTube Music
- SoundCloud
- Apple Music
- Bandcamp
- TIDAL
- Deezer
- Qobuz
- Amazon Music
- Yandex Music
- YouTube, ignored by default

Most bundled web players are enabled. Unmatched HTTP/HTTPS browser URLs are ignored.

## Extension bridge setup

**Install the extension bridge only if browser MPRIS is not enough.**

You need:

- Rust/Cargo
- Node.js/npm
- Firefox or Chromium-based browser
- running `mprisence`

Build and install:

```bash
# Build native bridge
cargo build --release -p mprisence-web-bridge

# Register native messaging manifests
./target/release/mprisence-web-bridge install

# Verify native messaging setup
./target/release/mprisence-web-bridge doctor

# Build browser extension
cd extension
npm install
npm run build:firefox     # or: npm run build:chromium
cd ..
```

Load extension:

### Firefox

1. Open `about:debugging#/runtime/this-firefox`
2. Click **Load Temporary Add-on**
3. Select `extension/dist/firefox/manifest.json`

### Chromium, Chrome, Edge, Brave

1. Open `chrome://extensions`
2. Enable **Developer mode**
3. Click **Load unpacked**
4. Select `extension/dist/chromium/`

Verify bridge player:

```bash
mprisence
playerctl -l | grep mprisence_web
playerctl -p mprisence_web.youtube_music.* play-pause
```

Bridge logs:

```bash
tail -f /tmp/bridge-stderr.log
```

Uninstall bridge manifests:

```bash
./target/release/mprisence-web-bridge uninstall
```

Then remove the extension from your browser.

## Supported players

The full bundled support list lives in [`config.default.toml`](./config/config.default.toml).

Common local players:

- VLC
- MPV
- Audacious
- Elisa
- Lollypop
- Rhythmbox
- CMUS
- MPD
- Musikcube
- Clementine
- Strawberry
- Amberol
- SMPlayer
- Supersonic
- Feishin
- kew
- Quod Libet
- Euphonica

Streaming apps:

- YouTube Music
- Spotify, disabled by default

Browsers, disabled by default as generic players:

- Firefox
- Zen
- Chrome
- Edge
- Brave

Web players:

- YouTube Music
- SoundCloud
- Apple Music
- Bandcamp
- TIDAL
- Deezer
- Qobuz
- Amazon Music
- Yandex Music
- YouTube, ignored by default

MPD frontends can also expose MPD rich presence. Disable the MPD entry if you do not want both frontend and MPD activity.

Want a player name, icon, or web-player preset added? Open an issue with the player name, MPRIS identity, and website URL if relevant.

## CLI commands

```bash
# Help
mprisence --help

# Run foreground
mprisence

# List detected MPRIS players
mprisence players list

# Show detailed player metadata and matched config
mprisence players list --detailed

# Show resolved configuration
mprisence config

# Show version
mprisence version

# Verbose logs
RUST_LOG=debug mprisence
RUST_LOG=trace mprisence
```

## Troubleshooting

### Discord activity does not show

Check:

1. Discord desktop client is running.
2. Discord setting is enabled: **Settings → Activity Privacy → Display current activity as a status message**.
3. `mprisence` is running:

   ```bash
   systemctl --user status mprisence
   ```

4. Your player is detected:

   ```bash
   mprisence players list
   ```

5. Logs show no errors:

   ```bash
   journalctl --user -u mprisence -f
   ```

If you use Vesktop Flatpak, set up the Discord IPC symlink from the Vesktop Flatpak guide: [Native applications](https://github.com/flathub/dev.vencord.Vesktop?tab=readme-ov-file#native-applications).

### Player appears in `players list`, but Discord does not update

The player may be ignored by config.

Run:

```bash
mprisence players list --detailed
mprisence config
```

Then add or override a `[player.*]` entry.

### Browser tab does not show

Check which path you use:

- Browser MPRIS: browser must expose media as an MPRIS player with URL metadata.
- Extension bridge: extension must be loaded, native manifest installed, and supported site open.

Useful checks:

```bash
playerctl -l
playerctl -l | grep mprisence_web
./target/release/mprisence-web-bridge doctor
tail -f /tmp/bridge-stderr.log
```

### Cover art missing

Check:

1. Run with debug logs:

   ```bash
   RUST_LOG=debug mprisence
   ```

2. Confirm provider config.
3. Confirm MusicBrainz can match title, artist, and album.
4. Confirm ImgBB API key if using ImgBB.
5. Clear stale cache:

   ```bash
   rm -rf ~/.cache/mprisence/cover_art
   ```

Cover art order:

1. cache
2. direct URL from metadata
3. local files
4. configured providers

### Config changes do not apply

Most changes hot-reload. If unsure, restart:

```bash
systemctl --user restart mprisence
```

Validate TOML syntax with a TOML validator or `toml-lint`.

To test defaults, temporarily move your config:

```bash
mv ~/.config/mprisence/config.toml ~/.config/mprisence/config.toml.bak
mprisence
```

## Contributing

Issues and pull requests are welcome.

For bridge internals, see [`docs/web-bridge-design.md`](./docs/web-bridge-design.md).

## License

MIT. See [`LICENSE`](./LICENSE).
