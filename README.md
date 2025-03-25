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
- Highly configurable (see [example config](./config/example.toml))

## Quick Start

Choose the installation method for your system:

### On Arch Linux (Recommended)
```bash
# Install from AUR
yay -S mprisence

# Enable auto-start
systemctl --user enable --now mprisence.service
```

### On Other Linux Systems (Using Cargo)
```bash
# Install using Rust's package manager
cargo install --git "https://github.com/lazykern/mprisence.git"

# Set up auto-start
mkdir -p "$HOME/.config/systemd/user"
curl https://raw.githubusercontent.com/lazykern/mprisence/main/mprisence.service >"$HOME/.config/systemd/user/mprisence.service"
# Update the service file to use the cargo bin path
sed -i 's|ExecStart=/usr/bin/mprisence|ExecStart='"$HOME"'/.cargo/bin/mprisence|' "$HOME/.config/systemd/user/mprisence.service"
systemctl --user daemon-reload
systemctl --user enable --now mprisence.service
```

> **Note**: When installing via cargo, make sure `~/.cargo/bin` is in your `$PATH`. If it isn't, add `export PATH="$HOME/.cargo/bin:$PATH"` to your shell's config file.

## Basic Usage

Just run:
```bash
mprisence
```

## Settings

You can customize how MPRISence works by editing its settings file. Put your settings in:
- `~/.config/mprisence/config.toml` or
- `$XDG_CONFIG_HOME/mprisence/config.toml`

Need help with settings?
- Check [example settings](./config/example.toml) for all options
- See [default settings](./config/default.toml) for what's pre-configured

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

> **Note**: After changing settings, restart MPRISence:
> ```bash
> systemctl --user restart mprisence.service
> ```
