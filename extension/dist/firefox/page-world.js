// src/page-world.ts
(function() {
  if (window.__mprisence_page_world) return;
  window.__mprisence_page_world = true;
  function debounce(fn, ms) {
    let timer = null;
    return ((...args) => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(() => {
        timer = null;
        fn(...args);
      }, ms);
    });
  }
  function dispatch(data) {
    window.dispatchEvent(
      new CustomEvent("mprisence-media-state", { detail: data })
    );
  }
  function collectState() {
    const video = document.querySelector("video");
    const audio = document.querySelector("audio");
    const media = video ?? audio;
    let metadata = { artist: [] };
    let playback = { status: "stopped", position_ms: 0, duration_ms: 0, rate: 1 };
    let capabilities = {
      play_pause: true,
      next: false,
      previous: false,
      seek: false,
      set_position: false,
      raise: true
    };
    let confidence = "dom";
    try {
      const ms = navigator.mediaSession;
      if (ms?.metadata) {
        const md = ms.metadata;
        metadata = {
          title: md.title || void 0,
          artist: md.artist ? [md.artist] : [],
          album: md.album || void 0,
          album_artist: [],
          art_url: void 0,
          track_id: void 0
        };
        if (md.artwork?.length > 0) {
          const best = md.artwork.reduce((a, b) => {
            const aSize = parseInt(a.sizes) || 0;
            const bSize = parseInt(b.sizes) || 0;
            return aSize > bSize ? a : b;
          });
          metadata.art_url = resolveArtworkUrl(best.src || void 0);
        }
        confidence = "provider";
      }
    } catch {
    }
    if (media) {
      playback = {
        status: media.paused ? "paused" : media.ended ? "stopped" : "playing",
        position_ms: Math.floor((media.currentTime || 0) * 1e3),
        duration_ms: Math.floor((media.duration || 0) * 1e3),
        rate: media.playbackRate || 1
      };
      capabilities = {
        ...capabilities,
        play_pause: true,
        seek: true,
        set_position: true
      };
    }
    return {
      type: "media-state",
      metadata,
      playback,
      capabilities,
      confidence
    };
  }
  const dispatchDebounced = debounce(() => {
    dispatch(collectState());
  }, 500);
  try {
    const ms = navigator.mediaSession;
    if (ms?.metadata) {
      setInterval(() => {
        dispatchDebounced();
      }, 1e3);
    }
  } catch {
  }
  const observer = new MutationObserver(() => {
    const media = document.querySelector("video, audio");
    if (media) {
      dispatchDebounced();
    }
  });
  observer.observe(document.body || document.documentElement, {
    childList: true,
    subtree: true
  });
  document.addEventListener(
    "timeupdate",
    ((e) => {
      const target = e.target;
      if (target && (target.tagName === "VIDEO" || target.tagName === "AUDIO")) {
        dispatchDebounced();
      }
    }),
    true
    // capture
  );
  function resolveArtworkUrl(url) {
    if (!url) return void 0;
    if (url.includes("sndcdn.com") || url.includes("soundcloud")) {
      url = url.replace(/-t\d+x\d+(?=\.[a-z]+)/i, "-t500x500");
      url = url.replace(/-original(?=\.[a-z]+)/i, "-t500x500");
      url = url.replace(/-crop-[a-z]+(?=\.[a-z]+)/i, "");
    }
    if (url.includes("ytimg.com") || url.includes("yt3.")) {
      url = url.replace(/\/[a-z]+default\./g, "/maxresdefault.");
      url = url.replace(/=[a-z0-9-]+$/, "");
    }
    return url;
  }
  dispatch(collectState());
})();
//# sourceMappingURL=page-world.js.map
