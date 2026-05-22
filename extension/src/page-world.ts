/**
 * Page-world script — runs in the page's JavaScript context, not the
 * extension's isolated world. This allows direct access to page DOM,
 * Media Session API, and JavaScript variables without CSP issues.
 *
 * Communicates back to content.ts via CustomEvent.
 *
 * Injected as a static <script src="page-world.js"> element from content.ts.
 */

(function () {
  // Prevent double injection
  if ((window as any).__mprisence_page_world) return;
  (window as any).__mprisence_page_world = true;

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

    let metadata: Record<string, any> = { artist: [] };
    let playback = { status: "stopped", position_ms: 0, duration_ms: 0, rate: 1.0 };
    let capabilities = {
      play_pause: true,
      next: false,
      previous: false,
      seek: false,
      set_position: false,
      raise: true,
    };
    let confidence = "dom";

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

        confidence = "provider";
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
        rate: media.playbackRate || 1.0,
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
      confidence,
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

  function checkMetadataAndDispatch(): void {
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

  // Initial dispatch
  dispatch(collectState());
})();
