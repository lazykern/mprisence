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

  matches(url: URL): boolean {
    return url.origin === this.origin;
  }

  extract(): ProviderResult | null {
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

    // ── Artist (first part of byline before "•") ───────────────
    const byline = artistEl?.textContent?.trim() || "";
    const artist = byline.split("•")[0]?.trim() || "";
    // Album is NOT in YTM byline — leave empty (bridge can infer)

    // ── Album art (HTTPS URL, no blob: issues) ─────────────────
    let artUrl = artImg?.src || undefined;
    // Skip 1×1 placeholder GIFs
    if (artUrl && artUrl.startsWith("data:")) artUrl = undefined;
    // Upgrade thumbnails — only for channel avatars (safe to strip size params).
    // Do NOT upgrade video thumbnails to maxresdefault: YouTube only serves
    // maxresdefault.jpg for 720p+ uploads; older/lower-res videos return 404.
    // YouTube's thumb HTML already provides the correct size.
    if (artUrl) {
      if (artUrl.includes("yt3.googleusercontent.com")) {
        // Channel avatar: strip size params to get default 512x512.
        // Use =s800-c-k-no for 800x800 if needed.
        artUrl = artUrl.replace(/=[a-z0-9-]+$/, "");
      }
    }

    // ── Video ID from thumbnail URL ────────────────────────────
    const thumbSrc = artImg?.src || "";
    const videoIdMatch = thumbSrc.match(this.videoIdRegex);
    const videoId = videoIdMatch?.[1] || "";
    const trackId = videoId ? `ytm:${videoId}` : undefined;

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

    const currentSec = trackPositionSec ?? (video?.currentTime || 0);
    const totalSec = trackDurationSec ?? (video?.duration || 0);

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
      album: undefined, // YTM byline has no album info
      album_artist: [],
      art_url: artUrl,
      track_id: trackId,
    };

    const playback: PlaybackState = {
      status,
      position_ms: Math.floor(currentSec * 1000),
      duration_ms: Math.floor(totalSec * 1000),
      rate: video?.playbackRate ?? 1.0,
    };

    const capabilities: Capabilities = {
      play_pause: true,
      next: true,
      previous: true,
      seek: true,
      set_position: true,
      raise: true,
    };

    return {
      metadata,
      playback,
      capabilities,
      confidence: "provider",
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


}
