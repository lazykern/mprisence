# Privacy Policy — mprisence bridge

Last updated: 2026-08-01

## Summary

mprisence bridge does not collect, transmit, store, or sell any personal data.
No data ever leaves your computer.

## What the extension reads

On sites where it is active, the extension reads media playback information from
the page:

- track title, artist, album
- cover art URL
- playback position, duration, playback state (playing/paused)
- the page URL and site favicon, used to label the player

## Where that data goes

It is sent over Chrome native messaging to `mprisence`, a program running on the
same machine, which publishes it on the local D-Bus session as an MPRIS media
player. No network requests are made by the extension. There is no server, no
analytics, no telemetry, no advertising, and no third-party code.

If you configure `mprisence` itself to show Discord Rich Presence, `mprisence`
(not the extension) sends the track metadata to Discord. That is a separate,
user-configured feature of the desktop program.

## Permissions

- `nativeMessaging` — required to talk to the local `mprisence` host.
- `storage` — stores one local setting (whether the generic fallback is on).
- `scripting` — registers the extension's own bundled content scripts when the
  generic fallback is enabled.
- Site host permissions — read media metadata on supported music sites.
- `<all_urls>` (optional, off by default) — only requested if you explicitly
  enable the generic fallback on the options page, so the extension can read
  media metadata on other sites. Revoking the toggle drops the permission.

## Contact

https://github.com/lazykern/mprisence/issues
