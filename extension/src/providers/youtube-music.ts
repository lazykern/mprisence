import type {
  Capabilities,
  MediaMetadata,
  PlaybackState,
  ExtMessage,
} from "../types";
import type { Provider, ProviderResult } from "./base";

type MediaPlaybackState = Pick<HTMLMediaElement, "paused" | "ended">;

export function isYouTubeMusicPlaying(
  media: MediaPlaybackState | null | undefined,
  playButtonTitle: string | null | undefined,
): boolean {
  if (media) return !media.paused && !media.ended;
  return playButtonTitle?.toLowerCase().includes("pause") ?? false;
}

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
 *   - No MediaSession API - must use DOM scraping
 *   - Byline has NO album - only "Artist • views • likes"
 *   - Album art is HTTPS (i.ytimg.com) - no blob: issue
 *   - Keep YTM's supplied thumbnail; maxresdefault is not universal
 *   - videoId in thumbnail URL, not always in ?v= param
 *   - No <audio> - YTM uses <video>
 */
export class YouTubeMusicProvider implements Provider {
  readonly siteKey = "youtube_music";
  private readonly origin = "https://music.youtube.com";
  private readonly videoIdRegex = /\/vi\/([a-zA-Z0-9_-]+)\//;
  private mismatchedTitle: { title: string; since: number } | null = null;

  matches(url: URL): boolean {
    return url.origin === this.origin;
  }

  extract(): ProviderResult | null {
    // Skip extraction during YouTube Music ads.
    if (document.querySelector('.ad-showing')) return null;

    const titleEl = this.qs<HTMLElement>(".title.ytmusic-player-bar");
    const artistEl = this.qs<HTMLElement>(".byline.ytmusic-player-bar");
    const artImg = this.playerArtImage();
    const playBtn = this.qs<HTMLElement>("#play-pause-button");
    const video = this.qs<HTMLVideoElement>("video");

    if (!titleEl && !video) return null;
    if (
      !video ||
      video.readyState < 2 ||
      !Number.isFinite(video.duration) ||
      video.duration <= 0
    ) {
      return null;
    }

    // ── Title ──────────────────────────────────────────────────
    // A bare "YouTube Music" document title means no track is shown yet.
    const docTitle = document.title?.endsWith(" - YouTube Music")
      ? document.title.slice(0, -" - YouTube Music".length).trim()
      : "";
    const title = titleEl?.textContent?.trim() || docTitle || undefined;

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
    if (!videoId) {
      const current = this.currentVideoId(title);
      // Page-world's ID still belongs to the previous track; wait for it.
      if (current === null) return null;
      videoId = current;
    }
    // Fallback: extract videoId from page URL params.
    // YTM's <img> sometimes shows a channel avatar (yt3 URL) instead
    // of a video thumbnail - the regex won't match, so we need the
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
        // Channel avatar - not the track's cover art.
        // Prefer a guaranteed video thumbnail over the channel avatar.
        // Only keep channel avatar if we have no video ID.
        if (videoId) {
          artUrl = `https://i.ytimg.com/vi/${videoId}/hqdefault.jpg`;
        } else {
          // Strip size params to get default 512x512.
          artUrl = artUrl.replace(/=[a-z0-9-]+$/, "");
        }
      } else {
        // Keep YouTube's supplied thumbnail. maxresdefault is absent for
        // many videos, which leaves MPRIS clients with a 404 artwork URL.
      }
    } else if (videoId) {
      // No img element src but we have a video ID - construct
      // thumbnail URL. hqdefault is available when maxresdefault is not.
      artUrl = `https://i.ytimg.com/vi/${videoId}/hqdefault.jpg`;
    }

    // ── Playback state ─────────────────────────────────────────
    // YTM <video> spans the entire queue: currentTime/duration can be
    // 30-60 minutes. Per-track position/duration live on the player-bar
    // progress element as aria-valuenow/aria-valuemax. If unavailable,
    // skip instead of publishing queue time as track time.
    const progressBar = this.visibleProgressBar();
    const progressNow = progressBar ? parseFloat(progressBar.getAttribute("aria-valuenow") ?? "") : NaN;
    const progressMax = progressBar ? parseFloat(progressBar.getAttribute("aria-valuemax") ?? "") : NaN;
    const trackPositionSec = (isFinite(progressNow) && progressNow >= 0) ? progressNow : undefined;
    const trackDurationSec = (isFinite(progressMax) && progressMax > 0) ? progressMax : undefined;

    // Without both values, fallback would use queue-wide video time.
    if (video && (trackPositionSec === undefined || trackDurationSec === undefined) && video.duration > 600) {
      return null;
    }

    const hasStartupProgressPlaceholder =
      trackPositionSec === 0 &&
      trackDurationSec === 100 &&
      Math.abs(video.duration - trackDurationSec) > 1;

    if (hasStartupProgressPlaceholder) {
      return null;
    }

    let currentSec = trackPositionSec ?? (video.currentTime || 0);
    let totalSec = trackDurationSec ?? video.duration;
    // If video exists but duration is invalid (NaN/0/Infinity), skip -
    // metadata hasn't loaded yet. We'll retry on next poll.
    if (video && (totalSec === 0 || !isFinite(totalSec))) {
      return null;
    }

    // ── Playback status ─────────────────────────────────────────
    const isPlaying = isYouTubeMusicPlaying(
      video,
      playBtn?.getAttribute("title"),
    );

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
    // Class-selector map (verified live - there are no #id selectors for prev/next)
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

  private playerArtImage(): HTMLImageElement | null {
    const images = Array.from(document.querySelectorAll<HTMLImageElement>("ytmusic-player-bar img"));
    return images.find((image) => this.videoIdRegex.test(image.src))
      ?? images[0]
      ?? this.qs<HTMLImageElement>("ytmusic-player-bar img.image, ytmusic-player-bar img");
  }

  /** Returns null while page-world's video ID lags a track change. */
  private currentVideoId(title: string | undefined): string | null {
    const root = document.documentElement;
    const fromPageWorld = root?.getAttribute("data-mprisence-ytm-video-id");
    if (fromPageWorld && /^[a-zA-Z0-9_-]+$/.test(fromPageWorld)) {
      const idTitle = root?.getAttribute("data-mprisence-ytm-video-title");
      if (!idTitle || !title || idTitle === title) {
        this.mismatchedTitle = null;
        return fromPageWorld;
      }
      // Don't stall forever if the player API and player bar never agree.
      if (this.mismatchedTitle?.title !== title) {
        this.mismatchedTitle = { title, since: Date.now() };
      }
      return Date.now() - this.mismatchedTitle.since > 3000 ? fromPageWorld : null;
    }

    const selectors = [
      "ytmusic-player-bar[video-id]",
      "ytmusic-player-queue-item[video-id][play-button-state='playing']",
      "ytmusic-player-queue-item[video-id][selected]",
      "ytmusic-player-queue-item[video-id][aria-selected='true']",
    ];
    for (const selector of selectors) {
      const videoId = this.qs<HTMLElement>(selector)?.getAttribute("video-id");
      if (videoId) return videoId;
    }
    return "";
  }

  private visibleProgressBar(): HTMLElement | null {
    const bars = Array.from(document.querySelectorAll<HTMLElement>("#progress-bar"));
    return bars.find((bar) => !bar.getClientRects || bar.getClientRects().length > 0) ?? bars[0] ?? null;
  }
}
