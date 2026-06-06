# Privacy Policy — mprisence Bridge Extension

**Last updated:** 2026-06-01

## Overview

mprisence Bridge is a browser extension that reads media playback metadata from supported websites (YouTube Music, YouTube, SoundCloud, Bandcamp, Tidal, Apple Music) and forwards it via native messaging to a local binary (`mprisence`) running on your computer.

The extension does **not** communicate with Discord, the internet, or any remote server on its own. Its sole purpose is feeding web media data into the local MPRIS desktop standard so media keys and desktop integrations can work.

## What data is accessed

When you play media on a supported website, the Extension reads the following from the page DOM:

| Data | Purpose |
|------|---------|
| **Website URL** | Identify the media source |
| **Media metadata** (title, artist, album) | Provide track info to MPRIS |
| **Playback state** (playing/paused, position, duration) | Provide playback status to MPRIS |
| **Album art URL** | Pass to MPRIS so desktop clients can display cover art |

## How data is used

All data is sent via **native messaging** to the `mprisence` binary on your machine. That binary publishes MPRIS players on D-Bus so desktop applications (media keys, Discord via mprisence daemon, etc.) can see your web media.

```
 ┌─────────────────────────────────────────────┐
 │  Browser Extension                          │
 │  (reads DOM, sends JSON via native msg)     │
 └──────────┬──────────────────────────────────┘
            │  stdin/stdout (local)
            ▼
 ┌──────────────────────┐        ┌──────────────────────────┐
 │ mprisence (web host mode) │───────▶│  D-Bus MPRIS             │
 │ (local binary)       │pub.    │  org.mpris.MediaPlayer2. │
 │                      │MPRIS   │  mprisence_web.*         │
 └──────────────────────┘        └──────────────────────────┘
                                         │
                                         ▼
                               Desktop integrations
                               (media keys, mprisence daemon,
                                Discord RP, etc.)
```

The extension itself transmits nothing to the network. All communication stays on your machine.

### Example — actual JSON message the extension sends

When you play music on YouTube Music, the extension sends a message like this to the bridge binary:

```json
{
  "type": "update",
  "source_id": "firefox:tab:42:0",
  "url": "https://music.youtube.com/watch?v=ABC123",
  "origin": "https://music.youtube.com",
  "site": "youtube_music",
  "playback": {
    "status": "playing",
    "position_ms": 45000,
    "duration_ms": 240000
  },
  "metadata": {
    "title": "Song Title",
    "artist": ["Artist Name"],
    "album": "Album Name",
    "album_artist": ["Artist Name"],
    "art_url": "https://i.ytimg.com/vi/ABC123/hqdefault.jpg"
  },
  "capabilities": {
    "play_pause": true,
    "next": true,
    "previous": true,
    "seek": true,
    "set_position": true
  }
}
```

This is the **only** data the extension ever transmits. No cookies, no history, no personal identifiers beyond a random tab ID.

## What data is NOT accessed

- No personal information (name, email, address, etc.)
- No authentication credentials or cookies
- No browsing history beyond the supported media websites
- No keystrokes, form data, or page content
- No telemetry, analytics, or usage statistics

## Data storage

Data is processed in real-time and is not persistently stored by the Extension or the native messaging host.

## Data sharing

- **No third-party sharing.** The extension sends data only to a local binary on your computer.
- **No data selling.** Your data is never sold, rented, or traded.
- **No advertising.** Your data is never used for advertising or marketing purposes.

## Security

- Communication between the Extension and the native messaging host uses the browser's built-in **native messaging** protocol, restricted to the registered extension ID.
- No data is transmitted over the network by the extension.

## Third-party services

The Extension reads data from the DOM of supported websites (as listed in the manifest). It does not communicate with any third-party API or server.

Downstream tools (e.g., the mprisence daemon, Discord) are separate software and subject to their own privacy policies.

## Changes to this policy

If this privacy policy changes, the "Last updated" date will be updated. Material changes will be communicated via the extension's update notes.

## Contact

For questions about this privacy policy or the Extension's data practices, open an issue at:
[https://github.com/lazykern/mprisence/issues](https://github.com/lazykern/mprisence/issues)
