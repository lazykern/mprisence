# Changelog

All notable changes to mprisence will be documented in this file.

## [1.2.7](https://github.com/lazykern/mprisence/compare/v1.2.6...v1.2.7)

* Template
  * No escape for handlebars

## [1.2.6](https://github.com/lazykern/mprisence/compare/v1.2.5...v1.2.6)

* Config
  * Added kew to default config

## [1.2.5](https://github.com/lazykern/mprisence/compare/v1.2.4...v1.2.5)

* Cover Art
  * Fixed album cover path that is derived from artUrl (`file://`) not getting decoded #51

## [1.2.4](https://github.com/lazykern/mprisence/compare/v1.2.3...v1.2.4)

* Config
  * Added Supersonic and Feishin to default config

## [1.2.3](https://github.com/lazykern/mprisence/compare/v1.2.2...v1.2.3)

* Dependencies
  * Upgraded `notify` from 8.0.0 to 8.1.0
  * Upgraded `toml` from 0.8.20 to 0.9
  * Upgraded `blake3` from 1.8.1 to 1.8.2

## [1.2.2](https://github.com/lazykern/mprisence/compare/v1.2.1...v1.2.2)

* Config
  * Added fooyin music player to default config

## [1.2.1](https://github.com/lazykern/mprisence/compare/v1.2.0...v1.2.1)

* Cover Art
  * Use album artists for MusicBrainz release group search


## [1.2.0](https://github.com/lazykern/mprisence/compare/v1.1.2...v1.2.0)

* Config
  * Added `cover.provider.musicbrainz.min_score` to set the minimum score for MusicBrainz cover art
  * Added `cover.local_search_depth` to set the depth of local cover art search
* Cover Art
  * Increased MusicBrainz query duration range from 3 seconds to 5 seconds
  * Added album name to MusicBrainz track search query
  * Escape Lucene special characters in MusicBrainz search queries

## [1.1.2](https://github.com/lazykern/mprisence/compare/v1.1.1...v1.1.2)

* Fix [#41](https://github.com/lazykern/mprisence/issues/41) 
* Check discord status with a IPC socket connection attempt instead of `SingletonLock`

## [1.1.1](https://github.com/lazykern/mprisence/compare/v1.1.0...v1.1.1)

* Fixed issue when hot reloading config only updates when the config file is changed for the first time

## [1.1.0](https://github.com/lazykern/mprisence/compare/v1.0.6...v1.1.0)

* Use MusicBrainz ID from tags to fetch cover art if available
* Increased base MusicBrainz query score from 90% to 95%

## [1.0.6](https://github.com/lazykern/mprisence/compare/v1.0.5...v1.0.6)

* Prevent program crash when updating presence fails
* Set D-Bus timeout before fetching players to 5 seconds

## [1.0.5](https://github.com/lazykern/mprisence/compare/v1.0.4...v1.0.5)

* Create `PlayerFinder` within the update loop instead of storing it in app state.

## [1.0.4](https://github.com/lazykern/mprisence/compare/v1.0.3...v1.0.4)

* Fix potential panic when fetching player status due to D-Bus errors
* Refine position jump detection logic to reduce false positives caused by D-Bus latency
* Set D-Bus timeout explicitly when iterating players to 5 seconds

## [1.0.3](https://github.com/lazykern/mprisence/compare/v1.0.2...v1.0.3)

* Show player icon as large image if cover art is not available

## [1.0.2](https://github.com/lazykern/mprisence/compare/v1.0.1...v1.0.2)

* Show normalized player identity in CLI

## [1.0.1](https://github.com/lazykern/mprisence/compare/v1.0.0...v1.0.1)

* Fixed issue with stale player configuration state after config reload

## [1.0.0](https://github.com/lazykern/mprisence/compare/v0.5.2...v1.0.0)

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
