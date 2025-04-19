# mprisence

[![AUR version](https://img.shields.io/aur/version/mprisence)](https://aur.archlinux.org/packages/mprisence)
[![GitHub license](https://img.shields.io/github/license/lazykern/mprisence)](https://github.com/lazykern/mprisence/blob/main/LICENSE)
[![GitHub stars](https://img.shields.io/github/stars/lazykern/mprisence)](https://github.com/lazykern/mprisence/stargazers)

A highly customizable Discord Rich Presence client for MPRIS media players. Supports VLC, MPV, RhythmBox, and many other Linux music and media players.

![mprisence Example](assets/example.gif)

*(Note: Actual appearance depends on your configuration and the specific media player)*

## Preconfigured Players

Ready to use with popular media players (configured in [`config.default.toml`](./config/config.default.toml)):

- **Media Players**: VLC, MPV, Audacious, Elisa, Lollypop, Rhythmbox, CMUS, MPD, Musikcube, Clementine, Strawberry, Amberol, SMPlayer
- **Streaming**: YouTube Music, Spotify (disabled by default)
- **Browsers** (disabled by default): Firefox, Zen, Chrome, Edge, Brave

## Features

- **Universal Media Player Support**: Works with any MPRIS-compatible Linux media player.
- **Advanced Templating**: Utilize the power of Handlebars templates for complete control over your presence text, including conditionals and helpers.
- **Sophisticated Cover Art**: Fetches cover art from MusicBrainz, uploads local art via ImgBB (optional), caches results, and uses metadata intelligently.
- **Live Configuration (Hot Reload)**: Change your `config.toml` and see the updates reflected instantly without restarting the service.
- **Content-Aware Activity**: Automatically sets your Discord status to "Listening", "Watching", etc., based on the media type (configurable).
- **Player-Specific Settings**: Customize Discord App IDs, icons, and behavior for individual players.
- **Detailed Metadata**: Access a rich set of metadata (including technical audio details) within your templates.

## Prerequisites

- **For running:** A desktop environment with an active D-Bus session (standard on most Linux desktops).
- **For service management:** `systemd` (user instance).
- **For manual installation/building from source:**
    - `rustc` and `cargo` (latest stable version recommended)
    - `make`
    - `git` (to clone the repository)

## Installation

### Arch Linux
```bash
# Install from AUR
yay -S mprisence
```

### Manual Installation
```bash
# Clone the repository
git clone https://github.com/lazykern/mprisence.git
cd mprisence

# Build and install (includes service activation by default)
make

# Install without enabling service
make install-local ENABLE_SERVICE=0

# Uninstall
make uninstall-local
```

*If you installed using `make` (without `ENABLE_SERVICE=0`), the systemd service will be enabled. See the next section for how to manage it.*

## Autostarting / Service Management

If you installed using `make` or enabled the service manually, `mprisence` will run as a systemd user service.

You can manage the service using `systemctl --user`:

```bash
# Check service status
systemctl --user status mprisence

# Start the service
systemctl --user start mprisence

# Stop the service
systemctl --user stop mprisence

# Restart the service
systemctl --user restart mprisence

# Enable the service to start on login
systemctl --user enable mprisence

# Disable the service from starting on login
systemctl --user disable mprisence

# View detailed logs
journalctl --user -u mprisence -f
```

## Configuration

`mprisence` is highly configurable via `~/.config/mprisence/config.toml` (or `$XDG_CONFIG_HOME/mprisence/config.toml`).

### Getting Started
1.  **Ensure the configuration directory exists:**
    ```bash
    mkdir -p ~/.config/mprisence
    ```
2.  **Copy the example configuration:**
    *   **If installed via AUR or package manager:**
        ```bash
        # The exact path might vary slightly depending on the package.
        # Check with 'pacman -Ql mprisence | grep config.example.toml' if unsure.
        cp /usr/share/mprisence/config.example.toml ~/.config/mprisence/config.toml
        ```
    *   **If built manually from source:** Navigate to the cloned repository directory and run:
        ```bash
        cp config/config.example.toml ~/.config/mprisence/config.toml
        ```
    *   **Alternatively, download the latest example directly:**
        ```bash
        curl -o ~/.config/mprisence/config.toml https://raw.githubusercontent.com/lazykern/mprisence/main/config/config.example.toml
        ```

    This file contains detailed explanations of all options.
3.  **Modify** `~/.config/mprisence/config.toml` to your liking.
4.  You can refer to `config.default.toml` (in the source repository or installation files) to see the default settings applied to specific players if you don't override them.

### Configuration Reference
- [`config.example.toml`](./config.example.toml): Detailed options and explanations.
- [`config.default.toml`](./config/config.default.toml): Default configurations for popular players.
- `src/metadata.rs`: Definitive source for all available template variables.

---

### Key Template Variables
Some commonly used variables available in templates:

- `{{player}}`: Name of the media player (e.g., `vlc`, `spotify`).
- `{{status}}`: Playback status (`Playing`, `Paused`, `Stopped`).
- `{{status_icon}}`: Icon representing the status (`▶`, `⏸`, `⏹`).
- `{{title}}`: Track title.
- `{{artists}}`: List of track artists.
- `{{artist_display}}`: Comma-separated track artists.
- `{{album}}`: Album title.
- `{{album_artists}}`: List of album artists.
- `{{album_artist_display}}`: Comma-separated album artists.
- `{{year}}`: Release year.
- `{{duration_display}}`: Formatted track duration (e.g., `03:45`).
- `{{track_display}}`: Formatted track number (e.g., `1/12`).

*(See `config.example.toml` or `src/metadata.rs` for the complete list)*

---

### Basic Configuration Example
```toml
# Basic settings
# Whether to clear Discord activity when media is paused
clear_on_pause = true

# How often to update Discord presence (in milliseconds)
interval = 2000

# Display template
[template]
# First line in Discord presence
detail = "{{{title}}}"

# Second line in Discord presence - using Handlebars array iteration
state = "{{#each artists}}{{this}}{{#unless @last}} & {{/unless}}{{/each}}"
# or just use
# state = "{{{artist_display}}}"

# Text shown when hovering over the large image - using conditional helpers
large_text = "{{#if album}}{{{album}}}{{#if year}} ({{year}}){{/if}}{{#if album_artist_display}} by {{{album_artist_display}}}{{/if}}{{/if}}"

# Text shown when hovering over the small image - using equality helper
small_text = "{{#if (eq status \"playing\")}}▶{{else}}⏸{{/if}} on {{{player}}}"

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

---

### Cover Art Setup
```toml
[cover.provider]
# Cover art providers in order of preference
# (imgbb will be used as a fallback if musicbrainz fails)
provider = ["musicbrainz", "imgbb"]

[cover.provider.imgbb]
# Your ImgBB API key (get one at: https://api.imgbb.com/)
api_key = "YOUR_API_KEY_HERE"
# How long to keep uploaded images (in seconds, default: 1 day)
expiration = 86400
```

---

### Player-Specific Configuration
```toml
# Use 'mprisence players list' to get the correct player identity
[player.vlc_media_player]
# Discord application ID (get yours at: https://discord.com/developers/docs/quick-start/overview-of-apps)
app_id = "YOUR_APP_ID_HERE"
# Player icon URL (shown as small image)
icon = "https://example.com/vlc-icon.png"
# Show player icon in Discord as small image
show_icon = true
# Allow Discord presence for web/streaming content
allow_streaming = true
# Override activity type for this player
override_activity_type = "listening"
```

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

### Common Issues

1.  **Discord Presence Not Showing / Updating**
    *   **Is Discord running?** Ensure the Discord desktop client is open.
    *   **Is your player running and MPRIS-compatible?** Run `mprisence players list` to see detectable players.
    *   **Is the service running?** `systemctl --user status mprisence`
    *   **Discord Settings:** Check `Discord Settings -> Registered Games / Activity Privacy`. Ensure `Display current activity as a status message.` is ON. Sometimes toggling this off and on helps. Add `mprisence` if it's not listed.
    *   **Correct App ID?** Verify the `app_id` in your config matches a valid Discord application ID.
    *   **Logs:** Check `journalctl --user -u mprisence -f` or run `RUST_LOG=debug mprisence` for errors.

2.  **Cover Art Not Displaying**
    *   **Check the logs:** Run with `RUST_LOG=debug mprisence` to see the cover art process.
    *   **Provider Order:** Cover art is checked in this order: Cache -> Direct URL (from metadata) -> Local Files -> Configured Providers (e.g., MusicBrainz, ImgBB).
    *   **MusicBrainz:** Does the track metadata (title, artist, album) accurately match the MusicBrainz database? Check the `min_score` in your config.
    *   **ImgBB:**
        *   Is a local file available (embedded or matching `file_names` in the folder/parent folders)? ImgBB is primarily used to *upload* local art.
        *   Is your `api_key` in `[cover.provider.imgbb]` correct and valid?
        *   Is the image file format supported and readable?
    *   **Cache:** Try clearing the cache (`rm -rf ~/.cache/mprisence/cover_art`) if you suspect stale entries.

3.  **Service Issues**
    ```bash
    # Check service status
    systemctl --user status mprisence

    # View detailed logs
    journalctl --user -u mprisence

    # Restart service (e.g., after config changes)
    systemctl --user restart mprisence
    ```

4.  **Configuration Issues**
    *   **Syntax Errors:** Validate your `config.toml` using an online TOML validator or `toml-lint`.
    *   **Logs:** Check for parsing errors: `journalctl --user -u mprisence` or `RUST_LOG=debug mprisence`.
    *   **Incorrect Player Identity:**  Ensure the `[player.<identity>]` section uses the **exact, normalized** identity shown by running `mprisence players list`. Player names are typically converted to lowercase, and special characters/spaces are replaced with underscores (e.g., "VLC media player" becomes `vlc_media_player`).
    *   **Defaults:** If unsure, temporarily remove your `~/.config/mprisence/config.toml` to test with the built-in defaults.

## Contributing

Contributions are welcome! Please feel free to open an issue to report bugs, suggest features, or discuss changes. If you'd like to contribute code, please open a pull request.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
