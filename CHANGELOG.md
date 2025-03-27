# Changelog

All notable changes to mprisence will be documented in this file.

## [1.0.0](https://github.com/lazykern/mprisence/compare/v0.5.2...v1.0.0) (2025-03-27)

> Major release with comprehensive improvements to configuration, cover art handling, templating, and Discord integration

### Upgrade Steps
* **Configuration File Updates Required:**
  1. Update template variables usage:
     - Replace `artists` with `artist_display` for formatted output
     - Use new `track_display` instead of manual `track_number`/`track_total`
     - New audio info variables: `bitrate_display`, `sample_rate_display`

### Breaking Changes
* Template system updated to Handlebars 6.x - verify your custom templates
* Status icon handling changed in templates - now accessed via separate `status_icon` variable
* Configuration structure changes require updates to existing config files

### New Features
* **Configuration System**
  - Live config reloading while running
  - New `[activity_type]` section with:
    - Configurable default activity type (listening/watching/playing/competing)
    - Content type detection from media URLs
  - Player-specific activity type overrides
  - Case-insensitive player name handling

* **Cover Art System**
  - Disk caching with configurable TTL (24 hours default)
  - Multiple provider fallback chain
  - MusicBrainz integration for online lookups
  - Enhanced ImgBB provider with expiration control
  - Support for embedded audio file cover art

* **Template System**
  - New template variables:
    - Audio details: `bitrate_display`, `sample_rate_display`
    - Extended metadata: `composer`, `lyricist`, `genre_display`
    - Formatted values: `track_display`, `duration_display`
    - Separated `status_icon` variable
  - Full template variable reference available in [`src/metadata.rs`](./src/metadata.rs)
  - Template rendering implementation details in [`src/template.rs`](./src/template.rs)

* **CLI Interface**
  - New subcommands structure:
    - `mprisence players list [--detailed]`
    - `mprisence config`
    - `mprisence version`

### Performance Improvements
* Reduced CPU usage through debounced updates
* Implemented smart caching for expensive operations
* Memory usage optimizations
* More efficient player state change detection
* Smarter Discord presence updates to minimize API calls

### Other Changes
* Enhanced Discord connection reliability with auto-reconnection
* Improved support for all Discord activity types
* Better player name normalization

## [0.5.2](https://github.com/lazykern/mprisence/compare/v0.5.1...v0.5.2)

For changes before v1.0.0, please refer to the [releases page](https://github.com/lazykern/mprisence/releases).
