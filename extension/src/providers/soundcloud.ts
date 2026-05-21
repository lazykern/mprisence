/**
 * SoundCloud provider for mprisence.
 *
 * Extracts metadata from:
 *   - MediaSession API (primary — always set on track pages)
 *   - DOM selectors (fallback for playback state, controls)
 *
 * SoundCloud uses Web Audio API — NO `<audio>`/`<video>` element.
 * Metadata comes from MediaSession API. Playback state from button UI.
 * Controls via DOM clicks on play/pause/next/prev buttons.
 *
 * SoundCloud web player structure (soundcloud.com):
 *   - Metadata: `navigator.mediaSession.metadata` set on EVERY track page
 *   - Controls: `.playControls__play` (bottom bar, appears after first play)
 *   - Track page: `.soundTitle` area with `.sc-button-play` button
 *   - Artwork: `.soundTitle__artwork img` or `soundTitleArt__artwork img`
 *   - No audio/video DOM elements — Web Audio API only
 */

import type {
  Capabilities,
  ConfidenceLevel,
  MediaMetadata,
  PlaybackState,
} from "../types";
import type { Provider, ProviderResult } from "./base";

export class SoundCloudProvider implements Provider {
  matches(url: URL): boolean {
    return url.hostname === "soundcloud.com" || url.hostname.endsWith(".soundcloud.com");
  }

  extract(): ProviderResult | null {
    const meta: MediaMetadata = {
      title: undefined,
      artist: [],
      album: undefined,
      album_artist: [],
      art_url: undefined,
      track_id: undefined,
    };

    let playback: PlaybackState = {
      status: "stopped",
      position_ms: 0,
      duration_ms: 0,
      rate: 1.0,
    };

    let confidence: ConfidenceLevel = "dom";
    let pageUrl: string | undefined;

    // ── Primary: MediaSession API ──────────────────────────────
    // SoundCloud sets MediaSession on every track page, even before playing.
    let hasMs = false;
    try {
      const ms = (navigator as any).mediaSession;
      if (ms?.metadata) {
        const md = ms.metadata;
        const hasContent = !!(md.title || md.artist || md.album);
        if (hasContent) {
          if (md.title) meta.title = md.title;
          if (md.artist) meta.artist = [md.artist];
          if (md.album) meta.album = md.album;
          if (md.artwork?.length > 0) {
            const best = md.artwork.reduce((a: any, b: any) => {
              const aSize = parseInt(a.sizes) || 0;
              const bSize = parseInt(b.sizes) || 0;
              return aSize > bSize ? a : b;
            });
            meta.art_url = resolveArtwork(best.src || undefined);
          }
          confidence = "provider";
          hasMs = true;
        }
      }
    } catch {
      /* MediaSession not available */
    }

    // ── Fallback: DOM selectors ────────────────────────────────
    if (!meta.title) {
      // Track page: `.soundTitle__title` or `.soundTitle__title > span`
      const titleEl =
        document.querySelector<HTMLElement>(".soundTitle__title") ??
        document.querySelector<HTMLElement>(".soundTitle__title > span");
      if (titleEl) {
        meta.title = titleEl.textContent?.trim() || undefined;
      }
    }

    if (meta.artist.length === 0) {
      // Track page: `.soundTitle__username` or similar
      const artistEl =
        document.querySelector<HTMLAnchorElement>(".soundTitle__username");
      if (artistEl) {
        meta.artist = [artistEl.textContent?.trim() || ""].filter(Boolean);
      }
    }

    if (!meta.art_url) {
      // Track page artwork
      const artImg =
        document.querySelector<HTMLImageElement>(".soundTitle__artwork img, .soundTitleArt__artwork img");
      if (artImg?.src) {
        meta.art_url = resolveArtwork(artImg.src);
      }
    }

    // ── Playback state ─────────────────────────────────────────
    // SoundCloud uses Web Audio — no `<audio>`/`<video>` DOM element.
    // Determine state from play button appearance.
    const playBtn = document.querySelector(".sc-button-play");
    const isPlaying = playBtn?.classList.contains("playing") ||
      playBtn?.getAttribute("title") === "Pause" ||
      document.querySelector(".playControls__play.playing") !== null;

    // Parse duration from DOM timeline:
    //   "Duration: 8 minutes 11 seconds" or aria-hidden "8:11"
    let durationMs = 0;
    const durHidden = document.querySelector<HTMLElement>(
      ".playbackTimeline__duration .sc-visuallyhidden"
    );
    if (durHidden?.textContent) {
      durationMs = parseSoundCloudDuration(durHidden.textContent);
    }
    if (durationMs === 0) {
      // Fallback: parse "8:11" format
      const durSpan = document.querySelector<HTMLElement>(
        ".playbackTimeline__duration span[aria-hidden=true]"
      );
      if (durSpan?.textContent) {
        durationMs = parseMmSsDuration(durSpan.textContent);
      }
    }

    // Estimate position from progress bar width percentage
    let positionMs = 0;
    if (durationMs > 0) {
      const progressBar = document.querySelector<HTMLElement>(
        ".playbackTimeline__progressBar"
      );
      if (progressBar) {
        const style = progressBar.getAttribute("style") || "";
        const m = style.match(/width\s*:\s*([\d.]+)%/);
        if (m) {
          positionMs = Math.floor((parseFloat(m[1]) / 100) * durationMs);
        }
      }
    }

    playback = {
      status: isPlaying ? "playing" : hasMs ? "paused" : "stopped",
      position_ms: positionMs,
      duration_ms: durationMs,
      rate: 1.0,
    };

    // ── Capabilities check ─────────────────────────────────────
    const hasNext =
      !!document.querySelector(".playControls__next") ||
      !!document.querySelector(".skipButton__forward");
    const hasPrev =
      !!document.querySelector(".playControls__prev") ||
      !!document.querySelector(".skipButton__backward");
    const hasPlayBtn =
      !!document.querySelector(".sc-button-play") ||
      !!document.querySelector(".playControls__play");

    const capabilities: Capabilities = {
      play_pause: hasPlayBtn,
      next: hasNext,
      previous: hasPrev,
      seek: false,    // no audio element for seeking
      set_position: false,
      raise: true,
    };

    // Must have at least a title to be useful
    if (!meta.title && !hasMs) return null;

    return {
      metadata: meta,
      playback,
      capabilities,
      confidence,
      pageUrl,
    };
  }

  async command(cmd: string): Promise<void> {
    switch (cmd) {
      case "play_pause": {
        // Try bottom player bar first, then track page button
        const btn =
          document.querySelector<HTMLElement>(".playControls__play") ??
          document.querySelector<HTMLElement>(".sc-button-play");
        btn?.click();
        break;
      }
      case "next": {
        const btn =
          document.querySelector<HTMLElement>(".playControls__next") ??
          document.querySelector<HTMLElement>(".skipButton__forward");
        btn?.click();
        break;
      }
      case "previous": {
        const btn =
          document.querySelector<HTMLElement>(".playControls__prev") ??
          document.querySelector<HTMLElement>(".skipButton__backward");
        btn?.click();
        break;
      }
      case "seek":
      case "set_position":
        // SoundCloud uses Web Audio — no programmatic seek via DOM
        break;
    }
  }
}

/**
 * Resolve artwork URL to highest available resolution.
 *
 * SoundCloud CDN patterns:
 *   - `-t500x500.jpg` / `-t200x200.jpg` / `-t67x67.jpg`
 *   - Replace size suffix with `-original` for largest
 */
function resolveArtwork(url: string | undefined): string | undefined {
  if (!url) return undefined;

  // Upgrade to largest standard square size if on sndcdn.com
  if (url.includes("sndcdn.com") || url.includes("soundcloud")) {
    // Replace any size suffix (t500x500, t300x300, etc., or original)
    // with t500x500 — the largest standard square size for album art
    url = url.replace(/-t\d+x\d+(?=\.[a-z]+)/i, "-t500x500");
    url = url.replace(/-original(?=\.[a-z]+)/i, "-t500x500");
    url = url.replace(/-crop-[a-z]+(?=\.[a-z]+)/i, "");
  }

  return url;
}

/**
 * Parse SoundCloud duration text like "Duration: 8 minutes 11 seconds".
 */
function parseSoundCloudDuration(text: string): number {
  const m = text.match(/(\d+)\s*minutes?\s*(\d+)?\s*seconds?/i);
  if (m) {
    const mins = parseInt(m[1]) || 0;
    const secs = parseInt(m[2]) || 0;
    return (mins * 60 + secs) * 1000;
  }
  // Try "Duration: 1 minute"
  const m2 = text.match(/(\d+)\s*minute/i);
  if (m2) {
    return parseInt(m2[1]) * 60 * 1000;
  }
  // Try "Duration: 30 seconds"
  const m3 = text.match(/(\d+)\s*second/i);
  if (m3) {
    return parseInt(m3[1]) * 1000;
  }
  return 0;
}

/**
 * Parse "mm:ss" or "h:mm:ss" format.
 */
function parseMmSsDuration(text: string): number {
  text = text.trim();
  const parts = text.split(":");
  if (parts.length === 2) {
    // mm:ss
    const mins = parseInt(parts[0]) || 0;
    const secs = parseInt(parts[1]) || 0;
    return (mins * 60 + secs) * 1000;
  }
  if (parts.length === 3) {
    // h:mm:ss
    const hours = parseInt(parts[0]) || 0;
    const mins = parseInt(parts[1]) || 0;
    const secs = parseInt(parts[2]) || 0;
    return (hours * 3600 + mins * 60 + secs) * 1000;
  }
  return 0;
}
