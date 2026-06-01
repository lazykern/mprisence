import type {
  Capabilities,
  MediaMetadata,
  PlaybackState,
} from "../types";
import type { Provider, ProviderResult } from "./base";

/**
 * YouTube provider (regular youtube.com, not music.youtube.com).
 *
 * Watches the main video player (#movie_player video) — works on
 * /watch pages and with the persistent mini player (homepage etc).
 * Ignores preview popups in recommendation thumbnails.
 *
 * On /watch pages: extracts title/channel from DOM.
 * On other pages (mini player): uses MediaSession API.
 *
 * Art URL comes from MediaSession API (YouTube sets it everywhere).
 */
export class YouTubeProvider implements Provider {
  readonly siteKey = "youtube";
  private readonly origin = "https://www.youtube.com";
  private readonly videoIdRe = /\/vi\/([a-zA-Z0-9_-]+)\//;

  matches(url: URL): boolean {
    // Match all youtube.com pages (mini player can appear anywhere)
    return url.origin === this.origin;
  }

  extract(): ProviderResult | null {
    // Skip extraction during YouTube ads — the DOM still shows the real
    // video title/channel but MediaSession is overridden with ad metadata
    // and the <video> element reflects ad position/duration.
    if (document.querySelector('.ad-showing')) return null;

    const mainPlayer = document.querySelector("#movie_player");
    const video = mainPlayer?.querySelector<HTMLVideoElement>("video");

    if (!video || !mainPlayer) return null;

    // Skip if duration is invalid (not loaded yet)
    const dur = video.duration;
    if (!dur || !isFinite(dur)) return null;

    const ct = video.currentTime;
    const isPaused = video.paused;
    const isWatchPage = location.pathname === "/watch";

    // ── Extract from MediaSession (available everywhere) ────────
    let msTitle: string | undefined;
    let msArtist: string | undefined;
    let msArtwork: string | undefined;
    let videoId: string | undefined;

    if ("mediaSession" in navigator) {
      const ms = (navigator as any).mediaSession;
      const md = ms?.metadata;
      if (md) {
        if (md.title) msTitle = md.title;
        if (md.artist) msArtist = md.artist;
        if (md.artwork?.length > 0) {
          const best = md.artwork.reduce(
            (a: any, b: any) => {
              const aSize = parseInt(a.sizes) || 0;
              const bSize = parseInt(b.sizes) || 0;
              return aSize > bSize ? a : b;
            }
          );
          msArtwork = best.src || undefined;
          // Extract videoId from artwork URL
          const m = (msArtwork || "").match(this.videoIdRe);
          if (m) videoId = m[1];
        }
      }
    }

    // Fallback: extract videoId from page URL params when artwork not loaded yet
    if (!videoId && isWatchPage) {
      const urlParams = new URLSearchParams(window.location.search);
      videoId = urlParams.get("v") || undefined;
    }

    // ── Title ───────────────────────────────────────────────────
    let title: string | undefined;
    if (isWatchPage) {
      const titleEl = document.querySelector(
        "#title h1.ytd-watch-metadata, h1.title.ytd-video-primary-info-renderer"
      );
      title = titleEl?.textContent?.trim() || undefined;
    }
    // Fall back to MediaSession title
    if (!title && msTitle) title = msTitle;
    // Last resort: document title
    if (!title) {
      const cleaned = document.title.replace(" - YouTube", "").trim();
      if (cleaned) title = cleaned;
    }

    // ── Channel / artist ────────────────────────────────────────
    let channelName: string | undefined;
    if (isWatchPage) {
      const channelEl = document.querySelector(
        "#owner #channel-name #text-container, #owner yt-formatted-string.ytd-channel-name"
      );
      channelName = (channelEl?.textContent?.trim() || "")
        .replace(/\s*-\s*Topic$/, "") || undefined;
    }
    // Fall back to MediaSession artist
    if (!channelName && msArtist) {
      channelName = msArtist.replace(/\s*-\s*Topic$/, "") || undefined;
    }

    // ── Album art ───────────────────────────────────────────────
    let artUrl = msArtwork;
    // Fallback: construct from video ID
    // Use hqdefault (always exists) — maxresdefault 404s for <720p uploads.
    if (!artUrl) {
      const urlParams = new URLSearchParams(window.location.search);
      const vid = urlParams.get("v") || videoId;
      if (vid) {
        artUrl = `https://i.ytimg.com/vi/${vid}/hqdefault.jpg`;
      }
    }

    // Channel avatar: strip size params for 512x512 default.
    // Do NOT upgrade ytimg thumbnails to maxresdefault — YouTube's
    // MediaSession already provides the best available size, and
    // maxresdefault.jpg 404s for many uploads.
    if (artUrl) {
      if (artUrl.includes("yt3.googleusercontent.com")) {
        artUrl = artUrl.replace(/=[a-z0-9-]+$/, "");
      }
    }

    const status = isPaused ? "paused" : "playing";

    // Construct proper watch URL if we have a videoId
    let watchUrl: string | undefined;
    if (videoId) {
      watchUrl = `https://www.youtube.com/watch?v=${videoId}`;
    }

    const metadata: MediaMetadata = {
      title,
      artist: channelName ? [channelName] : [],
      album: undefined,
      album_artist: [],
      art_url: artUrl,
      track_id: videoId ? `yt:${videoId}` : undefined,
    };

    const playback: PlaybackState = {
      status,
      position_ms: Math.floor(ct * 1000),
      duration_ms: Math.floor(dur * 1000),
    };

    const capabilities: Capabilities = {
      play_pause: true,
      next: false,
      previous: false,
      seek: true,
      set_position: true,
    };

    return {
      metadata,
      playback,
      capabilities,
      pageUrl: watchUrl || undefined,
      canonicalUrl: watchUrl || undefined,
    };
  }

  async command(cmd: string, positionMs?: number): Promise<void> {
    if (cmd === "play_pause" || cmd === "play" || cmd === "pause") {
      const video = document.querySelector<HTMLVideoElement>("#movie_player video");
      if (cmd === "play" && !video?.paused) return;
      if (cmd === "pause" && video?.paused) return;

      const btn = document.querySelector<HTMLElement>(
        ".ytp-play-button"
      );
      btn?.click();
      return;
    }

    if (cmd === "set_position") {
      const video = document.querySelector<HTMLVideoElement>("#movie_player video");
      if (video && typeof positionMs === "number" && isFinite(positionMs)) {
        video.currentTime = Math.max(0, positionMs / 1000);
      }
      return;
    }

    // YouTube doesn't have prev/next for regular videos beyond
    // playlist navigation — not implemented in MVP.
  }
}
