// src/utils/generic-media.ts
function pickArtwork(artwork) {
  if (!artwork || artwork.length === 0) return void 0;
  let best;
  let bestArea = -1;
  for (const entry of artwork) {
    if (!entry?.src) continue;
    const area = artworkArea(entry.sizes);
    if (area > bestArea) {
      bestArea = area;
      best = entry;
    }
  }
  return best?.src;
}
function artworkArea(sizes) {
  if (!sizes) return 0;
  let max = 0;
  for (const token of sizes.trim().split(/\s+/)) {
    if (token.toLowerCase() === "any") return Number.POSITIVE_INFINITY;
    const m = /^(\d+)x(\d+)$/i.exec(token);
    if (m) {
      const area = parseInt(m[1], 10) * parseInt(m[2], 10);
      if (area > max) max = area;
    }
  }
  return max;
}
var NOTIFICATION_SOUND_MAX_SECONDS = 8;
function isNotificationSound(durationSec) {
  return Number.isFinite(durationSec) && durationSec > 0 && durationSec < NOTIFICATION_SOUND_MAX_SECONDS;
}
function hasPublishableIdentity(title, artist) {
  return !!(title && title.trim()) || artist.length > 0;
}

// src/page-world.ts
(function() {
  if (window.__mprisence_page_world) return;
  window.__mprisence_page_world = true;
  const SUPPORTED_ORIGINS = [
    "https://music.youtube.com",
    "https://www.youtube.com",
    "https://soundcloud.com",
    "https://bandcamp.com",
    "https://tidal.com",
    "https://music.apple.com"
  ];
  const host = window.location.hostname;
  const origin = window.location.origin;
  const supported = SUPPORTED_ORIGINS.includes(origin) || host.endsWith(".soundcloud.com") || host.endsWith(".bandcamp.com") || host.endsWith(".tidal.com");
  if (!supported) {
    startGenericMode();
    return;
  }
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
    let metadata = { artist: [], album_artist: [] };
    let playback = { status: "stopped", position_ms: 0, duration_ms: 0 };
    let capabilities = {
      play_pause: true,
      next: false,
      previous: false,
      seek: false,
      set_position: false
    };
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
      }
    } catch {
    }
    if (media) {
      playback = {
        status: media.paused ? "paused" : media.ended ? "stopped" : "playing",
        position_ms: Math.floor((media.currentTime || 0) * 1e3),
        duration_ms: Math.floor((media.duration || 0) * 1e3)
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
      capabilities
    };
  }
  let lastMeta = "";
  function metaIdentity() {
    const ms = navigator.mediaSession;
    if (!ms?.metadata) return "";
    const md = ms.metadata;
    return JSON.stringify({ t: md.title, a: md.artist, l: md.album, u: md.artwork?.[0]?.src });
  }
  function isYoutubeAdPlaying() {
    return !!document.querySelector(".ad-showing");
  }
  function checkMetadataAndDispatch() {
    if (isYoutubeAdPlaying()) return;
    const id = metaIdentity();
    if (id && id !== lastMeta) {
      lastMeta = id;
      dispatch(collectState());
    }
  }
  setInterval(() => {
    checkMetadataAndDispatch();
  }, 1e3);
  const observer = new MutationObserver(() => {
    if (document.querySelector("video, audio")) {
      checkMetadataAndDispatch();
    }
  });
  observer.observe(document.body || document.documentElement, {
    childList: true,
    subtree: true
  });
  document.addEventListener("playing", () => checkMetadataAndDispatch(), true);
  document.addEventListener("loadedmetadata", () => checkMetadataAndDispatch(), true);
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
  let lastYtmVideoId = "";
  let cachedSquareArt = {};
  let pendingFetch = {};
  let dispatchedVideos = {};
  const INNERTUBE_KEY = "AIzaSyC9XL3ZjB78yOKadE1T3dT4iSfB9l6stUU";
  const INNERTUBE_CLIENT = "WEB_REMIX";
  const INNERTUBE_VER = "1.20250521.00.00";
  async function fetchSquareArt(videoId) {
    if (cachedSquareArt[videoId]) {
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
                hl: "en"
              }
            },
            videoId
          })
        }
      );
      const data = await resp.json();
      const thumbs = data?.videoDetails?.thumbnail?.thumbnails;
      if (thumbs && thumbs.length > 0) {
        var best = thumbs[0];
        for (var i = 1; i < thumbs.length; i++) {
          if ((thumbs[i].width || 0) > (best.width || 0)) best = thumbs[i];
        }
        if (best?.url && best.url.indexOf(".googleusercontent.com") > -1) {
          var url = best.url.replace(/=[a-z0-9-]+$/, "=w544-h544-l90-rj");
          cachedSquareArt[videoId] = url;
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
  function dispatchSquareArt(videoId, artUrl) {
    if (dispatchedVideos[videoId]) return;
    dispatchedVideos[videoId] = true;
    dispatch({
      type: "media-state",
      metadata: {
        artist: [],
        album_artist: [],
        art_url: artUrl
      },
      playback: {
        status: "playing",
        position_ms: 0,
        duration_ms: 0
      },
      capabilities: {
        play_pause: true,
        next: false,
        previous: false,
        seek: false,
        set_position: false
      }
    });
  }
  function checkYtmVideoId() {
    if (window.location.hostname !== "music.youtube.com") return;
    var params = new URLSearchParams(window.location.search);
    var videoId = params.get("v") || "";
    if (!videoId) return;
    if (videoId !== lastYtmVideoId) {
      lastYtmVideoId = videoId;
      delete dispatchedVideos[videoId];
      fetchSquareArt(videoId);
    }
  }
  setInterval(function() {
    checkYtmVideoId();
  }, 1e3);
  if (!isYoutubeAdPlaying()) {
    dispatch(collectState());
  }
  checkYtmVideoId();
  function startGenericMode() {
    let activeMedia = null;
    function setActive(el) {
      activeMedia = el;
    }
    try {
      const proto = HTMLMediaElement.prototype;
      const origPlay = proto.play;
      proto.play = function() {
        setActive(this);
        return origPlay.apply(this, arguments);
      };
    } catch {
    }
    document.addEventListener(
      "play",
      (e) => {
        const t = e.target;
        if (t instanceof HTMLMediaElement) setActive(t);
      },
      true
    );
    if (!activeMedia) {
      const existing = document.querySelector("video, audio");
      if (existing) setActive(existing);
    }
    const handlers = /* @__PURE__ */ new Map();
    try {
      const ms = navigator.mediaSession;
      if (ms?.setActionHandler) {
        const orig = ms.setActionHandler.bind(ms);
        ms.setActionHandler = (action, handler) => {
          if (handler) handlers.set(action, handler);
          else handlers.delete(action);
          scheduleDispatch();
          return orig(action, handler);
        };
      }
    } catch {
    }
    function faviconUrl() {
      const links = Array.from(
        document.querySelectorAll('link[rel~="icon"]')
      );
      let best;
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
    function metaContent(sel) {
      const el = document.querySelector(sel);
      const v = el?.content?.trim();
      return v || void 0;
    }
    function collectGeneric(keepalive = false) {
      const media = activeMedia;
      const durSec = media?.duration ?? NaN;
      const isNoise = media ? isNotificationSound(durSec) : false;
      const usableMedia = media && !isNoise ? media : null;
      let title;
      let artist = [];
      let album;
      let artUrl;
      const ms = navigator.mediaSession;
      const md = ms?.metadata;
      if (md) {
        title = md.title || void 0;
        artist = md.artist ? [md.artist] : [];
        album = md.album || void 0;
        artUrl = resolveArtworkUrl(pickArtwork(md.artwork));
      }
      if (!usableMedia && !md) return null;
      if (!title) title = metaContent('meta[property="og:title"]') || document.title || void 0;
      if (!artUrl) artUrl = metaContent('meta[property="og:image"]') || faviconUrl();
      if (!album) album = metaContent('meta[property="og:site_name"]');
      if (!hasPublishableIdentity(title, artist)) return null;
      const playback = usableMedia ? {
        status: usableMedia.paused ? "paused" : usableMedia.ended ? "stopped" : "playing",
        position_ms: Math.floor((usableMedia.currentTime || 0) * 1e3),
        duration_ms: Number.isFinite(usableMedia.duration) ? Math.floor(usableMedia.duration * 1e3) : 0
      } : { status: "playing", position_ms: 0, duration_ms: 0 };
      const seekable = !!usableMedia && Number.isFinite(usableMedia.duration);
      const capabilities = {
        play_pause: !!usableMedia,
        next: handlers.has("nexttrack"),
        previous: handlers.has("previoustrack"),
        seek: seekable,
        set_position: seekable
      };
      return {
        type: "media-state",
        metadata: { title, artist, album, album_artist: [], art_url: artUrl },
        playback,
        capabilities,
        keepalive
      };
    }
    let lastGenericId = "";
    function scheduleDispatch(force = false) {
      const state = collectGeneric(force);
      if (!state) return;
      const id = JSON.stringify({
        t: state.metadata.title,
        a: state.metadata.artist,
        l: state.metadata.album,
        u: state.metadata.art_url,
        s: state.playback.status,
        p: Math.floor(state.playback.position_ms / 1e3),
        c: state.capabilities
      });
      if (!force && id === lastGenericId) return;
      lastGenericId = id;
      dispatch(state);
    }
    const onTimeupdate = /* @__PURE__ */ (() => {
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
    setInterval(() => scheduleDispatch(), 1e3);
    setInterval(() => scheduleDispatch(true), 3e4);
    window.addEventListener("mprisence-command", ((e) => {
      const { command, position_ms } = e.detail || {};
      const m = activeMedia;
      switch (command) {
        case "play_pause":
          if (m) m.paused ? m.play().catch(() => {
          }) : m.pause();
          break;
        case "play":
          if (m?.paused) m.play().catch(() => {
          });
          break;
        case "pause":
          if (m && !m.paused) m.pause();
          break;
        case "set_position":
          if (m && typeof position_ms === "number" && Number.isFinite(position_ms)) {
            m.currentTime = Math.max(0, position_ms / 1e3);
          }
          break;
        case "next":
          handlers.get("nexttrack")?.({ action: "nexttrack" });
          break;
        case "previous":
          handlers.get("previoustrack")?.({ action: "previoustrack" });
          break;
      }
      scheduleDispatch(true);
    }));
    scheduleDispatch(true);
  }
})();
//# sourceMappingURL=page-world.js.map
