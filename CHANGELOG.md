# Changelog

All notable changes to mprisence will be documented in this file.

## [1.0.0] - 2025-03-27

### Architecture
- Modular code architecture with improved component isolation
- Thread-safe configuration using RwLock and Arc
- Added proper error handling with thiserror
- Improved logging system with different log levels

### Configuration System
- Added live config reloading (edit config.toml while running)
- Configuration structure improvements:
  - New `[activity_type]` section:
    - `default = "listening"` - choose between listening/watching/playing/competing
    - `use_content_type = true` - detects content type from media URLs
  - Expanded `[cover]` section with provider priorities
  - Added `[cover.provider.imgbb]` settings with expiration control
  - Player configs now support `override_activity_type` option
  - Better player name handling (case insensitive, spaces → underscores)

### Cover Art System
- Completely rewritten cover art handling:
  - Disk caching with TTL (24 hours by default)
  - Multiple cover art providers with fallback chain
  - MusicBrainz provider for online cover lookups
  - Enhanced ImgBB provider with customizable settings
  - Support for embedded cover art in audio files

### Template System
- Updated to Handlebars 6.x with better error handling
- Many new template variables:
  - Audio info: `bitrate_display`, `sample_rate_display`, etc.
  - Extended metadata: `composer`, `lyricist`, `genre_display`, etc.
  - Formatted values: `track_display ("1/12")`, `duration_display ("3:45")`
  - Status variables: `status_icon` (separate from status text)

### CLI Interface
- Better command-line interface with subcommands:
  - `mprisence` - run normally (default)
  - `mprisence players list` - show available players
  - `mprisence players list --detailed` - show player details
  - `mprisence config` - show current configuration
  - `mprisence version` - display version info

### Discord Integration
- More reliable Discord connection with auto-reconnection
- Support for all Discord activity types:
  - Listening (default for audio)
  - Watching (auto-detected for video)
  - Playing and Competing (manually configurable)
- Smarter presence updates that reduce Discord API calls

### Performance
- Reduced CPU usage with debounced updates
- Smart caching for expensive operations
- Memory usage optimizations
- More efficient change detection for player state

### Dependencies
- Updated core dependencies to latest versions
- Added tokio async runtime for better concurrency
- Using reqwest 0.12 for modern HTTP operations

## Config Migration Notes

### Key Changes

If upgrading from 0.5.x, the main changes to make in your config:

1. Add activity type settings (if desired):
   ```toml
   [activity_type]
   use_content_type = true   # Auto-detect from media URLs
   default = "listening"     # Default activity type
   ```

2. Status icons: Previously used in templates like `{{{status_icon}}} {{{artists}}}`, now you can access `status_icon` separately in templates.

3. Player-specific activity types (optional):
   ```toml
   [player.vlc]
   # ... other settings ...
   override_activity_type = "watching"  # Force this player to always use "Watching"
   ```

4. Cover art caching:
   ```toml
   [cover.provider.imgbb]
   expiration = 86400  # Cache time in seconds (24 hours)
   ```

## [0.5.2]

For changes before v1.0.0, please refer to the [releases page](https://github.com/lazykern/mprisence/releases).
