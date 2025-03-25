# MPRISence

Show what you're playing on Linux in your Discord status. Works with any MPRIS-compatible media player.

## Features

- Shows your media in Discord status (music, videos)
- Works with any MPRIS-compatible media player
- Shows album/song artwork (MusicBrainz, local files hosted on ImgBB)
- Displays rich media info (title, artist, album)
- Handles multiple players at once
- Updates in real-time
- Highly configurable (see [configuration](#configuration))

## Quick Start

Choose the installation method for your system:

### On Arch Linux (Recommended)
```bash
# Install from AUR
yay -S mprisence
```

### Manual Installation
```bash
# Clone the repository
git clone https://github.com/lazykern/mprisence.git
cd mprisence

# Build and install (this will also enable and start the service by default)
make

# To install without enabling the service
make install-local ENABLE_SERVICE=0

# To uninstall
make uninstall-local
```

## Configuration

The configuration file is located at:
- `~/.config/mprisence/config.toml` or
- `$XDG_CONFIG_HOME/mprisence/config.toml`

When you first install MPRISence, a default configuration file will be created based on the example configuration. You can customize this file to suit your needs.

MPRISence uses three configuration files:
- [`config.default.toml`](./config/config.default.toml) - Built-in defaults used as fallback
- [`config.example.toml`](./config/config.example.toml) - Complete example with all available options and documentation
- `config.toml` - Your active configuration file (created from example if it doesn't exist)

### Album Artwork

MPRISence uses MusicBrainz and ImgBB by default. To use ImgBB hosting:
```toml
# Add your ImgBB API key
[cover.provider.imgbb]
api_key = "<YOUR API KEY>"

# Default providers are already set to ["musicbrainz", "imgbb"]
# Only change this if you want to modify the order or disable a provider
[cover.provider]
provider = ["musicbrainz", "imgbb"]
```

### Migrating from v0.5.2

If you're upgrading from v0.5.2, there are several important changes to the configuration:

1. **Template Format Changes**:
   - State format simplified: from `'{{{status_icon}}} {{{artists}}} '` to `"{{{artist_display}}}"`
   - Large text now uses `album` instead of `album_name`
   - New variables available:
     - Audio properties: `bitrate_display`, `sample_rate_display`, `bit_depth_display`, `channels_display`
     - Track metadata: `track_display`, `disc_display`, `genre`, `year`, `initial_key`, `bpm`, `mood`
     - Player info: `player_bus_name`, `duration_secs`, `duration_display`
   - Renamed variables:
     - `artists` → `artist_display`
     - `album_artists` → `album_artist_display`
     - `album_name` → `album`

2. **Player Configuration**:
   - Default player settings now use inline table syntax
   - Changed default `ignore` from `true` to `false`
   - Added new options:
     - `override_activity_type` for per-player activity type
     - `allow_streaming` for http/https media support

3. **Activity Types** (New Feature):
   - Added `[activity_type]` section
   - `use_content_type = true` to determine activity type based on media
   - Supports: "listening", "watching", "playing", "competing"
   - Set `default = "listening"` for fallback

4. **Cover Art Changes**:
   - Added `expiration = 86400` (1 day) for ImgBB uploads
   - File names for local cover art now configurable globally

Please refer to the [example configuration](./config/config.example.toml) for the complete list of options and their documentation.

## Running Manually

If you prefer not to use the service, you can run MPRISence directly:

```bash
mprisence
```

## Troubleshooting

If you encounter issues:

1. Check the service status:
   ```bash
   systemctl --user status mprisence
   ```

2. View the logs:
   ```bash
   journalctl --user -u mprisence
   ```

3. Verify your configuration:
   ```bash
   # Compare your config with the example
   diff ~/.config/mprisence/config.toml ~/.config/mprisence/config.example.toml
   ```
