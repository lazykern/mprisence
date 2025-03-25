# mprisence - discord rich presence for mpris media players

A feature-rich Rust application to show what you're playing on Linux in your Discord status. Works with any MPRIS-compatible media player.

[![AUR version](https://img.shields.io/aur/version/mprisence)](https://aur.archlinux.org/packages/mprisence)
[![GitHub license](https://img.shields.io/github/license/lazykern/mprisence)](https://github.com/lazykern/mprisence/blob/main/LICENSE)
[![GitHub stars](https://img.shields.io/github/stars/lazykern/mprisence)](https://github.com/lazykern/mprisence/stargazers)

Display your media activity from VLC, Spotify, Firefox, or any MPRIS-compatible player in Discord with rich details and album artwork.

## Preconfigured Players

### Media Players
- VLC Media Player
- MPV
- Audacious
- Elisa
- Lollypop
- Rhythmbox
- CMUS
- MPD (Music Player Daemon)
- Musikcube
- Clementine
- Strawberry
- Amberol
- SMPlayer

### Streaming Services
- YouTube Music
- Spotify (disabled by default)

### Browsers (disabled by default)
- Mozilla Firefox
- Google Chrome
- Microsoft Edge
- Brave

## Key Features

- **Universal Compatibility**: Works with all MPRIS-compatible media players on Linux
- **Rich Media Display**: Shows detailed information including:
  - Song/video title and artist
  - Album artwork (via MusicBrainz or local files)
  - Playback progress
  - Media quality (bitrate, sample rate)
- **Real-time Updates**: Instantly reflects your current media status
- **Multi-player Support**: Handles multiple media players simultaneously
- **Highly Configurable**: Extensive customization options for display format and behavior
- **Album Artwork Integration**: Supports MusicBrainz and ImgBB for cover art hosting
- **Performance**: Written in Rust for efficiency and reliability

## Quick Installation

### Arch Linux (Recommended)
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

Configuration files are located at:
- Primary: `~/.config/mprisence/config.toml`
- Alternative: `$XDG_CONFIG_HOME/mprisence/config.toml`

mprisence uses a three-tier configuration system:
1. [`config.default.toml`](./config/config.default.toml) - System defaults
2. [`config.example.toml`](./config/config.example.toml) - Documented example with all options
3. `config.toml` - Your personal configuration (auto-created from example)

### Album Artwork Setup

Enable album artwork display with MusicBrainz and ImgBB:
```toml
# Configure ImgBB integration
[cover.provider.imgbb]
api_key = "<YOUR_IMGBB_API_KEY>"

# Optional: Customize provider order
[cover.provider]
provider = ["musicbrainz", "imgbb"]
```

### Version Migration Guide

#### Upgrading from v0.5.2

Key configuration changes in v1.0.0-beta1:

1. **Template System Updates**:
   - Simplified state format
   - New metadata variables
   - Renamed template variables for clarity

2. **Enhanced Player Settings**:
   - Inline table syntax for defaults
   - New streaming media support
   - Per-player activity type override

3. **Activity Type Configuration**:
   - Content-based type detection
   - Multiple activity types support
   - Customizable default activities

4. **Cover Art Improvements**:
   - Configurable image expiration
   - Flexible file name patterns
   - Enhanced provider options

See the [example configuration](./config/config.example.toml) for complete documentation.

## Manual Operation

Run mprisence without system service:
```bash
mprisence
```

## Troubleshooting

If you encounter issues:

1. **Check Service Status**:
   ```bash
   systemctl --user status mprisence
   ```

2. **View System Logs**:
   ```bash
   journalctl --user -u mprisence
   ```

3. **Verify Configuration**:
   ```bash
   # Compare with example config
   diff ~/.config/mprisence/config.toml ~/.config/mprisence/config.example.toml
   ```

## Contributing

Contributions are welcome! Feel free to:
- Report bugs
- Suggest features
- Submit pull requests

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
