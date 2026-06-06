/**
 * Bandcamp provider for mprisence.
 *
 * Handles two player types:
 *
 * 1. Carousel player (collection pages: bandcamp.com/<username>)
 *    .carousel-player.show
 *      .now-playing         — artwork, album/collection title, artist
 *      .progress-transport  — controls, track title, position/duration
 *        .playpause          (click: playPauseClick)
 *          .play              (visible: showPlay)
 *          .pause             (visible: showPause) → visible = playing
 *        .info-progress .info .title span  — current TRACK title
 *        .pos-dur            — "00:53 / 02:28"
 *          span (positionStr)
 *          span (durationStr)
 *        .transport
 *          .prev > .prev-icon    (.disabled = no prev)
 *          .next > .next-icon    (.disabled = no next)
 *
 * 2. Inline player (album/track pages: <artist>.bandcamp.com/album/...)
 *    .inline_player
 *      .playbutton           — .playing class when playing
 *      .title                — current track title
 *      .time_elapsed / .time_total  — "mm:ss" format
 *      .progbar_fill         — width% progress
 *      .prevbutton / .nextbutton
 *
 * Page-level metadata (album pages):
 *   .trackTitle / h2.trackTitle  — album title
 *   #name-section h3 a            — artist name
 *   #tralbumArt img               — album artwork
 */

import type {
  Capabilities,
  MediaMetadata,
  PlaybackState,
} from "../types";
import type { Provider, ProviderResult } from "./base";

export class BandcampProvider implements Provider {
  readonly siteKey = "bandcamp";
  private readonly trackIdParam = /track_id=(\d+)/;

  matches(url: URL): boolean {
    const host = url.hostname;
    return host === "bandcamp.com" || host.endsWith(".bandcamp.com");
  }

  extract(): ProviderResult | null {
    // Try carousel player first (collection pages)
    const carouselResult = this.extractCarousel();
    if (carouselResult) return carouselResult;

    // Try inline player (album/track pages)
    const inlineResult = this.extractInline();
    if (inlineResult) return inlineResult;

    return null;
  }

  async command(cmd: string, positionMs?: number): Promise<void> {
    // Try carousel controls first
    if (document.querySelector(".carousel-player.show")) {
      await this.commandCarousel(cmd, positionMs);
      return;
    }

    // Try inline player controls
    if (document.querySelector(".inline_player")) {
      await this.commandInline(cmd, positionMs);
      return;
    }
  }

  // ── Carousel player (collection pages) ────────────────────────

  private extractCarousel(): ProviderResult | null {
    const player = this.qs<HTMLElement>(".carousel-player.show");
    if (!player) return null;

    // Track title: .info-progress .info .title → last span (trackTitle)
    // First span (if present) is trackNumber; we skip it.
    const trackTitleEl = this.qs<HTMLElement>(
      ".carousel-player .info-progress .info .title span:last-child"
    );
    const trackTitle = trackTitleEl?.textContent?.trim() || "";
    // Album/collection title: .now-playing .title
    const albumTitle = this.qsText(
      ".carousel-player .now-playing .title"
    );

    const title = trackTitle || albumTitle;
    if (!title) return null;

    const meta: MediaMetadata = {
      title,
      artist: [],
      album: undefined,
      album_artist: [],
      art_url: undefined,
      track_id: undefined,
    };

    // Album: only set when it differs from track title
    if (albumTitle && albumTitle !== title) {
      meta.album = albumTitle;
    }

    // Artist: .now-playing .artist (format: "by ArtistName")
    const artistRaw = this.qsText(
      ".carousel-player .now-playing .artist"
    );
    if (artistRaw) {
      const cleaned = artistRaw.replace(/^by\s+/i, "").trim();
      if (cleaned) meta.artist = [cleaned];
    }

    // Artwork: .now-playing img
    meta.art_url = this.resolveArtwork(
      this.qs<HTMLImageElement>(".carousel-player .now-playing img")?.src
    );

    // Track ID from audio src
    const audio = document.querySelector<HTMLAudioElement>("audio");
    const srcMatch = audio?.getAttribute("src")?.match(this.trackIdParam);
    if (srcMatch) meta.track_id = `bc:${srcMatch[1]}`;

    // Playing state: from audio element (native API)
    const isPlaying = audio ? !audio.paused : false;

    // Position/duration from .pos-dur spans (formatted time)
    let positionMs = 0;
    let durationMs = 0;
    const posDur = this.qs<HTMLElement>(".carousel-player .pos-dur");
    if (posDur) {
      const spans = posDur.querySelectorAll("span");
      if (spans.length >= 2) {
        positionMs = parseTimeSpan(spans[0]?.textContent);
        durationMs = parseTimeSpan(spans[1]?.textContent);
      }
    }
    // Fallback to audio element
    if (durationMs === 0 && audio && isFinite(audio.duration) && audio.duration > 0) {
      durationMs = Math.floor(audio.duration * 1000);
      positionMs = Math.floor((audio.currentTime || 0) * 1000);
    }
    if (durationMs === 0) return null;

    const playback: PlaybackState = {
      status: isPlaying ? "playing" : "paused",
      position_ms: positionMs,
      duration_ms: durationMs,
    };

    const prevIcon = this.qs<HTMLElement>(
      ".carousel-player .prev-icon"
    );
    const nextIcon = this.qs<HTMLElement>(
      ".carousel-player .next-icon"
    );
    const capabilities: Capabilities = {
      play_pause: true,
      next: nextIcon ? !nextIcon.classList.contains("disabled") : false,
      previous: prevIcon ? !prevIcon.classList.contains("disabled") : false,
      seek: true,
      set_position: true,
    };

    return { metadata: meta, playback, capabilities };
  }

  private async commandCarousel(cmd: string, positionMs?: number): Promise<void> {
    switch (cmd) {
      case "play_pause":
      case "play":
      case "pause": {
        const a = document.querySelector<HTMLAudioElement>("audio");
        if (!a) break;
        const isPlaying = !a.paused;
        if (cmd === "play" && isPlaying) break;
        if (cmd === "pause" && !isPlaying) break;
        if (isPlaying) { a.pause(); } else { a.play().catch(() => {}); }
        break;
      }
      case "next": {
        const btn = this.qs<HTMLElement>(
          ".carousel-player .next .next-icon"
        );
        if (btn && !btn.classList.contains("disabled")) btn.click();
        break;
      }
      case "previous": {
        const btn = this.qs<HTMLElement>(
          ".carousel-player .prev .prev-icon"
        );
        if (btn && !btn.classList.contains("disabled")) btn.click();
        break;
      }
      case "set_position":
      case "seek": {
        if (typeof positionMs === "number" && isFinite(positionMs)) {
          const a = document.querySelector<HTMLAudioElement>("audio");
          if (a) a.currentTime = Math.max(0, positionMs / 1000);
        }
        break;
      }
    }
  }

  // ── Inline player (album/track pages) ─────────────────────────

  private extractInline(): ProviderResult | null {
    const player = this.qs<HTMLElement>(".inline_player");
    if (!player) return null;

    const audio = document.querySelector<HTMLAudioElement>("audio");
    // Need audio loaded to be useful
    if (!audio || !isFinite(audio.duration) || audio.duration <= 0) return null;

    const meta: MediaMetadata = {
      title: undefined,
      artist: [],
      album: undefined,
      album_artist: [],
      art_url: undefined,
      track_id: undefined,
    };

    // Track title: .inline_player .title
    meta.title =
      this.qsText(".inline_player .title_link .title") ||
      this.qsText(".inline_player .title") ||
      undefined;
    if (!meta.title) return null;

    // Artist: from page context
    const pageArtist =
      this.qsText("#name-section h3 a") ||
      this.qsText("#name-section span") ||
      this.qsText(".detail_item a[href*=\"/music\"]");
    if (pageArtist) meta.artist = [pageArtist.trim()];

    // Album: from page context
    meta.album =
      this.qsText("h2.trackTitle") ||
      this.qsText(".trackTitle") ||
      undefined;

    // Artwork: album page cover art
    meta.art_url = this.resolveArtwork(
      document.querySelector<HTMLImageElement>(
        "#tralbumArt img, a.popupImage img"
      )?.src
    );

    // Track ID from audio src
    const srcMatch = audio.getAttribute("src")?.match(this.trackIdParam);
    if (srcMatch) meta.track_id = `bc:${srcMatch[1]}`;

    // Playing state: audio element
    const isPlaying = !audio.paused;

    // Position/duration from audio (most reliable) or time spans
    let positionMs = Math.floor((audio.currentTime || 0) * 1000);
    let durationMs = Math.floor(audio.duration * 1000);

    // Fallback to time spans if audio position is 0 but playing
    if (positionMs === 0 && durationMs > 0) {
      const elapsed = this.qsText(".inline_player .time_elapsed");
      if (elapsed) positionMs = parseTimeSpan(elapsed);
    }

    const playback: PlaybackState = {
      status: isPlaying ? "playing" : "paused",
      position_ms: positionMs,
      duration_ms: durationMs,
    };

    const capabilities: Capabilities = {
      play_pause: true,
      next: !!this.qs<HTMLElement>(".inline_player .nextbutton"),
      previous: !!this.qs<HTMLElement>(".inline_player .prevbutton"),
      seek: true,
      set_position: true,
    };

    return { metadata: meta, playback, capabilities };
  }

  private async commandInline(cmd: string, positionMs?: number): Promise<void> {
    switch (cmd) {
      case "play_pause":
      case "play":
      case "pause": {
        const a = document.querySelector<HTMLAudioElement>("audio");
        if (!a) break;
        const isPlaying = !a.paused;
        if (cmd === "play" && isPlaying) break;
        if (cmd === "pause" && !isPlaying) break;
        if (isPlaying) { a.pause(); } else { a.play().catch(() => {}); }
        break;
      }
      case "next": {
        const btn = this.qs<HTMLElement>(".inline_player .nextbutton");
        btn?.click();
        break;
      }
      case "previous": {
        const btn = this.qs<HTMLElement>(".inline_player .prevbutton");
        btn?.click();
        break;
      }
      case "set_position":
      case "seek": {
        if (typeof positionMs === "number" && isFinite(positionMs)) {
          const a = document.querySelector<HTMLAudioElement>("audio");
          if (a) a.currentTime = Math.max(0, positionMs / 1000);
        }
        break;
      }
    }
  }

  // ── Helpers ──────────────────────────────────────────────────

  private qs<T extends HTMLElement>(selector: string): T | null {
    return document.querySelector<T>(selector);
  }

  private qsText(selector: string): string {
    return document.querySelector<HTMLElement>(selector)?.textContent?.trim() || "";
  }

  /**
   * Upgrade Bandcamp artwork to highest available resolution.
   * _16 = 700×700, _10 = 1200×1200, _2 = 350×350
   */
  private resolveArtwork(url: string | undefined): string | undefined {
    if (!url) return undefined;
    if (url.includes("bcbits.com")) {
      url = url.replace(/_16\./, "_10.");
      url = url.replace(/_2\./, "_10.");
    }
    return url;
  }
}

/**
 * Parse "mm:ss" or "h:mm:ss" time string to milliseconds.
 */
function parseTimeSpan(text: string | null | undefined): number {
  if (!text) return 0;
  const parts = text.trim().split(":");
  if (parts.length === 2) {
    const mins = parseInt(parts[0], 10) || 0;
    const secs = parseInt(parts[1], 10) || 0;
    return (mins * 60 + secs) * 1000;
  }
  if (parts.length === 3) {
    const hrs = parseInt(parts[0], 10) || 0;
    const mins = parseInt(parts[1], 10) || 0;
    const secs = parseInt(parts[2], 10) || 0;
    return (hrs * 3600 + mins * 60 + secs) * 1000;
  }
  return 0;
}
