# mprisence

[![AUR version](https://img.shields.io/aur/version/mprisence)](https://aur.archlinux.org/packages/mprisence)
[![Nixpkgs](https://img.shields.io/badge/NixOS-nixpkgs-blue?logo=nixos)](https://search.nixos.org/packages?query=mprisence)

Highly customizable Discord Rich Presence client for MPRIS media players. Supports VLC, MPV, RhythmBox, and many other Linux music and media players.

![mprisence Example](https://raw.githubusercontent.com/lazykern/mprisence/main/assets/example.gif)

_(Note: Actual appearance depends on your configuration and the specific media player)_

## Preconfigured Players

Ready to use with popular media players (configured in [`config.default.toml`](./config/config.default.toml)):

- **Media Players**: VLC, MPV, Audacious, Elisa, Lollypop, Rhythmbox, CMUS, MPD, Musikcube, Clementine, Strawberry, Amberol, SMPlayer, Supersonic, Feishin, kew, Quod Libet, Euphonica
- **Streaming**: YouTube Music, Spotify (disabled by default)
- **Browsers** (disabled by default): Firefox, Zen, Chrome, Edge, Brave

Note: MPD frontends (e.g., Euphonica) will also show MPD rich presence in Discord; you can disable the MPD entry in your config (see [Configuration Reference](#configuration-reference)

Feel free to create a new issue if you want your player name+icon to be recognized by mprisence!

## Features

- **Works with any MPRIS player** (VLC, MPV, Rhythmbox, etc.)
- **Template-driven presence (Handlebars)**: full control over details/state text, with helpers + conditionals
- **Custom status display**: choose what Discord shows as your status (`name`, `state`, or `details`) — globally or per player
- **Cover art**: uses metadata, local files, and online providers (with caching)
- **Hot reload**: most config edits apply instantly (no restart)
- **Smart activity type**: “Listening” / “Watching” / etc. based on content (configurable)
- **Per-player overrides**: app IDs, icons, streaming rules, and behavior
- **Rich metadata**: access detailed fields (including technical audio info) inside templates

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

mprisence will first attempt to find cover art from MusicBrainz. If it's not found, it can re-host local cover art through Catbox (no key required) or ImgBB (requires an API key).

Update the provider order to include whichever host you prefer (e.g., `["musicbrainz", "catbox", "imgbb"]`, `["catbox"]`, etc.).

**Catbox (no key required)**

```toml
[cover.provider]
provider = ["catbox"]

[cover.provider.catbox]
# user_hash = "your_user_hash" # optional: lets you delete uploads later
use_litter = false            # true -> upload to Litterbox instead of permanent Catbox storage
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

### Custom Status Display

Use `status_display_type` to control which text Discord shows in your status.

| `status_display_type`                                                                 | Preview                                                     |
| ------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| `name`: shows the player/app name in your status                                      | ![Status display type name](./assets/status_name.png)       |
| `state`: shows the rendered `template.state` value (default template shows artist(s)) | ![Status display type state](./assets/status_state.png)     |
| `details`: shows the rendered `template.details` value (default template shows title) | ![Status display type details](./assets/status_details.png) |

Set a global default in `[player.default]`, then override per player only when needed.

```toml
# Global default for all players
[player.default]
status_display_type = "name" # name | state | details

# Optional per-player override (this one only affects VLC)
[player.vlc_media_player]
status_display_type = "details"
```

### Configuration Reference

- [`config.example.toml`](./config/config.example.toml): Detailed options and explanations.
- [`config.default.toml`](./config/config.default.toml): Default configurations for popular players.
- [`src/metadata.rs`](./src/metadata.rs): Definitive source for all available template variables.

---

<details>
<summary>Basic Configuration Example</summary>

```toml
# Basic settings
# Whether to clear Discord activity when media is paused
clear_on_pause = true

# How often to update Discord presence (in milliseconds)
interval = 2000

# Restrict discovery to specific players (identities, wildcards, or regex).
# Matches against player identity or the bus-name "player" part (e.g., "vlc"),
# not the full D-Bus name. Leave empty to allow all players.
allowed_players = []

# Note: Triple braces `{{{variable}}}` are used to prevent HTML escaping,
# which is generally desired for Discord presence fields.
# See: https://handlebarsjs.com/guide/#html-escaping

# Display template
[template]
# First line in Discord presence
details = "{{{title}}}"

# Second line in Discord presence - using Handlebars array iteration
state = "{{#each artists}}{{this}}{{#unless @last}} & {{/unless}}{{/each}}"
# or just use
# state = "{{{artist_display}}}"

# Text shown when hovering over the large image - using conditional helpers
large_text = "{{#if album}}{{{album}}}{{#if year}} ({{{year}}}){{/if}}{{#if album_artist_display}} by {{{album_artist_display}}}{{/if}}{{/if}}"

# Text shown when hovering over the small image (player icon)
# Only visible when show_icon = true in player settings
small_text = "{{#if player}}Playing on {{{player}}}{{else}}MPRIS{{/if}}"

# Activity type settings
[activity_type]
# Auto-detect type (audio -> "listening", video -> "watching")
use_content_type = true
# Default type: "listening", "watching", "playing", or "competing"
default = "listening"

# Time display settings
[time]
# Show progress bar/time in Discord
show = true
# true = show elapsed time, false = show remaining time
as_elapsed = true
```

</details>

---

<details>
<summary>Cover Art Setup</summary>

```toml
[cover]
# File names (without extension) to search for local art (e.g., cover.jpg, folder.png)
file_names = ["cover", "folder", "front", "album", "art"]
# How many parent directories to search upwards for local art (0 = same dir only)
local_search_depth = 2

[cover.provider]
# Cover art providers in order of preference
# (catbox will be used as a fallback if musicbrainz fails or local art isn't found)
provider = ["musicbrainz", "catbox"] # Also checks local files first based on above

[cover.provider.musicbrainz]
# Minimum score (0-100) for MusicBrainz matches. Higher = stricter.
min_score = 100

[cover.provider.catbox]
# user_hash = "your_user_hash" # optional: lets you delete uploads later
use_litter = false            # true -> upload to Litterbox instead of permanent Catbox storage
litter_hours = 24             # valid values: 1, 12, 24, 72

[cover.provider.imgbb]
# Your ImgBB API key (get one at: https://api.imgbb.com/)
api_key = "YOUR_API_KEY_HERE"
# How long to keep uploaded images (in seconds, default: 1 day)
expiration = 86400
```

</details>

---

<details>
<summary>Player-Specific Configuration</summary>

```toml
# Use 'mprisence players list' to get the correct player identity (e.g., vlc_media_player)

# Default settings applied to ALL players unless overridden below
[player.default]
ignore = false # Set to true to disable presence for all players by default
app_id = "1121632048155742288" # Default Discord Application ID
icon = "https://raw.githubusercontent.com/lazykern/mprisence/main/assets/icon.png" # Default icon URL
show_icon = false # Show player icon as small image by default?
allow_streaming = false # Allow HTTP/HTTPS streaming content? False clears Discord activity for those players.
status_display_type = "name" # Controls which text Discord shows in your status.
                              # For example:
                              # "name"    -> Player/app name
                              # "state"   -> Rendered template.state value (default: "{{{artists}}}")
                              # "details" -> Rendered template.details value (default: "{{{title}}}")

# Override settings for a specific player (VLC in this example)
[player.vlc_media_player]
# You can override any key from [player.default] here
app_id = "YOUR_VLC_APP_ID_HERE" # Use a VLC-specific Discord App ID
icon = "https://example.com/vlc-icon.png" # Use a VLC-specific icon
show_icon = true # Show the VLC icon
allow_streaming = true # Allow streaming content for VLC
# You could also add 'override_activity_type = "watching"' here if desired
status_display_type = "details"

# Example: Ignore Spotify
# [player.spotify]
# ignore = true

# Example: Wildcard matches
[player."*youtube_music*"]
show_icon = true
allow_streaming = true
```

When `allow_streaming` is `false`, mprisence will skip HTTP/HTTPS sources for that player and clear any previously published Discord activity so browsers stay hidden unless explicitly enabled.

</details>

Player config priority: user entries always override bundled ones. The resolver tries user exact > user regex > user wildcard > bundled exact > bundled regex > bundled wildcard; any field left unset on a higher-priority match is filled from the next match down, falling back to `[player.default]` (user, then bundled) and finally built-in defaults.

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
   - **Provider Order:** Cover art is checked in this order: Cache -> Direct URL (from metadata) -> Local Files -> Configured Providers (e.g., MusicBrainz, ImgBB).
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
