# mprisence

[![AUR version](https://img.shields.io/aur/version/mprisence)](https://aur.archlinux.org/packages/mprisence)
[![Nixpkgs](https://img.shields.io/badge/NixOS-nixpkgs-blue?logo=nixos)](https://search.nixos.org/packages?query=mprisence)

Highly customizable Discord Rich Presence client for MPRIS media players. Supports VLC, MPV, RhythmBox, and many other Linux music and media players.

<img src="/assets/example.gif" width="548" height="548"/>

_(Note: Actual appearance depends on your configuration and the specific media player)_

## Preconfigured Players

Ready to use with popular media players (configured in [`config.default.toml`](./config/config.default.toml)):

- **Media Players**: VLC, MPV, Audacious, Elisa, Lollypop, Rhythmbox, CMUS, MPD, Musikcube, Clementine, Strawberry, Amberol, SMPlayer, Supersonic, Feishin, kew, Quod Libet, Euphonica
- **Streaming**: YouTube Music, Spotify (disabled by default)
- **Browsers** (disabled by default): Firefox, Zen, Chrome, Edge, Brave
- **Web player overrides (overlays on top of any browser)**: YouTube Music, SoundCloud, Apple Music, Bandcamp, TIDAL, Spotify Web (disabled by default). When `xesam:url` matches a `[web_player.*]` entry, the web player's icon and Discord application replace the browser's, so Discord shows e.g. *Listening to YouTube Music* instead of *Firefox*. See [Web Player Overrides](#web-player-overrides) below.
- **Web Player Integration:** Bridge browser media players (YouTube Music, YouTube, SoundCloud, Bandcamp, Tidal, Apple Music) into MPRIS via a native messaging host + browser extension, unlocking MPRIS controls and Discord presence for web media. See [Web Player Integration](#web-player-integration).

Note: MPD frontends (e.g., Euphonica) will also show MPD rich presence in Discord; you can disable the MPD entry in your config (see [Configuration Reference](#configuration-reference)

Feel free to create a new issue if you want your player name+icon to be recognized by mprisence!

## Features

- **Works with any MPRIS player** (VLC, MPV, Rhythmbox, etc.)
- **Template-driven presence (Handlebars)**: full control over details/state text, with helpers + conditionals
- **Custom status display**: choose what Discord shows as your status (`name`, `state`, or `details`) — globally or per player
- **Cover art**: uses metadata, local files, and online providers (with caching)
- **Hot reload**: most config edits apply instantly (no restart)
- **Smart activity type**: “Listening” / “Watching” / etc. based on content (configurable)
- **Per-player overrides**: app IDs, icons, status, and more
- **Rich metadata**: access detailed fields (including technical audio info) inside templates

---

## Web Player Integration

A browser extension + native messaging host (`mprisence-web-bridge`) that bridges web media players into MPRIS. The extension reads playback metadata from page DOM, the bridge publishes per-tab MPRIS players on D-Bus, and mprisence discovers them like any other media player.

**Supported sites:** YouTube Music, YouTube, SoundCloud, Bandcamp, Tidal, Apple Music

### Build & Install

```bash
# 1. Build the native bridge
cargo build --release -p mprisence-web-bridge

# 2. Register native messaging manifests (so browser trusts the bridge)
./target/release/mprisence-web-bridge install

# 3. Verify setup
./target/release/mprisence-web-bridge doctor

# 4. Build the browser extension
cd extension
npm install
npm run build:firefox   # or: npm run build:chromium
cd ..
```

### Load extension in browser

Run these steps once per browser session (extension loads temporarily until browser restart):

**Firefox:**
1. Navigate to `about:debugging#/runtime/this-firefox`
2. Click **Load Temporary Add-on**
3. Select `extension/dist/firefox/manifest.json`

**Chromium (Chrome, Edge, Brave):**
1. Navigate to `chrome://extensions`
2. Enable **Developer mode** (top-right)
3. Click **Load unpacked**
4. Select the `extension/dist/chromium/` folder

### Run

```bash
# 1. Start the mprisence daemon
mprisence

# 2. The browser auto-launches the bridge when the extension connects
#    (visible in /tmp/bridge-stderr.log)

# 3. Visit a supported site and play media
#    You should see MPRIS players appear:
playerctl -l | grep mprisence_web

# 4. Test controls:
playerctl -p mprisence_web.youtube_music.* play-pause
playerctl -p mprisence_web.youtube_music.* position 30+
```

### Configuration

Web players are **disabled by default**. Enable them in your mprisence config:

```toml
[web_player.default]
ignore = true

[web_player.youtube_music]
match_patterns = ["music.youtube.com"]
ignore = false
app_id = "1121632048155742288"  # optional: custom Discord app ID
```

See `config/config.example.toml` for all available web_player entries.

### Logs

```bash
tail -f /tmp/bridge-stderr.log
```

### Uninstall

```bash
./target/release/mprisence-web-bridge uninstall
# Then remove the extension from the browser (Extensions page)
```

---

## Prerequisites

- **For running:** A desktop environment with an active D-Bus session (standard on most Linux desktops).
- **For service management:** `systemd` (user instance).
- **For manual installation/building from source:**
  - `rustc` and `cargo` (latest stable version recommended)
  - `git` (to clone the repository)

## Installation and Setup

<details>
<summary><b>Expand installation and setup steps</b></summary>

### Package Manager

#### Arch Linux

```bash
# Install the stable version
yay -S mprisence

# Or, install the latest development version
yay -S mprisence-git

# Or without building from source
yay -S mprisence-bin
```

#### Nix (NixOS, Linux)

Available in `nixpkgs`

```bash
# without flakes:
nix-env -iA nixpkgs.mprisence

# with flakes:
nix profile install nixpkgs#mprisence
```

NixOS configuration:

```nix
environment.systemPackages = [ pkgs.mprisence ];
```

#### Debian, Ubuntu, and derivatives

Download the `.deb` package from the [**GitHub Releases page**](https://github.com/lazykern/mprisence/releases) and install it:

```bash
sudo dpkg -i /path/to/mprisence_*.deb
```

### Manual Installation

This method is for other Linux distributions, or if you prefer to install from source or crates.io. It requires a few manual setup steps.

#### Step 1: Install the `mprisence` binary

Choose **one** of the following ways to get the executable:

<details>
<summary><b>Option A: From Crates.io (requires Rust)</b></summary>

```bash
cargo install mprisence
```

This will install the binary to `~/.cargo/bin/`. Ensure this directory is in your `$PATH`.

</details>

<details>
<summary><b>Option B: From GitHub Releases (pre-compiled)</b></summary>

Download the `...-unknown-linux-gnu.tar.gz` archive from the [**GitHub Releases page**](https://github.com/lazykern/mprisence/releases). Extract it, and place the `mprisence` binary in a directory included in your system's `$PATH` (e.g., `~/.local/bin` or `/usr/local/bin`).

</details>

<details>
<summary><b>Option C: From Source (for development)</b></summary>

```bash
# Clone the repository
git clone https://github.com/lazykern/mprisence.git
cd mprisence

# Install from local source
cargo install --path .
```

This also installs the binary to `~/.cargo/bin/`.

</details>

#### Step 2: Set up Configuration

`mprisence` looks for its configuration at `~/.config/mprisence/config.toml`.

1. **Create the configuration directory:**

   ```bash
   mkdir -p ~/.config/mprisence
   ```

2. **Download the example configuration:**

   ```bash
   curl -o ~/.config/mprisence/config.toml https://raw.githubusercontent.com/lazykern/mprisence/main/config/config.example.toml
   ```

   Now you can edit this file to customize mprisence. See the [Configuration Reference](#configuration-reference) section for more details.

#### Step 3: Set up and Run the Service

To have `mprisence` start automatically on login, set up the systemd user service.

1. **Create the systemd user directory if it doesn't exist:**

   ```bash
   mkdir -p ~/.config/systemd/user
   ```

2. **Download the service file:**
   The provided service file is configured to find the `mprisence` binary in `~/.cargo/bin/`.

   ```bash
   curl -o ~/.config/systemd/user/mprisence.service https://raw.githubusercontent.com/lazykern/mprisence/main/mprisence.service
   ```

   > **Note:** If you placed the binary in a different location (e.g., `/usr/local/bin`), you must edit `~/.config/systemd/user/mprisence.service` and change the `ExecStart` path.

3. **Enable and start the service:**

   ```bash
   systemctl --user enable --now mprisence
   ```

   This command enables `mprisence` to start at login and starts it immediately.

### Managing the Service

Once the service is installed (either manually or via a package), you can manage it using `systemctl --user`:

```bash
# Check service status
systemctl --user status mprisence

# Restart the service after changing the config
systemctl --user restart mprisence

# View detailed logs
journalctl --user -u mprisence -f

# Stop and disable the service
systemctl --user disable --now mprisence
```

</details>

## Configuration

`mprisence` is highly configurable via `~/.config/mprisence/config.toml` (or `$XDG_CONFIG_HOME/mprisence/config.toml`).

After following the installation steps, you can modify `~/.config/mprisence/config.toml` to your liking. The application will hot-reload most configuration changes automatically.

### Local Album Covers

By default, mprisence prefers Catbox uploads through Litterbox first for local/embedded cover art, then falls back to other configured providers such as MusicBrainz or ImgBB.

Update the provider order to include whichever host you prefer (e.g., `["catbox", "musicbrainz", "imgbb"]`, `["catbox"]`, etc.).

**Catbox (no key required)**

```toml
[cover.provider]
provider = ["catbox"]

[cover.provider.catbox]
# user_hash = "your_user_hash" # optional: lets you delete uploads later
use_litter = true             # default: upload to temporary Litterbox before permanent Catbox storage
litter_hours = 24             # valid values: 1, 12, 24, 72
```

**ImgBB (API key required)**

```toml
[cover.provider]
provider = ["imgbb"]

[cover.provider.imgbb]
api_key = "YOUR_API_KEY_HERE"
```

Notes:

- Clear cache: `rm -rf ~/.cache/mprisence/cover_art`.
- Authenticated/self-hosted art URLs from players like Feishin (Subsonic/OpenSubsonic/Navidrome/Jellyfin API image routes) are treated as source input and re-hosted via your configured providers instead of being cached as direct Discord URLs.

### Custom Status Display

Use `status_display_type` to control which text Discord shows in your status.

| `status_display_type`                                                                 | Preview                                                     |
| ------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| `name`: shows the player/app name in your status                                      | ![Status display type name](https://raw.githubusercontent.com/lazykern/mprisence/main/assets/status_name.png)       |
| `state`: shows the rendered `template.state` value (default template shows artist(s)) | ![Status display type state](https://raw.githubusercontent.com/lazykern/mprisence/main/assets/status_state.png)     |
| `details`: shows the rendered `template.details` value (default template shows title) | ![Status display type details](https://raw.githubusercontent.com/lazykern/mprisence/main/assets/status_details.png) |

Set a global default in `[player.default]`, then override per player only when needed. With the bundled app ID, `name` would show `mprisence`, so it falls back to `state`.

```toml
# Global default for all players
[player.default]
status_display_type = "name" # name | state | details

# Optional per-player override (this one only affects VLC)
[player.vlc_media_player]
status_display_type = "details"
```

### Web Player Overrides

Browsers like Firefox, Zen or Chrome publish the playing tab's URL through MPRIS (`xesam:url`). mprisence inspects that URL and, when its host matches a `[web_player.<key>]` entry, **uses that entry as the authoritative configuration for the player** — so listening to YouTube Music in any browser shows up as *Listening to YouTube Music* in Discord instead of *Firefox* or *Zen*.

The web player entry fully replaces the browser's `[player.*]` config for that URL: `ignore`, `allow_streaming`, `app_id`, `icon`, `name`, and activity type all come from the matched entry. You do not need to also enable or unhide the browser's `[player.*]` entry — adding a `[web_player.*]` is enough.

When the URL changes (e.g. you navigate from `music.youtube.com` to `soundcloud.com`), mprisence transparently recycles its Discord IPC client so the new application's name and icon take effect.

Bundled entries: YouTube, YouTube Music, SoundCloud, Apple Music, Bandcamp, TIDAL, Deezer, Qobuz, Amazon Music, Yandex Music, Pocket Casts, Apple Podcasts, Podurama (each ships with a placeholder app ID you can replace with your own registered Discord application), and Spotify Web (ships with `ignore = true`).

```toml
# Top-level table — sibling to [player.*].
[web_player.youtube_music]
match_pattern = "music.youtube.com"      # exact host, `*?` wildcards, or `re:` / `/.../` regex
app_id        = "1125082278339559505"    # your Discord application id
icon          = "https://…/yt-music.png"
allow_streaming = true                   # required since the source URL is HTTPS
ignore        = false                    # set true to skip presence for this site

# Per-user override: enable Spotify Web (bundled config ships ignore = true).
# Patterns are inherited from the bundled entry — you only need to write the
# fields you want to change.
[web_player.spotify_web]
ignore = false
app_id = "YOUR_SPOTIFY_WEB_APP_ID"
```

Notes:

- User entries merge with bundled entries: fields you leave unset fall through to the bundled entry (including `match_pattern`/`match_patterns`). A `[web_player.youtube] ignore = false` with no patterns still matches `youtube.com`.
- Any http/https URL that does NOT match a `[web_player.*]` entry is auto-ignored. Opt back in by adding an entry.
- Inspect resolved entries with `mprisence config` or `mprisence players list -d`.

### Configuration Reference

- [`config.example.toml`](./config/config.example.toml): Detailed options and explanations.
- [`config.default.toml`](./config/config.default.toml): Default configurations for popular players.
- [`src/metadata.rs`](./src/metadata.rs): Definitive source for all available template variables.

## CLI Commands

```bash
# Get help
mprisence --help

# Run without system service
mprisence

# List available MPRIS players
mprisence players list

# Show detailed player information including metadata and config
mprisence players list --detailed

# Show current configuration
mprisence config

# Show version
mprisence version

# Enable more verbose logging
RUST_LOG=debug mprisence # or RUST_LOG=trace mprisence
```

## Troubleshooting

<details>
<summary><b>Expand troubleshooting tips</b></summary>

### Common Issues

1. **Discord Presence Not Showing / Updating**
   - **Is your player running and MPRIS-compatible?** Run `mprisence players list` to see detectable players.
   - **Is the service running?** `systemctl --user status mprisence`
   - **Discord Settings:** Check `Discord Settings -> Registered Games / Activity Privacy`. Ensure `Display current activity as a status message.` is ON. Sometimes toggling this off and on helps. Add `mprisence` if it's not listed.
   - **Correct App ID?** Verify the `app_id` in your config matches a valid Discord application ID.
   - **Using Vesktop Flatpak?** Set up the Discord IPC symlink as described in the Vesktop Flatpak guide: [Native applications](https://github.com/flathub/dev.vencord.Vesktop?tab=readme-ov-file#native-applications).
   - **Logs:** Check `journalctl --user -u mprisence -f` or run `RUST_LOG=debug mprisence` for errors.

2. **Cover Art Not Displaying**
   - **Check the logs:** Run with `RUST_LOG=debug mprisence` to see the cover art process.
   - **Provider Order:** Cover art is checked in this order: Cache -> Direct URL (from metadata) -> Local Files -> Configured Providers (default: Catbox/Litterbox first, then MusicBrainz).
   - **MusicBrainz:** Does the track metadata (title, artist, album) accurately match the MusicBrainz database? Check the `min_score` in your config.
   - **ImgBB:**
     - Is a local file available (embedded or matching `file_names` in the folder/parent folders)? ImgBB is primarily used to _upload_ local art.
     - Is your `api_key` in `[cover.provider.imgbb]` correct and valid?
     - Is the image file format supported and readable?
   - **Cache:** Try clearing the cache (`rm -rf ~/.cache/mprisence/cover_art`) if you suspect stale entries.

3. **Service Issues**
   - Use the commands mentioned in the [Autostarting / Service Management](#autostarting--service-management) section to check status (`status`), view logs (`journalctl`), and manage the service (`start`, `stop`, `restart`).

4. **Configuration Issues**
   _**Syntax Errors:** Validate your `config.toml` using an online TOML validator or `toml-lint`.
   _ **Defaults:** If unsure, temporarily remove your `~/.config/mprisence/config.toml` to test with the built-in defaults.

</details>

## Contributing

Contributions are welcome! Please feel free to open an issue to report bugs, suggest features, or discuss changes. If you'd like to contribute code, please open a pull request.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
