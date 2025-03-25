# mprisence - discord rich presence for mpris media players


[![AUR version](https://img.shields.io/aur/version/mprisence)](https://aur.archlinux.org/packages/mprisence)
[![GitHub license](https://img.shields.io/github/license/lazykern/mprisence)](https://github.com/lazykern/mprisence/blob/main/LICENSE)
[![GitHub stars](https://img.shields.io/github/stars/lazykern/mprisence)](https://github.com/lazykern/mprisence/stargazers)

## Preconfigured Players

Preconfigured in [`config.default.toml`](./config/config.default.toml):
- **Media Players**: VLC, MPV, Audacious, Elisa, Lollypop, Rhythmbox, CMUS, MPD, Musikcube, Clementine, Strawberry, Amberol, SMPlayer
- **Streaming**: YouTube Music, Spotify (disabled by default)
- **Browsers** (disabled by default): Firefox, Chrome, Edge, Brave

## Quick Installation

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

The configuration file is located at `~/.config/mprisence/config.toml` or `$XDG_CONFIG_HOME/mprisence/config.toml`. Changes are automatically detected and applied without requiring a restart.

If there are any parsing errors in your configuration, mprisence will:
1. Keep running with the last valid configuration
2. Log the error details (viewable with `journalctl`)
3. Continue watching for new changes

See [`config.example.toml`](./config/config.example.toml) for all available options.

### Cover Art Setup
```toml
# Configure ImgBB integration (optional)
[cover.provider.imgbb]
api_key = "<YOUR_IMGBB_API_KEY>"
provider = ["musicbrainz", "imgbb"]  # Provider order
```

## CLI Commands

mprisence provides several command-line options:

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

If you encounter issues:

1. **Check Service Status**:
   ```bash
   systemctl --user status mprisence
   ```

2. **View System Logs**:
   ```bash
   journalctl --user -u mprisence
   ```

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
