# mprisence 

> A powerful Discord Rich Presence for MPRIS media players on Linux

[![AUR version](https://img.shields.io/aur/version/mprisence)](https://aur.archlinux.org/packages/mprisence)
[![GitHub license](https://img.shields.io/github/license/lazykern/mprisence)](https://github.com/lazykern/mprisence/blob/main/LICENSE)
[![GitHub stars](https://img.shields.io/github/stars/lazykern/mprisence)](https://github.com/lazykern/mprisence/stargazers)

A highly configurable service that shows your currently playing media on Discord. Works with VLC, MPV, Spotify, and any other Linux media player that supports MPRIS. Shows album art, track info, and playback status with extensive customization options.

## Features

- **Universal Media Player Support**: Works with any MPRIS-compatible media player
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
provider = ["musicbrainz", "imgbb"]

[cover.provider.imgbb]
# Your ImgBB API key (get one at: https://api.imgbb.com/)
api_key = "YOUR_API_KEY_HERE"
# How long to keep uploaded images (in seconds, default: 1 day)
expiration = 86400
```

### Player-Specific Configuration
```toml
# Use 'mprisence players' to get the correct player name
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

# Enable debug logging
RUST_LOG=debug mprisence
```

## Troubleshooting

### Common Issues

1. **Discord Presence Not Showing**
   - Check if your media player is MPRIS-compatible (try running `mprisence players`)
   - Ensure the correct Discord App ID is configured

2. **Cover Art Not Displaying**
   - Check if the media file has embedded artwork
   - Verify ImgBB API key if using ImgBB provider

3. **Service Issues**
   ```bash
   # Check service status
   systemctl --user status mprisence
   
   # View detailed logs
   journalctl --user -u mprisence
   
   # Restart service
   systemctl --user restart mprisence
   ```

4. **Configuration Issues**
   - Validate your TOML syntax
   - Check logs for parsing errors
   - Try with the default configuration first

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
