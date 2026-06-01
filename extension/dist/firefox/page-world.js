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
    let metadata = { artist: [], album_artist: [] };
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
  let lastMeta = "";
  function metaIdentity() {
    const ms = navigator.mediaSession;
    if (!ms?.metadata) return "";
    const md = ms.metadata;
    return JSON.stringify({ t: md.title, a: md.artist, l: md.album, u: md.artwork?.[0]?.src });
  }
  function checkMetadataAndDispatch() {
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
        duration_ms: 0,
        rate: 1
      },
      capabilities: {
        play_pause: true,
        next: false,
        previous: false,
        seek: false,
        set_position: false,
        raise: true
      },
      confidence: "provider"
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
  dispatch(collectState());
  checkYtmVideoId();
})();
//# sourceMappingURL=page-world.js.map
