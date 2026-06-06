/**
 * Tidal provider for mprisence.
 *
 * Tidal uses a React SPA with scrambled CSS module class names.
 * Detection relies on MediaSession API, aria-labels, and data-test attributes.
 *
 * Player structure:
 *   - Metadata: navigator.mediaSession.metadata (set on every track)
 *   - Playback: <video> element (Tidal uses video even for audio tracks)
 *   - Controls: buttons selected by aria-label or data-test attributes
 *     Play:   button[data-test="play"]   /  button[aria-label="Play"]
 *     Pause:  button[data-test="pause"]  /  button[aria-label="Pause"]
 *     Next:   button[aria-label="Next"]
 *     Prev:   button[aria-label="Previous"]
 *   - Progress: [role="slider"][aria-label="Progress bar"]
 *     aria-valuenow / aria-valuemax (seconds)
 *   - Artwork CDN: resources.tidal.com/images/<uuid>/<size>.jpg
 *     Upgrade 80x80 / 320x320 → 640x640 or 1280x1280
 */

import type {
  Capabilities,
  MediaMetadata,
  PlaybackState,
} from "../types";
import type { Provider, ProviderResult } from "./base";

export class TidalProvider implements Provider {
  readonly siteKey = "tidal";

  matches(url: URL): boolean {
    return (
      url.hostname === "tidal.com" ||
      url.hostname.endsWith(".tidal.com")
    );
  }

  extract(): ProviderResult | null {
    const video = document.querySelector<HTMLVideoElement>("video");
    if (!video) return null;

    // Duration must be valid
    if (!isFinite(video.duration) || video.duration <= 0) return null;

    const meta: MediaMetadata = {
      title: undefined,
      artist: [],
      album: undefined,
      album_artist: [],
      art_url: undefined,
      track_id: undefined,
    };

    // ── Primary: MediaSession API ──────────────────────────────
    try {
      const ms = (navigator as any).mediaSession;
      if (ms?.metadata) {
        const md = ms.metadata;
        if (md.title) meta.title = md.title;
        if (md.artist) meta.artist = [md.artist];
        if (md.album) meta.album = md.album;

        // Artwork: pick largest, then upgrade resolution
        if (md.artwork?.length > 0) {
          const best = md.artwork.reduce((a: any, b: any) => {
            const aSize = parseInt(a.sizes) || 0;
            const bSize = parseInt(b.sizes) || 0;
            return aSize > bSize ? a : b;
          });
          meta.art_url = this.resolveArtwork(best.src || undefined);
        }
      }
    } catch {
      // MediaSession not available
    }

    // Fallback: DOM extraction if MediaSession failed
    if (!meta.title) {
      // Could extract from page title or DOM selectors if available
      meta.title = document.title.replace(/ \| TIDAL$/, "").trim() || undefined;
    }

    if (!meta.title) return null;

    // ── Playback state ─────────────────────────────────────────
    const isPaused = video.paused;
    const status: PlaybackState["status"] = isPaused ? "paused" : "playing";

    // Position: prefer progress slider aria-value (more accurate per-track)
    // Fallback to video.currentTime
    let positionMs = Math.floor((video.currentTime || 0) * 1000);
    let durationMs = Math.floor(video.duration * 1000);

    const progressSlider = document.querySelector<HTMLElement>(
      '[role="slider"][aria-label="Progress bar"]'
    );
    if (progressSlider) {
      const now = parseFloat(progressSlider.getAttribute("aria-valuenow") ?? "");
      const max = parseFloat(progressSlider.getAttribute("aria-valuemax") ?? "");
      if (isFinite(now) && isFinite(max) && max > 0) {
        positionMs = Math.floor(now * 1000);
        durationMs = Math.floor(max * 1000);
      }
    }

    const playback: PlaybackState = {
      status,
      position_ms: positionMs,
      duration_ms: durationMs,
    };

    // ── Capabilities ───────────────────────────────────────────
    const nextBtn = document.querySelector<HTMLElement>(
      'button[aria-label="Next"]'
    );
    const prevBtn = document.querySelector<HTMLElement>(
      'button[aria-label="Previous"]'
    );

    const capabilities: Capabilities = {
      play_pause: true,
      next: nextBtn ? !nextBtn.disabled : false,
      previous: prevBtn ? !prevBtn.disabled : false,
      seek: !!progressSlider,
      set_position: !!progressSlider,
    };

    return {
      metadata: meta,
      playback,
      capabilities,
    };
  }

  async command(cmd: string, positionMs?: number): Promise<void> {
    switch (cmd) {
      case "play_pause":
      case "play":
      case "pause": {
        const video = document.querySelector<HTMLVideoElement>("video");
        if (!video) break;
        const isPlaying = !video.paused;
        if (cmd === "play" && isPlaying) break;
        if (cmd === "pause" && !isPlaying) break;
        if (isPlaying) { video.pause(); } else { video.play().catch(() => {}); }
        break;
      }
      case "next": {
        const btn = document.querySelector<HTMLElement>(
          'button[aria-label="Next"]'
        );
        if (btn && !btn.disabled) btn.click();
        break;
      }
      case "previous": {
        const btn = document.querySelector<HTMLElement>(
          'button[aria-label="Previous"]'
        );
        if (btn && !btn.disabled) btn.click();
        break;
      }
      case "set_position":
      case "seek": {
        if (typeof positionMs === "number" && isFinite(positionMs)) {
          const video = document.querySelector<HTMLVideoElement>("video");
          if (video) {
            video.currentTime = Math.max(0, positionMs / 1000);
          }
        }
        break;
      }
    }
  }

  // ── Helpers ──────────────────────────────────────────────────

  /**
   * Upgrade Tidal CDN artwork to higher resolution.
   * Pattern: resources.tidal.com/images/<uuid>/<size>.jpg
   * Available sizes: 80x80, 320x320, 640x640, 1080x1080, 1280x1280
   */
  private resolveArtwork(url: string | undefined): string | undefined {
    if (!url) return undefined;
    if (url.includes("resources.tidal.com")) {
      url = url.replace(/\/\d+x\d+\./g, "/1280x1280.");
    }
    return url;
  }
}
