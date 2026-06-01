import type {
  Capabilities,
  MediaMetadata,
  PlaybackState,
} from "../types";

export interface ProviderResult {
  metadata: MediaMetadata;
  playback: PlaybackState;
  capabilities: Capabilities;
  /** Override the page URL sent in ExtMessage (e.g. mini player watch URL) */
  pageUrl?: string;
  /** Canonical track/page URL when distinct from the visible page URL. */
  canonicalUrl?: string;
}

/**
 * Base media provider interface.
 * Each provider knows how to extract metadata from a specific site.
 */
export interface Provider {
  /** Check if this provider handles the given URL */
  matches(url: URL): boolean;

  /** Extract metadata and playback state from the page */
  extract(): ProviderResult | null;

  /** Stable site key used for bridge grouping/config, e.g. `youtube_music`. */
  readonly siteKey?: string;

  /** Execute a media control command. `positionMs` is absolute for set_position. */
  command(cmd: string, positionMs?: number): Promise<void>;
}

/**
 * Fallback provider for generic audio/video elements.
 */
export class GenericMediaProvider implements Provider {
  matches(_url: URL): boolean {
    // Generic provider matches any page with media elements
    return true;
  }

  extract(): ProviderResult | null {
    const video = document.querySelector("video");
    const audio = document.querySelector("audio");
    const media = video ?? audio;

    if (!media) return null;

    // Skip if media exists but duration is invalid (not loaded yet)
    const dur = media.duration;
    if (!dur || !isFinite(dur)) return null;

    const meta: MediaMetadata = {
      title: document.title || undefined,
      artist: [],
    };

    const playback: PlaybackState = {
      status: media.paused
        ? "paused"
        : media.ended
          ? "stopped"
          : "playing",
      position_ms: Math.floor(media.currentTime * 1000),
      duration_ms: Math.floor(dur * 1000),
    };

    const caps: Capabilities = {
      play_pause: true,
      next: false,
      previous: false,
      seek: true,
      set_position: true,
    };

    // Try Media Session API for richer metadata
    if ("mediaSession" in navigator) {
      const ms = (navigator as any).mediaSession;
      if (ms?.metadata) {
        const md = ms.metadata;
        if (md.title) meta.title = md.title;
        if (md.artist) meta.artist = [md.artist];
        if (md.album) meta.album = md.album;
        if (md.artwork?.length > 0) {
          // Pick the largest artwork
          const best = md.artwork.reduce(
            (a: any, b: any) =>
              (a.sizes ?? 0) > (b.sizes ?? 0) ? a : b
          );
          meta.art_url = best.src || undefined;
        }
      }
    }

    return {
      metadata: meta,
      playback,
      capabilities: caps,
    };
  }

  async command(cmd: string, positionMs?: number): Promise<void> {
    const video = document.querySelector<HTMLVideoElement>("video");
    const audio = document.querySelector<HTMLAudioElement>("audio");
    const media = video ?? audio;
    if (!media) return;

    switch (cmd) {
      case "play_pause":
        if (media.paused) await media.play().catch(() => undefined);
        else media.pause();
        break;
      case "play":
        if (media.paused) await media.play().catch(() => undefined);
        break;
      case "pause":
        if (!media.paused) media.pause();
        break;
      case "set_position":
        if (typeof positionMs === "number" && isFinite(positionMs)) {
          media.currentTime = Math.max(0, positionMs / 1000);
        }
        break;
      case "seek":
        // Bridge protocol currently has no signed relative offset; ignore.
        break;
    }
  }
}
