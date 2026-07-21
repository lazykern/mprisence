/**
 * Page-world script — runs in the page's JavaScript context, not the
 * extension's isolated world. This allows direct access to page DOM,
 * Media Session API, and JavaScript variables without CSP issues.
 *
 * Communicates back to content.ts via CustomEvent.
 *
 * Injected as a manifest-declared MAIN-world content script.
 */

import {
  pickArtwork,
  isNotificationSound,
  hasPublishableIdentity,
} from "./utils/generic-media";

(function () {
  // Prevent double injection
  if ((window as any).__mprisence_page_world) return;
  (window as any).__mprisence_page_world = true;

  const SUPPORTED_ORIGINS = [
    "https://music.youtube.com",
    "https://www.youtube.com",
    "https://soundcloud.com",
    "https://bandcamp.com",
    "https://tidal.com",
    "https://music.apple.com",
  ];
  const host = window.location.hostname;
  const origin = window.location.origin;
  const supported =
    SUPPORTED_ORIGINS.includes(origin) ||
    host.endsWith(".soundcloud.com") ||
    host.endsWith(".bandcamp.com") ||
    host.endsWith(".tidal.com");

  // On an unsupported origin this script is only present because the user
  // enabled the generic fallback (background dynamically registers it on
  // <all_urls> minus supported sites). So: supported → rich provider path
  // below; unsupported → generic collector, then return.
  if (!supported) {
    startGenericMode();
    return;
  }

  /** Debounce utility */
  function debounce<T extends (...args: any[]) => void>(
    fn: T,
    ms: number
  ): T {
    let timer: ReturnType<typeof setTimeout> | null = null;
    return ((...args: any[]) => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = null;
        fn(...args);
      }, ms);
    }) as T;
  }

  /** Send media data to content script via CustomEvent */
  function dispatch(data: Record<string, any>): void {
    window.dispatchEvent(
      new CustomEvent("mprisence-media-state", { detail: data })
    );
  }

  /** Collect state from a media element + Media Session API */
  function collectState(): Record<string, any> {
    const video = document.querySelector("video");
    const audio = document.querySelector("audio");
    const media = video ?? audio;

    let metadata: Record<string, any> = { artist: [], album_artist: [] };
    let playback = { status: "stopped", position_ms: 0, duration_ms: 0 };
    let capabilities = {
      play_pause: true,
      next: false,
      previous: false,
      seek: false,
      set_position: false,
    };

    // Get metadata from Media Session API (richer than DOM)
    try {
      const ms = (navigator as any).mediaSession;
      if (ms?.metadata) {
        const md = ms.metadata;
        metadata = {
          title: md.title || undefined,
          artist: md.artist ? [md.artist] : [],
          album: md.album || undefined,
          album_artist: [],
          art_url: undefined,
          track_id: undefined,
        };

        // Pick largest artwork, then upgrade to highest resolution
        if (md.artwork?.length > 0) {
          const best = md.artwork.reduce((a: any, b: any) => {
            const aSize = parseInt(a.sizes) || 0;
            const bSize = parseInt(b.sizes) || 0;
            return aSize > bSize ? a : b;
          });
          metadata.art_url = resolveArtworkUrl(best.src || undefined);
        }

      }
    } catch {
      // Media Session not available
    }

    if (media) {
      playback = {
        status: media.paused
          ? "paused"
          : media.ended
            ? "stopped"
            : "playing",
        position_ms: Math.floor((media.currentTime || 0) * 1000),
        duration_ms: Math.floor((media.duration || 0) * 1000),
      };

      capabilities = {
        ...capabilities,
        play_pause: true,
        seek: true,
        set_position: true,
      };
    }

    return {
      type: "media-state",
      metadata,
      playback,
      capabilities,
    };
  }

  // ─── Observers ──────────────────────────────────────────────

  // Track last METADATA identity so we only dispatch on actual
  // track/metadata changes — NOT on position updates. The isolated
  // world (content.ts) handles position updates via timeupdate.
  //
  // We DO the periodic poll here (Metadata identity comparison)
  // because `MediaSession` metadata changes (new track) can only
  // be detected from the page world (isolated world has no access).
  let lastMeta = "";

  function metaIdentity(): string {
    const ms = (navigator as any).mediaSession;
    if (!ms?.metadata) return "";
    const md = ms.metadata;
    return JSON.stringify({ t: md.title, a: md.artist, l: md.album, u: md.artwork?.[0]?.src });
  }

  function isYoutubeAdPlaying(): boolean {
    return !!document.querySelector('.ad-showing');
  }

  function checkMetadataAndDispatch(): void {
    // Skip updates while a YouTube ad is playing to prevent ad metadata
    // (MediaSession API) from leaking into mprisence as track info.
    if (isYoutubeAdPlaying()) return;

    const id = metaIdentity();
    if (id && id !== lastMeta) {
      lastMeta = id;
      dispatch(collectState());
    }
  }

  // Poll for Media Session metadata changes (Media Session API has no callback)
  setInterval(() => {
    checkMetadataAndDispatch();
  }, 1000);

  // Watch for new media elements being added (signals a new page/track)
  const observer = new MutationObserver(() => {
    if (document.querySelector("video, audio")) {
      checkMetadataAndDispatch();
    }
  });
  observer.observe(document.body || document.documentElement, {
    childList: true,
    subtree: true,
  });

  // Also catch metadata that arrives via DOM events like playing/loadstart
  document.addEventListener("playing", () => checkMetadataAndDispatch(), true);
  document.addEventListener("loadedmetadata", () => checkMetadataAndDispatch(), true);

  /** Upgrade artwork URL to highest available resolution */
  function resolveArtworkUrl(url: string | undefined): string | undefined {
    if (!url) return undefined;
    // SoundCloud sndcdn.com: use t500x500 (largest standard square)
    if (url.includes("sndcdn.com") || url.includes("soundcloud")) {
      url = url.replace(/-t\d+x\d+(?=\.[a-z]+)/i, "-t500x500");
      url = url.replace(/-original(?=\.[a-z]+)/i, "-t500x500");
      url = url.replace(/-crop-[a-z]+(?=\.[a-z]+)/i, "");
    }
    // YouTube ytimg.com: upgrade to maxresdefault
    if (url.includes("ytimg.com") || url.includes("yt3.")) {
      url = url.replace(/\/[a-z]+default\./g, "/maxresdefault.");
      url = url.replace(/=[a-z0-9-]+$/, "");
    }
    return url;
  }

  // ─── YTM square cover art via InnerTube API ──────────────
  //
  // YouTube Music hides square album art behind InnerTube.
  // Standard ytimg thumbnails (maxresdefault/hqdefault) are 16:9
  // with black bars (art tracks) or wrong images.
  // InnerTube returns square yt3.googleusercontent.com URLs (544-1400px).
  //
  // This runs in page-world so we have the page's cookies + fetch access.

  let lastYtmVideoId = "";
  let cachedSquareArt: Record<string, string> = {};
  let pendingFetch: Record<string, true> = {};
  let dispatchedVideos: Record<string, true> = {};

  const INNERTUBE_KEY = "AIzaSyC9XL3ZjB78yOKadE1T3dT4iSfB9l6stUU";
  const INNERTUBE_CLIENT = "WEB_REMIX";
  const INNERTUBE_VER = "1.20250521.00.00";

  /** Fetch square cover art for a YTM video ID via InnerTube API */
  async function fetchSquareArt(videoId: string): Promise<string | null> {
    if (cachedSquareArt[videoId]) {
      // Already fetched — just dispatch the cached art
      dispatchSquareArt(videoId, cachedSquareArt[videoId]);
      return cachedSquareArt[videoId];
    }
    if (pendingFetch[videoId]) return null;
    pendingFetch[videoId] = true;

    try {
      const resp = await fetch(
        "https://music.youtube.com/youtubei/v1/player?key=" + INNERTUBE_KEY,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({
            context: {
              client: {
                clientName: INNERTUBE_CLIENT,
                clientVersion: INNERTUBE_VER,
                hl: "en",
              },
            },
            videoId: videoId,
          }),
        }
      );
      const data = await resp.json();
      const thumbs = data?.videoDetails?.thumbnail?.thumbnails;
      if (thumbs && thumbs.length > 0) {
        // Pick largest square thumbnail (yt3.googleusercontent.com)
        var best = thumbs[0];
        for (var i = 1; i < thumbs.length; i++) {
          if ((thumbs[i].width || 0) > (best.width || 0)) best = thumbs[i];
        }
        if (best?.url && best.url.indexOf(".googleusercontent.com") > -1) {
          // Upgrade to 544px JPEG (good quality, small size)
          var url = best.url.replace(/=[a-z0-9-]+$/, "=w544-h544-l90-rj");
          cachedSquareArt[videoId] = url;
          // Dispatch immediately when fetch resolves
          dispatchSquareArt(videoId, url);
          return url;
        }
      }
      return null;
    } catch {
      return null;
    } finally {
      delete pendingFetch[videoId];
    }
  }

  /** Dispatch square art to content script via CustomEvent */
  function dispatchSquareArt(videoId: string, artUrl: string): void {
    // Only dispatch once per video — prevents flapping with the YTM
    // provider's DOM poll (which keeps sending maxresdefault from the
    // img element every second).
    if (dispatchedVideos[videoId]) return;
    dispatchedVideos[videoId] = true;

    dispatch({
      type: "media-state",
      metadata: {
        artist: [],
        album_artist: [],
        art_url: artUrl,
      },
      playback: {
        status: "playing",
        position_ms: 0,
        duration_ms: 0,
      },
      capabilities: {
        play_pause: true,
        next: false,
        previous: false,
        seek: false,
        set_position: false,
      },
    });
  }

  /** YTM: detect video ID changes from page URL, trigger fetch */
  function checkYtmVideoId(): void {
    if (window.location.hostname !== "music.youtube.com") return;

    var params = new URLSearchParams(window.location.search);
    var videoId = params.get("v") || "";
    if (!videoId) return;

    if (videoId !== lastYtmVideoId) {
      lastYtmVideoId = videoId;
      delete dispatchedVideos[videoId]; // Allow dispatch for new track
      // Kick off InnerTube fetch — dispatches on resolve
      fetchSquareArt(videoId);
    }
  }

  // Poll for YTM video ID changes (1s interval alongside MediaSession check)
  setInterval(function () {
    checkYtmVideoId();
  }, 1000);

  // Initial dispatch (skip during ads)
  if (!isYoutubeAdPlaying()) {
    dispatch(collectState());
  }
  // Also trigger initial YTM check
  checkYtmVideoId();

  // ─── Generic mode (unsupported sites) ────────────────────────
  //
  // Runs only when this script was dynamically injected on an
  // unsupported origin (user enabled the generic fallback). Reads the
  // page's own Media Session + the active media element, captures the
  // page's action handlers so Next/Previous can be routed back, and
  // executes commands relayed from the isolated world.
  function startGenericMode(): void {
    // Active element = last one to actually play. Beats a naive
    // querySelector: catches shadow-DOM / dynamically-added elements and
    // ignores idle ones. Hook the prototype so we see every play() call.
    let activeMedia: HTMLMediaElement | null = null;

    function setActive(el: HTMLMediaElement | null): void {
      activeMedia = el;
    }

    try {
      const proto = HTMLMediaElement.prototype;
      const origPlay = proto.play;
      proto.play = function (this: HTMLMediaElement) {
        setActive(this);
        return origPlay.apply(this, arguments as any);
      };
    } catch {
      // ignore — fall back to event-based detection below
    }

    // Backup: catch elements that started without our wrapped play()
    // (autoplay, media-key play) and pick the first present element.
    document.addEventListener(
      "play",
      (e) => {
        const t = e.target;
        if (t instanceof HTMLMediaElement) setActive(t);
      },
      true
    );
    if (!activeMedia) {
      const existing = document.querySelector<HTMLMediaElement>("video, audio");
      if (existing) setActive(existing);
    }

    // ── Capture the page's Media Session action handlers ──
    // We need to know which actions the page supports (for CanGoNext /
    // CanGoPrevious) and hold the handler refs so a relayed Next/Previous
    // can invoke the page's own logic. Play/Pause/Seek go to the element
    // directly, so we don't depend on handlers for those.
    const handlers = new Map<string, MediaSessionActionHandler | null>();
    try {
      const ms: any = (navigator as any).mediaSession;
      if (ms?.setActionHandler) {
        const orig = ms.setActionHandler.bind(ms);
        ms.setActionHandler = (
          action: string,
          handler: MediaSessionActionHandler | null
        ) => {
          if (handler) handlers.set(action, handler);
          else handlers.delete(action);
          scheduleDispatch();
          return orig(action, handler);
        };
      }
    } catch {
      // ignore
    }

    function faviconUrl(): string | undefined {
      const links = Array.from(
        document.querySelectorAll<HTMLLinkElement>('link[rel~="icon"]')
      );
      let best: string | undefined;
      let bestArea = -1;
      for (const l of links) {
        const area = (() => {
          const m = /^(\d+)x(\d+)$/i.exec(l.sizes?.value?.trim().split(/\s+/)[0] ?? "");
          return m ? parseInt(m[1]) * parseInt(m[2]) : 0;
        })();
        if (l.href && area > bestArea) {
          bestArea = area;
          best = l.href;
        }
      }
      return best;
    }

    function metaContent(sel: string): string | undefined {
      const el = document.querySelector<HTMLMetaElement>(sel);
      const v = el?.content?.trim();
      return v || undefined;
    }

    function collectGeneric(keepalive = false): Record<string, any> | null {
      const media = activeMedia;

      // Ad / notification-sound floor: ignore short finite clips.
      const durSec = media?.duration ?? NaN;
      const isNoise = media ? isNotificationSound(durSec) : false;
      const usableMedia = media && !isNoise ? media : null;

      // Metadata: Media Session first, then page-level fallbacks.
      let title: string | undefined;
      let artist: string[] = [];
      let album: string | undefined;
      let artUrl: string | undefined;

      const ms: any = (navigator as any).mediaSession;
      const md = ms?.metadata;
      if (md) {
        title = md.title || undefined;
        artist = md.artist ? [md.artist] : [];
        album = md.album || undefined;
        artUrl = resolveArtworkUrl(pickArtwork(md.artwork as any));
      }

      // Only publish when there's genuine media intent: real usable media,
      // or the page set its own Media Session. Otherwise this is just a web
      // page (or a filtered notification sound) and shouldn't be a player.
      if (!usableMedia && !md) return null;

      if (!title) title = metaContent('meta[property="og:title"]') || document.title || undefined;
      if (!artUrl) artUrl = metaContent('meta[property="og:image"]') || faviconUrl();
      if (!album) album = metaContent('meta[property="og:site_name"]');

      if (!hasPublishableIdentity(title, artist)) return null;

      const playback = usableMedia
        ? {
            status: usableMedia.paused
              ? "paused"
              : usableMedia.ended
                ? "stopped"
                : "playing",
            position_ms: Math.floor((usableMedia.currentTime || 0) * 1000),
            duration_ms: Number.isFinite(usableMedia.duration)
              ? Math.floor(usableMedia.duration * 1000)
              : 0,
          }
        : { status: "playing", position_ms: 0, duration_ms: 0 };

      const seekable = !!usableMedia && Number.isFinite(usableMedia.duration);
      const capabilities = {
        play_pause: !!usableMedia,
        next: handlers.has("nexttrack"),
        previous: handlers.has("previoustrack"),
        seek: seekable,
        set_position: seekable,
      };

      return {
        type: "media-state",
        metadata: { title, artist, album, album_artist: [], art_url: artUrl },
        playback,
        capabilities,
        keepalive,
      };
    }

    let lastGenericId = "";
    function scheduleDispatch(force = false): void {
      const state = collectGeneric(force);
      if (!state) return;
      const id = JSON.stringify({
        t: state.metadata.title,
        a: state.metadata.artist,
        l: state.metadata.album,
        u: state.metadata.art_url,
        s: state.playback.status,
        p: Math.floor(state.playback.position_ms / 1000),
        c: state.capabilities,
      });
      if (!force && id === lastGenericId) return;
      lastGenericId = id;
      dispatch(state);
    }

    const onTimeupdate = (() => {
      let last = 0;
      return () => {
        const now = Date.now();
        if (now - last < 900) return;
        last = now;
        scheduleDispatch();
      };
    })();

    for (const ev of ["play", "pause", "ended", "loadedmetadata", "durationchange", "ratechange", "seeked"]) {
      document.addEventListener(ev, () => scheduleDispatch(), true);
    }
    document.addEventListener("timeupdate", onTimeupdate, true);

    // Metadata can change with no media event (SPA track switch); poll identity.
    setInterval(() => scheduleDispatch(), 1000);
    // Keepalive so the bridge's stale-pruner keeps a paused tab alive.
    setInterval(() => scheduleDispatch(true), 30_000);

    // ── Command channel: isolated world → here ──
    window.addEventListener("mprisence-command", ((e: CustomEvent) => {
      const { command, position_ms } = e.detail || {};
      const m = activeMedia;
      switch (command) {
        case "play_pause":
          if (m) m.paused ? m.play().catch(() => {}) : m.pause();
          break;
        case "play":
          if (m?.paused) m.play().catch(() => {});
          break;
        case "pause":
          if (m && !m.paused) m.pause();
          break;
        case "set_position":
          if (m && typeof position_ms === "number" && Number.isFinite(position_ms)) {
            m.currentTime = Math.max(0, position_ms / 1000);
          }
          break;
        case "next":
          handlers.get("nexttrack")?.({ action: "nexttrack" } as any);
          break;
        case "previous":
          handlers.get("previoustrack")?.({ action: "previoustrack" } as any);
          break;
      }
      // Reflect the new state promptly.
      scheduleDispatch(true);
    }) as EventListener);

    scheduleDispatch(true);
  }
})();
