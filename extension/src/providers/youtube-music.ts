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

const MINIPLAYER = "ytmusic-miniplayer";

/** Player-bar selectors, classic layout first, then the Wiz miniplayer. */
const SELECTORS = {
  title: [".title.ytmusic-player-bar", `${MINIPLAYER} .ytmusicTrackInfoTitle`],
  byline: [".byline.ytmusic-player-bar", `${MINIPLAYER} .ytmusicTrackInfoBylineItem`],
  art: ["ytmusic-player-bar img", `${MINIPLAYER} img.ytmusicTrackInfoThumbnail`],
  playPause: ["#play-pause-button", `${MINIPLAYER} .ytmusicPlayerControlsPlayPauseButton button`],
  next: ["yt-icon-button.next-button button", `${MINIPLAYER} .ytmusicPlayerControlsNextButton button`],
  previous: ["yt-icon-button.previous-button button", `${MINIPLAYER} .ytmusicPlayerControlsPreviousButton button`],
} as const;

/**
 * True when the shown title names the player API's track. The player bar may
 * add a suffix the API lacks ("Song" → "Song (feat. X)"), never drop one, so a
 * stale "Song (remix)" doesn't match a newly shown "Song".
 */
export function sameYouTubeMusicTitle(apiTitle: string, shownTitle: string): boolean {
  const api = apiTitle.trim().toLowerCase();
  const shown = shownTitle.trim().toLowerCase();
  return !!api && !!shown && shown.startsWith(api);
}

/**
 * YouTube Music provider.
 *
 * Classic layout (verified live):
 *   titleEl:  .title.ytmusic-player-bar
 *   artistEl: .byline.ytmusic-player-bar  → "Artist • Album • Year" | "Artist • ## views • ## likes"
 *   artImg:   ytmusic-player-bar img
 *   prevBtn:  yt-icon-button.previous-button
 *   nextBtn:  yt-icon-button.next-button
 *   playBtn:  #play-pause-button  → title="Play"|"Pause"
 *   progress: #progress-bar  → aria-valuenow/aria-valuemax
 *
 * Wiz miniplayer layout (`music_web_enable_wiz_miniplayer`, verified live):
 *   ytmusic-miniplayer replaces ytmusic-player-bar in both player states.
 *   titleEl:  .ytmusicTrackInfoTitle[title]
 *   artistEl: .ytmusicTrackInfoBylineItem[title]  → "Artist • Album • Year"
 *   artImg:   img.ytmusicTrackInfoThumbnail
 *   buttons:  .ytmusicPlayerControls{PlayPause,Next,Previous}Button button
 *   progress: input.ytMusicMiniPlayerProgressBar  → value/max in seconds
 *
 * Both layouts: video (blob URL, has currentTime/duration)
 *
 * Key findings:
 *   - The isolated world can't reach the player API; page-world publishes
 *     its video ID/title/author as data-mprisence-ytm-* attributes
 *   - Video bylines have no album - only "Artist • views • likes"
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

    const titleText = this.firstText(SELECTORS.title);
    const artImg = this.playerArtImage();
    const playBtn = this.firstVisibleControl(SELECTORS.playPause);
    const video = this.qs<HTMLVideoElement>("video");

    if (!titleText && !video) return null;

    // ── Title ──────────────────────────────────────────────────
    // A bare "YouTube Music" document title means no track is shown yet.
    const docTitle = document.title?.match(/^(.*\S)\s+[-|]\s+YouTube Music$/)?.[1] ?? "";
    const pageWorldTitle = document.documentElement
      ?.getAttribute("data-mprisence-ytm-video-title") ?? "";
    const title = titleText || docTitle || pageWorldTitle || undefined;

    // ── Artist & Album from byline ───────────────────────────
    // Format: "Artist • Album • Year" or "Artist • ## views • ## likes"
    const byline = this.firstText(SELECTORS.byline)
      || document.documentElement?.getAttribute("data-mprisence-ytm-video-author")
      || "";
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
    // Page-world publishes Media Session artwork paired with its video ID.
    // When it matches, it is this track's own art: use it as-is.
    const [sessionArtId, sessionArtUrl] =
      document.documentElement?.getAttribute("data-mprisence-ytm-art")?.split(" ") ?? [];
    const sessionArt = videoId && sessionArtId === videoId && sessionArtUrl
      ? sessionArtUrl
      : undefined;

    let artUrl = sessionArt ?? (artImg?.src || undefined);
    // Skip 1×1 placeholder GIFs
    if (artUrl && artUrl.startsWith("data:")) artUrl = undefined;

    if (sessionArt) {
      // Already the track's artwork.
    } else if (artUrl) {
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
    // progress element (aria-valuenow/aria-valuemax, or the miniplayer's
    // range input). If unavailable, skip instead of publishing queue time
    // as track time.
    const { now: progressNow, max: progressMax } = this.trackProgress();
    const trackPositionSec = (isFinite(progressNow) && progressNow >= 0) ? progressNow : undefined;
    const trackDurationSec = (isFinite(progressMax) && progressMax > 0) ? progressMax : undefined;

    const validVideoDuration = video && video.readyState >= 2 &&
      Number.isFinite(video.duration) && video.duration > 0;
    if (trackPositionSec === undefined || trackDurationSec === undefined) {
      if (!validVideoDuration || video.duration > 600) return null;
    }

    const hasStartupProgressPlaceholder =
      trackPositionSec === 0 &&
      trackDurationSec === 100 &&
      (!validVideoDuration || Math.abs(video.duration - trackDurationSec) > 1);

    if (hasStartupProgressPlaceholder) {
      return null;
    }

    const currentSec = trackPositionSec ?? (video?.currentTime || 0);
    const totalSec = trackDurationSec ?? video!.duration;

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
      trackArt: !!sessionArt,
    };
  }

  async command(cmd: string, positionMs?: number): Promise<void> {
    // Class-selector map (verified live - there are no #id selectors for prev/next)
    if (cmd === "set_position") {
      if (typeof positionMs !== "number" || !isFinite(positionMs)) return;
      // Page-world seeks via the player API; the <video> timeline can span
      // the whole queue, so currentTime is only a fallback.
      if (document.documentElement?.hasAttribute?.("data-mprisence-ytm-video-id")) {
        window.dispatchEvent(new CustomEvent("mprisence-ytm-seek", { detail: positionMs }));
        return;
      }
      const video = this.qs<HTMLVideoElement>("video");
      if (video) video.currentTime = Math.max(0, positionMs / 1000);
      return;
    }

    if (cmd === "play" || cmd === "pause") {
      const video = this.qs<HTMLVideoElement>("video");
      if (cmd === "play" && !video?.paused) return;
      if (cmd === "pause" && video?.paused) return;
    }

    const btnMap: Record<string, readonly string[]> = {
      play_pause: SELECTORS.playPause,
      play: SELECTORS.playPause,
      pause: SELECTORS.playPause,
      next: SELECTORS.next,
      previous: SELECTORS.previous,
    };

    const selectors = btnMap[cmd];
    if (selectors) {
      this.firstVisibleControl(selectors)?.click();
    }
  }

  private qs<T extends HTMLElement>(selector: string): T | null {
    return document.querySelector<T>(selector);
  }

  private isVisible(el: HTMLElement): boolean {
    return !el.getClientRects || el.getClientRects().length > 0;
  }

  private visibleControl(selector: string): HTMLElement | null {
    const controls = Array.from(document.querySelectorAll<HTMLElement>(selector));
    return controls.find((control) => this.isVisible(control))
      ?? controls[0]
      ?? null;
  }

  private firstVisibleControl(selectors: readonly string[]): HTMLElement | null {
    for (const selector of selectors) {
      const control = this.visibleControl(selector);
      if (control) return control;
    }
    return null;
  }

  /** Text of the first visible non-empty match, trying layouts in order. */
  private firstText(selectors: readonly string[]): string {
    for (const selector of selectors) {
      const texts = Array.from(document.querySelectorAll<HTMLElement>(selector))
        .map((el) => ({
          el,
          text: el.textContent?.trim() || el.getAttribute?.("title")?.trim() || "",
        }))
        .filter(({ text }) => text);
      const match = texts.find(({ el }) => this.isVisible(el)) ?? texts[0];
      if (match) return match.text;
    }
    return "";
  }

  private playerArtImage(): HTMLImageElement | null {
    for (const selector of SELECTORS.art) {
      const images = Array.from(document.querySelectorAll<HTMLImageElement>(selector));
      const image = images.find((img) => this.videoIdRegex.test(img.src)) ?? images[0];
      if (image) return image;
    }
    return null;
  }

  /** Returns null while page-world's video ID lags a track change. */
  private currentVideoId(title: string | undefined): string | null {
    const root = document.documentElement;
    const fromPageWorld = root?.getAttribute("data-mprisence-ytm-video-id");
    if (fromPageWorld && /^[a-zA-Z0-9_-]+$/.test(fromPageWorld)) {
      const idTitle = root?.getAttribute("data-mprisence-ytm-video-title");
      if (!idTitle || !title || sameYouTubeMusicTitle(idTitle, title)) {
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

  /** Per-track position/duration in seconds (NaN when unavailable). */
  private trackProgress(): { now: number; max: number } {
    const bar = this.visibleProgressBar();
    if (bar) {
      return {
        now: parseFloat(bar.getAttribute("aria-valuenow") ?? ""),
        max: parseFloat(bar.getAttribute("aria-valuemax") ?? ""),
      };
    }
    const slider = this.qs<HTMLInputElement>(`${MINIPLAYER} input.ytMusicMiniPlayerProgressBar`);
    return {
      now: slider ? parseFloat(slider.value) : NaN,
      max: slider ? parseFloat(slider.max) : NaN,
    };
  }

  private visibleProgressBar(): HTMLElement | null {
    const bars = Array.from(document.querySelectorAll<HTMLElement>("#progress-bar"));
    const visible = bars.filter((bar) => this.isVisible(bar));
    return visible.find((bar) => bar.getAttribute("aria-disabled") !== "true")
      ?? visible[0]
      ?? bars[0]
      ?? null;
  }
}
