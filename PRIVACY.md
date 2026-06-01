# Privacy Policy — mprisence Bridge Extension

**Last updated:** 2026-06-01

## Overview

The mprisence Bridge browser extension ("Extension") sends media playback metadata from supported websites (YouTube Music, YouTube, SoundCloud, Bandcamp, Tidal, Apple Music) to the mprisence native messaging host, which forwards it to the mprisence daemon for Discord Rich Presence display.

## What data is accessed

When you play media on a supported website, the Extension reads the following from the page:

| Data | Purpose |
|------|---------|
| **Website URL** | Identify the media source and match it to a Discord application |
| **Media metadata** (title, artist, album) | Display "now playing" information in Discord |
| **Playback state** (playing/paused, position, duration) | Show current playback status and progress |
| **Album art URL** | Fetch and display album cover art in Discord |

## How data is used

All data is transmitted exclusively to the **mprisence native messaging host** — a local binary running on your computer. The data is then forwarded to the **mprisence daemon**, also running locally, which publishes it to **Discord** via the Discord Rich Presence API.

**Data flow:** Browser Extension → Native Messaging Host (local) → mprisence Daemon (local) → Discord API

## What data is NOT accessed

- No personal information (name, email, address, etc.)
- No authentication credentials or cookies
- No browsing history beyond the supported media websites
- No keystrokes, form data, or page content
- No telemetry, analytics, or usage statistics

## Data storage

Data is processed in real-time and is **not persistently stored** by the Extension or the native messaging host. Cached album art images are stored locally on your machine under `~/.cache/mprisence/cover_art/` and may be retained for performance.

## Data sharing

- **No third-party sharing.** Data is transmitted only to Discord via their Rich Presence API for the sole purpose of displaying your current media activity.
- **No data selling.** Your data is never sold, rented, or traded.
- **No advertising.** Your data is never used for advertising or marketing purposes.

## Security

- Communication between the Extension and the native messaging host uses Chrome/Firefox's built-in **native messaging** protocol, which is restricted to the extension ID registered in the native messaging manifest.
- Communication with Discord uses **HTTPS** encryption.
- No data is transmitted to any server operated by the Extension developer.

## Third-party services

The Extension communicates with:
- **Discord** (discord.com) — to display your media activity via Rich Presence
- **YouTube/YouTube Music** (i.ytimg.com) — to fetch album art thumbnails
- **Other media websites** (as listed in the manifest) — to read playback metadata from the DOM

All communication with these services is subject to their respective privacy policies.

## Changes to this policy

If this privacy policy changes, the "Last updated" date will be updated. Material changes will be communicated via the extension's update notes.

## Contact

For questions about this privacy policy or the Extension's data practices, open an issue at:
[https://github.com/lazykern/mprisence/issues](https://github.com/lazykern/mprisence/issues)
