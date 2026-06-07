import type {
  Capabilities,
  MediaMetadata,
  PlaybackState,
  ExtMessage,
} from "../types";
import type { Provider, ProviderResult } from "./base";

/**
 * YouTube Music provider.
 *
 * Real DOM structure (verified via zenctl on live YTM):
 *   titleEl:  .title.ytmusic-player-bar
 *   artistEl: .byline.ytmusic-player-bar  → "Artist • ## views • ## likes"
 *   artImg:   .ytmusic-player-bar img.image
 *   prevBtn:  yt-icon-button.previous-button
 *   nextBtn:  yt-icon-button.next-button
 *   playBtn:  #play-pause-button  → title="Play"|"Pause"
 *   video:    video  (blob URL, has currentTime/duration)
 *
 * Key findings:
 *   - No MediaSession API — must use DOM scraping
 *   - Byline has NO album — only "Artist • views • likes"
 *   - Album art is HTTPS (i.ytimg.com) — no blob: issue
 *   - Upgrade to /maxresdefault/ for 1280x720 clean art (no black bar)
 *   - videoId in thumbnail URL, not always in ?v= param
 *   - No <audio> — YTM uses <video>
 */
export class YouTubeMusicProvider implements Provider {
  readonly siteKey = "youtube_music";
  private readonly origin = "https://music.youtube.com";
  private readonly videoIdRegex = /\/vi\/([a-zA-Z0-9_-]+)\//;
  private stablePlayback: {
    trackId: string;
    positionSec: number;
    durationSec: number;
  } | null = null;

  matches(url: URL): boolean {
    return url.origin === this.origin;
  }

  extract(): ProviderResult | null {
    // Skip extraction during YouTube Music ads.
    if (document.querySelector('.ad-showing')) return null;

    const titleEl = this.qs<HTMLElement>(".title.ytmusic-player-bar");
    const artistEl = this.qs<HTMLElement>(".byline.ytmusic-player-bar");
    const artImg = this.qs<HTMLImageElement>(
      "ytmusic-player-bar img.image, ytmusic-player-bar img"
    );
    const playBtn = this.qs<HTMLElement>("#play-pause-button");
    const video = this.qs<HTMLVideoElement>("video");

    if (!titleEl && !video) return null;

    // ── Title ──────────────────────────────────────────────────
    const title =
      titleEl?.textContent?.trim() ||
      document.title.replace(" - YouTube Music", "").trim() ||
      undefined;

    // ── Artist & Album from byline ───────────────────────────
    // Format: "Artist • Album • Year" or "Artist • ## views • ## likes"
    const byline = artistEl?.textContent?.trim() || "";
    const parts = byline.split("•").map(s => s.trim()).filter(Boolean);
    const artist = parts[0] || "";
    // Album is the middle segment if it doesn't look like a view/like count
    let album: string | undefined = undefined;
    if (parts.length >= 3) {
      const mid = parts[1];
      if (mid && !/\b(view|like)s?\b/i.test(mid)) {
        album = mid;
      }
    }

    // ── Video ID from thumbnail URL or page URL ────────────────
    const thumbSrc = artImg?.src || "";
    let videoId = (thumbSrc.match(this.videoIdRegex) || [])[1] || "";
    // Fallback: extract videoId from page URL params.
    // YTM's <img> sometimes shows a channel avatar (yt3 URL) instead
    // of a video thumbnail — the regex won't match, so we need the
    // page URL as a fallback to construct proper cover art.
    if (!videoId) {
      videoId = new URLSearchParams(window.location.search).get("v") || "";
    }
    const trackId = videoId ? `ytm:${videoId}` : undefined;

    // ── Album art ──────────────────────────────────────────────
    let artUrl = artImg?.src || undefined;
    // Skip 1×1 placeholder GIFs
    if (artUrl && artUrl.startsWith("data:")) artUrl = undefined;

    if (artUrl) {
      if (artUrl.includes("yt3.googleusercontent.com")) {
        // Channel avatar — not the track's cover art.
        // Prefer video thumbnail constructed from video ID.
        // Only keep channel avatar if we have no video ID.
        if (videoId) {
          artUrl = `https://i.ytimg.com/vi/${videoId}/maxresdefault.jpg`;
        } else {
          // Strip size params to get default 512x512.
          artUrl = artUrl.replace(/=[a-z0-9-]+$/, "");
        }
      } else {
        // i.ytimg.com thumbnail — upgrade to maxresdefault.
        // hqdefault/sddefault are 4:3 and often include top/bottom
        // black bars; maxresdefault is 16:9 and clean when available.
        artUrl = artUrl.replace(/\/[a-z]+default\./g, "/maxresdefault.");
      }
    } else if (videoId) {
      // No img element src but we have a video ID — construct
      // thumbnail URL using maxresdefault to avoid black bars.
      artUrl = `https://i.ytimg.com/vi/${videoId}/maxresdefault.jpg`;
    }

    // ── Playback state ─────────────────────────────────────────
    const isPaused = video?.paused ?? true;

    // YTM <video> spans the entire queue: currentTime/duration can be
    // 30-60 minutes. Per-track position/duration live on the player-bar
    // progress element as aria-valuenow/aria-valuemax. If unavailable,
    // skip instead of publishing queue time as track time.
    const progressBar = this.qs<HTMLElement>("#progress-bar");
    const progressNow = progressBar ? parseFloat(progressBar.getAttribute("aria-valuenow") ?? "") : NaN;
    const progressMax = progressBar ? parseFloat(progressBar.getAttribute("aria-valuemax") ?? "") : NaN;
    const trackPositionSec = (isFinite(progressNow) && progressNow >= 0) ? progressNow : undefined;
    const trackDurationSec = (isFinite(progressMax) && progressMax > 0) ? progressMax : undefined;

    // Without both values, fallback would use queue-wide video time.
    if (video && (trackPositionSec === undefined || trackDurationSec === undefined) && video.duration > 600) {
      return null;
    }

    let currentSec = trackPositionSec ?? (video?.currentTime || 0);
    let totalSec = trackDurationSec ?? (video?.duration || 0);
    ({ positionSec: currentSec, durationSec: totalSec } = this.stabilizePlayback(
      trackId,
      currentSec,
      totalSec,
    ));

    // If video exists but duration is invalid (NaN/0/Infinity), skip -
    // metadata hasn't loaded yet. We'll retry on next poll.
    if (video && (totalSec === 0 || !isFinite(totalSec))) {
      return null;
    }

    // ── Status from play button title ──────────────────────────
    const isPlaying = playBtn
      ? playBtn.getAttribute("title")?.toLowerCase().includes("pause")
      : !isPaused;

    const status = isPlaying ? "playing" : "paused";

    const metadata: MediaMetadata = {
      title,
      artist: artist ? [artist] : [],
      album, // extracted from byline when present
      album_artist: [],
      art_url: artUrl,
      track_id: trackId,
    };

    const playback: PlaybackState = {
      status,
      position_ms: Math.floor(currentSec * 1000),
      duration_ms: Math.floor(totalSec * 1000),
    };

    const capabilities: Capabilities = {
      play_pause: true,
      next: true,
      previous: true,
      seek: true,
      set_position: true,
    };

    return {
      metadata,
      playback,
      capabilities,
      canonicalUrl: videoId ? `https://music.youtube.com/watch?v=${videoId}` : undefined,
    };
  }

  async command(cmd: string, positionMs?: number): Promise<void> {
    // Class-selector map (verified live — there are no #id selectors for prev/next)
    if (cmd === "set_position") {
      const video = this.qs<HTMLVideoElement>("video");
      if (video && typeof positionMs === "number" && isFinite(positionMs)) {
        video.currentTime = Math.max(0, positionMs / 1000);
      }
      return;
    }

    if (cmd === "play" || cmd === "pause") {
      const video = this.qs<HTMLVideoElement>("video");
      if (cmd === "play" && !video?.paused) return;
      if (cmd === "pause" && video?.paused) return;
    }

    const btnMap: Record<string, string> = {
      play_pause: "#play-pause-button",
      play: "#play-pause-button",
      pause: "#play-pause-button",
      next: "yt-icon-button.next-button button",
      previous: "yt-icon-button.previous-button button",
    };

    const selector = btnMap[cmd];
    if (selector) {
      const btn = document.querySelector<HTMLElement>(selector);
      btn?.click();
    }
  }

  private qs<T extends HTMLElement>(selector: string): T | null {
    return document.querySelector<T>(selector);
  }

  private stabilizePlayback(
    trackId: string | undefined,
    positionSec: number,
    durationSec: number,
  ): { positionSec: number; durationSec: number } {
    if (!trackId || durationSec <= 0 || !isFinite(durationSec)) {
      return { positionSec, durationSec };
    }

    const prev = this.stablePlayback;
    if (!prev || prev.trackId !== trackId) {
      this.stablePlayback = { trackId, positionSec, durationSec };
      return { positionSec, durationSec };
    }

    let pos = positionSec;
    let dur = durationSec;

    const durDiff = Math.abs(dur - prev.durationSec);
    if (prev.durationSec > 0 && durDiff > 10 && durDiff / prev.durationSec > 0.15) {
      dur = prev.durationSec;
    }

    if (prev.positionSec > 5 && pos + 3 < prev.positionSec) {
      pos = prev.positionSec;
    }
    if (prev.positionSec > 30 && pos === 0) {
      pos = prev.positionSec;
    }
    if (dur > 0 && pos > dur) {
      pos = Math.min(prev.positionSec, dur);
    }

    this.stablePlayback = { trackId, positionSec: pos, durationSec: dur };
    return { positionSec: pos, durationSec: dur };
  }
}
