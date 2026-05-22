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
    // Upgrade thumbnails to higher resolution.
    if (artUrl) {
      if (artUrl.includes("yt3.googleusercontent.com")) {
        // Channel avatar: strip size params to get default 512x512.
        // Use =s800-c-k-no for 800x800 if needed.
        artUrl = artUrl.replace(/=[a-z0-9-]+$/, "");
      } else {
        // Video thumbnail: upgrade to maxresdefault (1280x720, 16:9)
        // — no YouTube black bar at bottom.
        artUrl = artUrl.replace(/\/[a-z]+default\./g, "/maxresdefault.");
      }
    }

    // ── Video ID from thumbnail URL ────────────────────────────
    const thumbSrc = artImg?.src || "";
    const videoIdMatch = thumbSrc.match(this.videoIdRegex);
    const videoId = videoIdMatch?.[1] || "";
    const trackId = videoId ? `ytm:${videoId}` : undefined;

    // ── Playback state via video element ───────────────────────
    const currentSec = video?.currentTime || 0;
    const isPaused = video?.paused ?? true;

    // The progress-bar aria-valuemax holds the per-track duration.
    // The <video> element spans the entire autoplay queue, so its
    // .duration can be 10-30x longer. During initial load or track
    // transitions aria-valuemax may be momentarily unavailable (NaN/0),
    // causing a fallback to the queue-wide video.duration which yields
    // garbage MPRIS length. Skip this update when the fallback fires
    // on a suspiciously long value; the next poll (~1s) will get the
    // correct track duration from a stable progress bar.
    const progressBar = this.qs<HTMLElement>("#progress-bar");
    const progressMax = progressBar ? parseFloat(progressBar.getAttribute("aria-valuemax") ?? "") : NaN;
    const trackDurationSec = (isFinite(progressMax) && progressMax > 0)
      ? progressMax
      : undefined;
    const totalSec = trackDurationSec ?? (video?.duration || 0);

    // If aria-valuemax wasn't ready yet, the fallback to video.duration
    // may be the queue length, not the track. Reject durations that
    // exceed a generous per-track limit (600s = 10 min; YTM songs
    // rarely exceed even 300s). Next poll will have the correct value.
    if (!trackDurationSec && video && video.duration > 600) {
      return null;
    }

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
