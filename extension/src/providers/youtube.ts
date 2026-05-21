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
  private readonly origin = "https://www.youtube.com";
  private readonly videoIdRe = /\/vi\/([a-zA-Z0-9_-]+)\//;

  matches(url: URL): boolean {
    // Match all youtube.com pages (mini player can appear anywhere)
    return url.origin === this.origin;
  }

  extract(): ProviderResult | null {
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
    if (!artUrl) {
      const urlParams = new URLSearchParams(window.location.search);
      const vid = urlParams.get("v") || videoId;
      if (vid) {
        artUrl = `https://i.ytimg.com/vi/${vid}/maxresdefault.jpg`;
      }
    }

    // Upgrade thumbnail resolution
    if (artUrl) {
      if (artUrl.includes("yt3.googleusercontent.com")) {
        artUrl = artUrl.replace(/=[a-z0-9-]+$/, "");
      } else if (artUrl.includes("ytimg.com")) {
        artUrl = artUrl.replace(/\/[a-z]+default\./g, "/maxresdefault.");
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
    };

    const playback: PlaybackState = {
      status,
      position_ms: Math.floor(ct * 1000),
      duration_ms: Math.floor(dur * 1000),
      rate: video.playbackRate || 1.0,
    };

    const capabilities: Capabilities = {
      play_pause: true,
      next: false,
      previous: false,
      seek: true,
      set_position: true,
      raise: true,
    };

    return {
      metadata,
      playback,
      capabilities,
      confidence: "provider",
      pageUrl: watchUrl || undefined,
    };
  }

  async command(cmd: string): Promise<void> {
    if (cmd === "play_pause") {
      const btn = document.querySelector<HTMLElement>(
        ".ytp-play-button"
      );
      btn?.click();
      return;
    }

    // YouTube doesn't have prev/next for regular videos beyond
    // playlist navigation — not implemented in MVP.
  }
}
