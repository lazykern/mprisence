/**
 * Apple Music provider for mprisence.
 *
 * Apple Music web player (music.apple.com) is a React SPA using MusicKit JS.
 *
 * Key observations:
 *   - Reliable MediaSession API — always set on track/album pages
 *   - Uses <audio> element for streaming playback
 *   - Controls: aria-label buttons (Play/Pause/Next/Previous)
 *   - Progress: <audio>.currentTime / <audio>.duration (track-specific)
 *   - Artwork CDN: is1-ssl.mzstatic.com (upgrade from MediaSession's small src)
 *   - URL: music.apple.com/<storefront>/album/<slug>/<album-id>?i=<track-id>
 *
 * Unlike YTM, Apple Music sets MediaSession metadata consistently,
 * so we use MediaSession as the primary source with DOM fallback.
 */

import type {
  Capabilities,
  MediaMetadata,
  PlaybackState,
} from "../types";
import type { Provider, ProviderResult } from "./base";

export class AppleMusicProvider implements Provider {
  readonly siteKey = "apple_music";

  matches(url: URL): boolean {
    return url.hostname === "music.apple.com";
  }

  extract(): ProviderResult | null {
    const audio = document.querySelector<HTMLAudioElement>("audio");
    if (!audio) return null;

    // Duration must be valid
    if (!isFinite(audio.duration) || audio.duration <= 0) return null;

    const meta: MediaMetadata = {
      title: undefined,
      artist: [],
      album: undefined,
      album_artist: [],
      art_url: undefined,
      track_id: undefined,
    };

    // ── Primary: MediaSession API ──────────────────────────────
    let confidence: "provider" | "dom" = "dom";
    let pageUrl: string | undefined;

    try {
      const ms = (navigator as any).mediaSession;
      if (ms?.metadata) {
        const md = ms.metadata;
        const hasContent = !!(md.title || md.artist || md.album);
        if (hasContent) {
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
          confidence = "provider";
        }
      }
    } catch {
      // MediaSession not available
    }

    // ── Fallback: DOM extraction ───────────────────────────────
    if (!meta.title) {
      // Page title format: "Song Name — Artist — Apple Music"
      meta.title = document.title
        .replace(/ — Apple Music$/, "")
        .replace(/^(.+?) — (.+)$/, "$1")
        .trim() || undefined;
    }

    if (meta.artist.length === 0) {
      // Try extracting artist from page title
      const titleMatch = document.title.match(/^.+? — (.+?) — Apple Music$/);
      if (titleMatch) {
        meta.artist = [titleMatch[1]];
      }
    }

    // ── Track ID from URL ──────────────────────────────────────
    // URL: /<storefront>/album/<slug>/<album-id>?i=<track-id>
    const urlParams = new URLSearchParams(window.location.search);
    const trackId = urlParams.get("i");
    if (trackId) {
      meta.track_id = `am:${trackId}`;
      pageUrl = window.location.href.split("?")[0] + `?i=${trackId}`;
    }

    if (!meta.title) return null;

    // ── Playback state ─────────────────────────────────────────
    const isPaused = audio.paused;
    const status: PlaybackState["status"] = isPaused ? "paused" : "playing";

    const playback: PlaybackState = {
      status,
      position_ms: Math.floor((audio.currentTime || 0) * 1000),
      duration_ms: Math.floor(audio.duration * 1000),
      rate: audio.playbackRate ?? 1.0,
    };

    // ── Capabilities ───────────────────────────────────────────
    const playBtn = document.querySelector<HTMLElement>(
      'button[aria-label="Play"], button[data-testid="play"]'
    );
    const nextBtn = document.querySelector<HTMLElement>(
      'button[aria-label="Next"], button[data-testid="next"]'
    );
    const prevBtn = document.querySelector<HTMLElement>(
      'button[aria-label="Previous"], button[data-testid="previous"]'
    );

    const capabilities: Capabilities = {
      play_pause: !!playBtn || !!audio,
      next: nextBtn ? !nextBtn.disabled : false,
      previous: prevBtn ? !prevBtn.disabled : false,
      seek: true,
      set_position: true,
      raise: true,
    };

    return {
      metadata: meta,
      playback,
      capabilities,
      confidence,
      pageUrl,
    };
  }

  async command(cmd: string, positionMs?: number): Promise<void> {
    switch (cmd) {
      case "play_pause":
      case "play":
      case "pause": {
        const audio = document.querySelector<HTMLAudioElement>("audio");
        if (!audio) break;

        const isPlaying = !audio.paused;
        if (cmd === "play" && isPlaying) break;
        if (cmd === "pause" && !isPlaying) break;

        if (isPlaying) {
          audio.pause();
          // Also try clicking the Play/Pause button for UI sync
          this.clickBtn('button[aria-label="Pause"]');
        } else {
          await audio.play().catch(() => {});
          this.clickBtn('button[aria-label="Play"]');
        }
        break;
      }
      case "next": {
        const btn = document.querySelector<HTMLElement>(
          'button[aria-label="Next"], button[data-testid="next"]'
        );
        if (btn && !btn.disabled) btn.click();
        break;
      }
      case "previous": {
        const btn = document.querySelector<HTMLElement>(
          'button[aria-label="Previous"], button[data-testid="previous"]'
        );
        if (btn && !btn.disabled) btn.click();
        break;
      }
      case "set_position":
      case "seek": {
        if (typeof positionMs === "number" && isFinite(positionMs)) {
          const audio = document.querySelector<HTMLAudioElement>("audio");
          if (audio) {
            audio.currentTime = Math.max(0, positionMs / 1000);
          }
        }
        break;
      }
    }
  }

  // ── Helpers ──────────────────────────────────────────────────

  private clickBtn(selector: string): void {
    const btn = document.querySelector<HTMLElement>(selector);
    if (btn && !btn.disabled) btn.click();
  }

  /**
   * Upgrade Apple Music artwork to higher resolution.
   *
   * Apple CDN URL pattern:
   *   https://is{1-5}-ssl.mzstatic.com/image/thumb/Music{id}/{uuid}/{name}.{w}x{h}bb.{ext}
   *
   * MediaSession typically provides small sizes (50x50, 100x100, 200x200).
   * Upgrade to 600x600bb for good quality without being too large.
   */
  private resolveArtwork(url: string | undefined): string | undefined {
    if (!url) return undefined;
    if (url.includes("mzstatic.com")) {
      // Replace {w}x{h}bb with 600x600bb for high-res square art
      url = url.replace(/\d+x\d+bb(?=\.[a-z]+)/i, "600x600bb");
    }
    return url;
  }
}
