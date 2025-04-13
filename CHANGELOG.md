# Changelog

All notable changes to mprisence will be documented in this file.

## [1.0.5](https://github.com/lazykern/mprisence/compare/v1.0.4...v1.0.5) (YYYY-MM-DD)

* Create `PlayerFinder` within the update loop instead of storing it in app state.

## [1.0.4](https://github.com/lazykern/mprisence/compare/v1.0.3...v1.0.4) (2025-04-13)

* Fix potential panic when fetching player status due to D-Bus errors (Fixes #41)
* Refine position jump detection logic to reduce false positives caused by D-Bus latency
* Set D-Bus timeout explicitly when iterating players to 5 seconds

## [1.0.3](https://github.com/lazykern/mprisence/compare/v1.0.2...v1.0.3) (2025-03-28)

* Show player icon as large image if cover art is not available

## [1.0.2](https://github.com/lazykern/mprisence/compare/v1.0.1...v1.0.2) (2025-03-28)

* Show normalized player identity in CLI

## [1.0.1](https://github.com/lazykern/mprisence/compare/v1.0.0...v1.0.1) (2025-03-27)

* Fixed issue with stale player configuration state after config reload

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
* **Configuration**
  - Live config reloading while running
  - New `[activity_type]` section with:
    - Configurable default activity type (listening/watching/playing/competing)
    - Content type detection from media URLs

* **Cover Art**
  - Disk caching with configurable TTL (24 hours default)
  - Enhanced ImgBB provider with expiration control

* **Template**
  - New template variables:
    - Audio details: `bitrate_display`, `sample_rate_display`
    - Extended metadata: `composer`, `lyricist`, `genre_display`
    - Formatted values: `track_display`, `duration_display`
    - Separated `status_icon` variable
  - Full template variable reference available in [`src/metadata.rs`](./src/metadata.rs)
  - Template rendering implementation details in [`src/template.rs`](./src/template.rs)

* **CLI**
  - New subcommands structure:
    - `mprisence players list [--detailed]`
    - `mprisence config`
    - `mprisence version`

### Other Changes
* Enhanced Discord connection reliability with auto-reconnection
* Improved support for all Discord activity types
* Better player name normalization

## [0.5.2](https://github.com/lazykern/mprisence/compare/v0.5.1...v0.5.2)

For changes before v1.0.0, please refer to the [releases page](https://github.com/lazykern/mprisence/releases).
