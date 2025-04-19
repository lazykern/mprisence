# mprisence 

[![AUR version](https://img.shields.io/aur/version/mprisence)](https://aur.archlinux.org/packages/mprisence)
[![GitHub license](https://img.shields.io/github/license/lazykern/mprisence)](https://github.com/lazykern/mprisence/blob/main/LICENSE)
[![GitHub stars](https://img.shields.io/github/stars/lazykern/mprisence)](https://github.com/lazykern/mprisence/stargazers)

A highly customizable Discord Rich Presence client for MPRIS media players. Supports VLC, MPV, RhythmBox, and many other Linux music and media players.

![mprisence Example](assets/example.gif)

*(Note: Actual appearance depends on your configuration and the specific media player)*

## Features

**Display your Linux media player activity on Discord**

- **Universal Media Player Support**: Works with any MPRIS-compatible Linux media player.
- **Hot Reload**: Configuration changes are applied instantly without restart
- **Cover Art Integration**: Supports local files, MusicBrainz, and ImgBB hosting
-  **Content Based Activity Type**: Shows "Listening" for music, "Watching" for videos automatically (configurable)

## Preconfigured Players

Ready to use with popular media players (configured in [`config.default.toml`](./config/config.default.toml)):

- **Media Players**: VLC, MPV, Audacious, Elisa, Lollypop, Rhythmbox, CMUS, MPD, Musikcube, Clementine, Strawberry, Amberol, SMPlayer
- **Streaming**: YouTube Music, Spotify (disabled by default)
- **Browsers** (disabled by default): Firefox, Zen, Chrome, Edge, Brave

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

# Build and install (includes service activation)
make

# Install without enabling service
make install-local ENABLE_SERVICE=0

# Uninstall
make uninstall-local
```

See [Autostarting / Service Management](#autostarting--service-management) for details on managing the systemd service.

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

# Restart the service (needed after config changes if running as a service)
systemctl --user restart mprisence

# Enable the service to start on login
systemctl --user enable mprisence

# Disable the service from starting on login
systemctl --user disable mprisence

# View detailed logs
journalctl --user -u mprisence -f
```

## Configuration

The configuration file is located at:
- `~/.config/mprisence/config.toml` or
- `$XDG_CONFIG_HOME/mprisence/config.toml`

Changes are automatically detected and applied without requiring a restart.

For a complete configuration reference:
- See [`config.example.toml`](./config/config.example.toml) for detailed configuration options with explanations
- See [`config.default.toml`](./config/config.default.toml) for default configurations of popular media players
- See [`src/metadata.rs`](./src/metadata.rs) for all available template variables and their implementations
- See [`src/template.rs`](./src/template.rs) for template rendering system details

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

(See `src/metadata.rs` for the complete list)

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

### Player-Specific Configuration
```toml
# Use 'mprisence players' to get the correct player identity
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
# Run without system service
mprisence

# List available MPRIS players
mprisence players

# Show detailed player information including metadata and config
mprisence players --detailed

# Show current configuration
mprisence config

# Show version
mprisence version

# Enable more verbose logging
RUST_LOG=debug mprisence # or RUST_LOG=trace mprisence
```

## Troubleshooting

### Common Issues

1. **Discord Presence Not Showing**
   - Check if your media player is MPRIS-compatible (try running `mprisence players`)
   - Ensure the correct Discord App ID is configured
   - Verify Discord is running and detects external applications in its settings.

2. **Cover Art Not Displaying**
   - Check if the media file has embedded artwork or if metadata matches MusicBrainz.
   - If ImgBB is used
     - Check if the media file has embedded artwork or if the folder of the media file has an image file matching `cover.file_names` in `config.toml`.
     - Check if the `api_key` in `[cover.provider.imgbb]` is valid.

3. **Service Issues**
   ```bash
   # Check service status
   systemctl --user status mprisence
   
   # View detailed logs
   journalctl --user -u mprisence
   
   # Restart service (e.g., after config changes)
   systemctl --user restart mprisence
   ```

4. **Configuration Issues**
   - Validate your TOML syntax (use an online validator if unsure).
   - Check logs for parsing errors (`journalctl --user -u mprisence` or run `RUST_LOG=debug mprisence` directly).
   - Try with the default configuration first (remove or rename your config file).
   - **Incorrect Player Identity**: Ensure the `[player.<identity>]` section in your config uses the exact identity shown by `mprisence players`. Player names are normalized (lowercase, spaces replaced with underscores). For example, "VLC media player" becomes `vlc_media_player`.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
