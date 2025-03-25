# MPRISence

⚠️ **This is a work in progress. Things might break!** ⚠️

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

### Service Management

The service is managed through systemd user services:

```bash
# Start the service
systemctl --user start mprisence

# Stop the service
systemctl --user stop mprisence

# Enable service to start on boot
systemctl --user enable mprisence

# Disable service from starting on boot
systemctl --user disable mprisence

# Restart after config changes
systemctl --user restart mprisence

# Check service status
systemctl --user status mprisence

# View logs
journalctl --user -u mprisence
```

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
