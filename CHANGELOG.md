# Changelog

All notable changes to mprisence will be documented in this file.

## [1.0.0-beta1] - 2025-03

### Added
- Hot reload configuration support
- New activity type system:
  - Content type detection (audio/video)
  - Configurable activity types: "listening", "watching", "playing", "competing"
  - Per-player activity type override
- Support for Mozilla Zen browser
- Enhanced CLI features:
  - `config` command to display current configuration
  - `players --detailed` flag to show player metadata and config
  - `version` command to show version info

### Changed
- Configuration structure improvements:
  - Renamed config files to `config.default.toml` and `config.example.toml`
  - Changed default player config from section to inline table
  - Default player now enabled by default
- Template variable names updated:
  - `artists` -> `artist_display`
  - `album_artists` -> `album_artist_display`
  - `album_name` -> `album`
- Simplified default state template (removed status icon)
- Check discord status by its SingletonLock symlink instead of trying to connect via ipc
- CLI improvements:
  - Removed explicit `start` command (now default behavior)
  - Enhanced player listing with status and metadata
  - Better organized command structure
- Updated dependencies to latest versions

### Fixed
- Faster startup time and improved performance
- Various bug fixes and performance improvements

## [0.5.2] - 2025-03-20

For changes before v1.0.0-beta1, please refer to the [releases page](https://github.com/lazykern/mprisence/releases). 