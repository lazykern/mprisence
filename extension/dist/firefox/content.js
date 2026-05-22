// src/utils/browser-detect.ts
function detectBrowser() {
  const ua = navigator.userAgent.toLowerCase();
  if (ua.includes("firefox")) return "firefox";
  if (ua.includes("edg")) return "edge";
  if (ua.includes("vivaldi")) return "vivaldi";
  if (ua.includes("brave")) return "brave";
  if (ua.includes("chrome")) return "chromium";
  console.warn("[mprisence] Unknown browser, assuming chromium");
  return "chromium";
}
function makeSourceId(browser2, tabId2, frameId) {
  return `${browser2}:tab:${tabId2 ?? 0}:${frameId ?? 0}`;
}

// src/providers/base.ts
var GenericMediaProvider = class {
  matches(_url) {
    return true;
  }
  extract() {
    const video = document.querySelector("video");
    const audio = document.querySelector("audio");
    const media = video ?? audio;
    if (!media) return null;
    const dur = media.duration;
    if (!dur || !isFinite(dur)) return null;
    const meta = {
      title: document.title || void 0,
      artist: []
    };
    const playback = {
      status: media.paused ? "paused" : media.ended ? "stopped" : "playing",
      position_ms: Math.floor(media.currentTime * 1e3),
      duration_ms: Math.floor(dur * 1e3),
      rate: media.playbackRate
    };
    const caps = {
      play_pause: true,
      next: false,
      previous: false,
      seek: true,
      set_position: true,
      raise: false
    };
    if ("mediaSession" in navigator) {
      const ms = navigator.mediaSession;
      if (ms?.metadata) {
        const md = ms.metadata;
        if (md.title) meta.title = md.title;
        if (md.artist) meta.artist = [md.artist];
        if (md.album) meta.album = md.album;
        if (md.artwork?.length > 0) {
          const best = md.artwork.reduce(
            (a, b) => (a.sizes ?? 0) > (b.sizes ?? 0) ? a : b
          );
          meta.art_url = best.src || void 0;
        }
      }
    }
    return {
      metadata: meta,
      playback,
      capabilities: caps,
      confidence: "dom"
    };
  }
  async command(cmd, positionMs) {
    const video = document.querySelector("video");
    const audio = document.querySelector("audio");
    const media = video ?? audio;
    if (!media) return;
    switch (cmd) {
      case "play_pause":
        if (media.paused) await media.play().catch(() => void 0);
        else media.pause();
        break;
      case "play":
        if (media.paused) await media.play().catch(() => void 0);
        break;
      case "pause":
        if (!media.paused) media.pause();
        break;
      case "set_position":
        if (typeof positionMs === "number" && isFinite(positionMs)) {
          media.currentTime = Math.max(0, positionMs / 1e3);
        }
        break;
      case "seek":
        break;
    }
  }
};

// src/providers/youtube-music.ts
var YouTubeMusicProvider = class {
  siteKey = "youtube_music";
  origin = "https://music.youtube.com";
  videoIdRegex = /\/vi\/([a-zA-Z0-9_-]+)\//;
  matches(url) {
    return url.origin === this.origin;
  }
  extract() {
    const titleEl = this.qs(".title.ytmusic-player-bar");
    const artistEl = this.qs(".byline.ytmusic-player-bar");
    const artImg = this.qs(
      "ytmusic-player-bar img.image, ytmusic-player-bar img"
    );
    const playBtn = this.qs("#play-pause-button");
    const video = this.qs("video");
    if (!titleEl && !video) return null;
    const title = titleEl?.textContent?.trim() || document.title.replace(" - YouTube Music", "").trim() || void 0;
    const byline = artistEl?.textContent?.trim() || "";
    const artist = byline.split("\u2022")[0]?.trim() || "";
    let artUrl = artImg?.src || void 0;
    if (artUrl && artUrl.startsWith("data:")) artUrl = void 0;
    if (artUrl) {
      if (artUrl.includes("yt3.googleusercontent.com")) {
        artUrl = artUrl.replace(/=[a-z0-9-]+$/, "");
      } else {
        artUrl = artUrl.replace(/\/[a-z]+default\./g, "/maxresdefault.");
      }
    }
    const thumbSrc = artImg?.src || "";
    const videoIdMatch = thumbSrc.match(this.videoIdRegex);
    const videoId = videoIdMatch?.[1] || "";
    const trackId = videoId ? `ytm:${videoId}` : void 0;
    const isPaused = video?.paused ?? true;
    const progressBar = this.qs("#progress-bar");
    const progressNow = progressBar ? parseFloat(progressBar.getAttribute("aria-valuenow") ?? "") : NaN;
    const progressMax = progressBar ? parseFloat(progressBar.getAttribute("aria-valuemax") ?? "") : NaN;
    const trackPositionSec = isFinite(progressNow) && progressNow >= 0 ? progressNow : void 0;
    const trackDurationSec = isFinite(progressMax) && progressMax > 0 ? progressMax : void 0;
    if (video && (trackPositionSec === void 0 || trackDurationSec === void 0) && video.duration > 600) {
      return null;
    }
    const currentSec = trackPositionSec ?? (video?.currentTime || 0);
    const totalSec = trackDurationSec ?? (video?.duration || 0);
    if (video && (totalSec === 0 || !isFinite(totalSec))) {
      return null;
    }
    const isPlaying = playBtn ? playBtn.getAttribute("title")?.toLowerCase().includes("pause") : !isPaused;
    const status = isPlaying ? "playing" : "paused";
    const metadata = {
      title,
      artist: artist ? [artist] : [],
      album: void 0,
      // YTM byline has no album info
      album_artist: [],
      art_url: artUrl,
      track_id: trackId
    };
    const playback = {
      status,
      position_ms: Math.floor(currentSec * 1e3),
      duration_ms: Math.floor(totalSec * 1e3),
      rate: video?.playbackRate ?? 1
    };
    const capabilities = {
      play_pause: true,
      next: true,
      previous: true,
      seek: true,
      set_position: true,
      raise: true
    };
    return {
      metadata,
      playback,
      capabilities,
      confidence: "provider",
      canonicalUrl: videoId ? `https://music.youtube.com/watch?v=${videoId}` : void 0
    };
  }
  async command(cmd, positionMs) {
    if (cmd === "set_position") {
      const video = this.qs("video");
      if (video && typeof positionMs === "number" && isFinite(positionMs)) {
        video.currentTime = Math.max(0, positionMs / 1e3);
      }
      return;
    }
    if (cmd === "play" || cmd === "pause") {
      const video = this.qs("video");
      if (cmd === "play" && !video?.paused) return;
      if (cmd === "pause" && video?.paused) return;
    }
    const btnMap = {
      play_pause: "#play-pause-button",
      play: "#play-pause-button",
      pause: "#play-pause-button",
      next: "yt-icon-button.next-button button",
      previous: "yt-icon-button.previous-button button"
    };
    const selector = btnMap[cmd];
    if (selector) {
      const btn = document.querySelector(selector);
      btn?.click();
    }
  }
  qs(selector) {
    return document.querySelector(selector);
  }
};

// src/providers/youtube.ts
var YouTubeProvider = class {
  siteKey = "youtube";
  origin = "https://www.youtube.com";
  videoIdRe = /\/vi\/([a-zA-Z0-9_-]+)\//;
  matches(url) {
    return url.origin === this.origin;
  }
  extract() {
    const mainPlayer = document.querySelector("#movie_player");
    const video = mainPlayer?.querySelector("video");
    if (!video || !mainPlayer) return null;
    const dur = video.duration;
    if (!dur || !isFinite(dur)) return null;
    const ct = video.currentTime;
    const isPaused = video.paused;
    const isWatchPage = location.pathname === "/watch";
    let msTitle;
    let msArtist;
    let msArtwork;
    let videoId;
    if ("mediaSession" in navigator) {
      const ms = navigator.mediaSession;
      const md = ms?.metadata;
      if (md) {
        if (md.title) msTitle = md.title;
        if (md.artist) msArtist = md.artist;
        if (md.artwork?.length > 0) {
          const best = md.artwork.reduce(
            (a, b) => {
              const aSize = parseInt(a.sizes) || 0;
              const bSize = parseInt(b.sizes) || 0;
              return aSize > bSize ? a : b;
            }
          );
          msArtwork = best.src || void 0;
          const m = (msArtwork || "").match(this.videoIdRe);
          if (m) videoId = m[1];
        }
      }
    }
    if (!videoId && isWatchPage) {
      const urlParams = new URLSearchParams(window.location.search);
      videoId = urlParams.get("v") || void 0;
    }
    let title;
    if (isWatchPage) {
      const titleEl = document.querySelector(
        "#title h1.ytd-watch-metadata, h1.title.ytd-video-primary-info-renderer"
      );
      title = titleEl?.textContent?.trim() || void 0;
    }
    if (!title && msTitle) title = msTitle;
    if (!title) {
      const cleaned = document.title.replace(" - YouTube", "").trim();
      if (cleaned) title = cleaned;
    }
    let channelName;
    if (isWatchPage) {
      const channelEl = document.querySelector(
        "#owner #channel-name #text-container, #owner yt-formatted-string.ytd-channel-name"
      );
      channelName = (channelEl?.textContent?.trim() || "").replace(/\s*-\s*Topic$/, "") || void 0;
    }
    if (!channelName && msArtist) {
      channelName = msArtist.replace(/\s*-\s*Topic$/, "") || void 0;
    }
    let artUrl = msArtwork;
    if (!artUrl) {
      const urlParams = new URLSearchParams(window.location.search);
      const vid = urlParams.get("v") || videoId;
      if (vid) {
        artUrl = `https://i.ytimg.com/vi/${vid}/maxresdefault.jpg`;
      }
    }
    if (artUrl) {
      if (artUrl.includes("yt3.googleusercontent.com")) {
        artUrl = artUrl.replace(/=[a-z0-9-]+$/, "");
      } else if (artUrl.includes("ytimg.com")) {
        artUrl = artUrl.replace(/\/[a-z]+default\./g, "/maxresdefault.");
      }
    }
    const status = isPaused ? "paused" : "playing";
    let watchUrl;
    if (videoId) {
      watchUrl = `https://www.youtube.com/watch?v=${videoId}`;
    }
    const metadata = {
      title,
      artist: channelName ? [channelName] : [],
      album: void 0,
      album_artist: [],
      art_url: artUrl,
      track_id: videoId ? `yt:${videoId}` : void 0
    };
    const playback = {
      status,
      position_ms: Math.floor(ct * 1e3),
      duration_ms: Math.floor(dur * 1e3),
      rate: video.playbackRate || 1
    };
    const capabilities = {
      play_pause: true,
      next: false,
      previous: false,
      seek: true,
      set_position: true,
      raise: true
    };
    return {
      metadata,
      playback,
      capabilities,
      confidence: "provider",
      pageUrl: watchUrl || void 0,
      canonicalUrl: watchUrl || void 0
    };
  }
  async command(cmd, positionMs) {
    if (cmd === "play_pause" || cmd === "play" || cmd === "pause") {
      const video = document.querySelector("#movie_player video");
      if (cmd === "play" && !video?.paused) return;
      if (cmd === "pause" && video?.paused) return;
      const btn = document.querySelector(
        ".ytp-play-button"
      );
      btn?.click();
      return;
    }
    if (cmd === "set_position") {
      const video = document.querySelector("#movie_player video");
      if (video && typeof positionMs === "number" && isFinite(positionMs)) {
        video.currentTime = Math.max(0, positionMs / 1e3);
      }
      return;
    }
  }
};

// src/providers/soundcloud.ts
var SoundCloudProvider = class {
  siteKey = "soundcloud";
  matches(url) {
    return url.hostname === "soundcloud.com" || url.hostname.endsWith(".soundcloud.com");
  }
  extract() {
    const meta = {
      title: void 0,
      artist: [],
      album: void 0,
      album_artist: [],
      art_url: void 0,
      track_id: void 0
    };
    let playback = {
      status: "stopped",
      position_ms: 0,
      duration_ms: 0,
      rate: 1
    };
    let confidence = "dom";
    let pageUrl;
    let hasMs = false;
    try {
      const ms = navigator.mediaSession;
      if (ms?.metadata) {
        const md = ms.metadata;
        const hasContent = !!(md.title || md.artist || md.album);
        if (hasContent) {
          if (md.title) meta.title = md.title;
          if (md.artist) meta.artist = [md.artist];
          if (md.album) meta.album = md.album;
          if (md.artwork?.length > 0) {
            const best = md.artwork.reduce((a, b) => {
              const aSize = parseInt(a.sizes) || 0;
              const bSize = parseInt(b.sizes) || 0;
              return aSize > bSize ? a : b;
            });
            meta.art_url = resolveArtwork(best.src || void 0);
          }
          confidence = "provider";
          hasMs = true;
        }
      }
    } catch {
    }
    if (!meta.title) {
      const titleEl = document.querySelector(".soundTitle__title") ?? document.querySelector(".soundTitle__title > span");
      if (titleEl) {
        meta.title = titleEl.textContent?.trim() || void 0;
      }
    }
    if (meta.artist.length === 0) {
      const artistEl = document.querySelector(".soundTitle__username");
      if (artistEl) {
        meta.artist = [artistEl.textContent?.trim() || ""].filter(Boolean);
      }
    }
    if (!meta.art_url) {
      const artImg = document.querySelector(".soundTitle__artwork img, .soundTitleArt__artwork img");
      if (artImg?.src) {
        meta.art_url = resolveArtwork(artImg.src);
      }
    }
    const playBtn = document.querySelector(".sc-button-play");
    const isPlaying = playBtn?.classList.contains("playing") || playBtn?.getAttribute("title") === "Pause" || document.querySelector(".playControls__play.playing") !== null;
    let durationMs = 0;
    const durHidden = document.querySelector(
      ".playbackTimeline__duration .sc-visuallyhidden"
    );
    if (durHidden?.textContent) {
      durationMs = parseSoundCloudDuration(durHidden.textContent);
    }
    if (durationMs === 0) {
      const durSpan = document.querySelector(
        ".playbackTimeline__duration span[aria-hidden=true]"
      );
      if (durSpan?.textContent) {
        durationMs = parseMmSsDuration(durSpan.textContent);
      }
    }
    let positionMs = 0;
    if (durationMs > 0) {
      const progressBar = document.querySelector(
        ".playbackTimeline__progressBar"
      );
      if (progressBar) {
        const style = progressBar.getAttribute("style") || "";
        const m = style.match(/width\s*:\s*([\d.]+)%/);
        if (m) {
          positionMs = Math.floor(parseFloat(m[1]) / 100 * durationMs);
        }
      }
    }
    playback = {
      status: isPlaying ? "playing" : hasMs ? "paused" : "stopped",
      position_ms: positionMs,
      duration_ms: durationMs,
      rate: 1
    };
    const hasNext = !!document.querySelector(".playControls__next") || !!document.querySelector(".skipButton__forward");
    const hasPrev = !!document.querySelector(".playControls__prev") || !!document.querySelector(".skipButton__backward");
    const hasPlayBtn = !!document.querySelector(".sc-button-play") || !!document.querySelector(".playControls__play");
    const capabilities = {
      play_pause: hasPlayBtn,
      next: hasNext,
      previous: hasPrev,
      seek: false,
      // no audio element for seeking
      set_position: false,
      raise: true
    };
    if (!meta.title && !hasMs) return null;
    return {
      metadata: meta,
      playback,
      capabilities,
      confidence,
      pageUrl
    };
  }
  async command(cmd, _positionMs) {
    switch (cmd) {
      case "play_pause":
      case "play":
      case "pause": {
        const isPlaying = document.querySelector(".playControls__play.playing") !== null || document.querySelector(".sc-button-play.playing") !== null;
        if (cmd === "play" && isPlaying) break;
        if (cmd === "pause" && !isPlaying) break;
        const btn = document.querySelector(".playControls__play") ?? document.querySelector(".sc-button-play");
        btn?.click();
        break;
      }
      case "next": {
        const btn = document.querySelector(".playControls__next") ?? document.querySelector(".skipButton__forward");
        btn?.click();
        break;
      }
      case "previous": {
        const btn = document.querySelector(".playControls__prev") ?? document.querySelector(".skipButton__backward");
        btn?.click();
        break;
      }
      case "seek":
      case "set_position":
        break;
    }
  }
};
function resolveArtwork(url) {
  if (!url) return void 0;
  if (url.includes("sndcdn.com") || url.includes("soundcloud")) {
    url = url.replace(/-t\d+x\d+(?=\.[a-z]+)/i, "-t500x500");
    url = url.replace(/-original(?=\.[a-z]+)/i, "-t500x500");
    url = url.replace(/-crop-[a-z]+(?=\.[a-z]+)/i, "");
  }
  return url;
}
function parseSoundCloudDuration(text) {
  const m = text.match(/(\d+)\s*minutes?\s*(\d+)?\s*seconds?/i);
  if (m) {
    const mins = parseInt(m[1]) || 0;
    const secs = parseInt(m[2]) || 0;
    return (mins * 60 + secs) * 1e3;
  }
  const m2 = text.match(/(\d+)\s*minute/i);
  if (m2) {
    return parseInt(m2[1]) * 60 * 1e3;
  }
  const m3 = text.match(/(\d+)\s*second/i);
  if (m3) {
    return parseInt(m3[1]) * 1e3;
  }
  return 0;
}
function parseMmSsDuration(text) {
  text = text.trim();
  const parts = text.split(":");
  if (parts.length === 2) {
    const mins = parseInt(parts[0]) || 0;
    const secs = parseInt(parts[1]) || 0;
    return (mins * 60 + secs) * 1e3;
  }
  if (parts.length === 3) {
    const hours = parseInt(parts[0]) || 0;
    const mins = parseInt(parts[1]) || 0;
    const secs = parseInt(parts[2]) || 0;
    return (hours * 3600 + mins * 60 + secs) * 1e3;
  }
  return 0;
}

// src/content.ts
var providers = [
  new YouTubeMusicProvider(),
  new YouTubeProvider(),
  new SoundCloudProvider(),
  new GenericMediaProvider()
];
var lastSourceId = "";
var lastTitle = "";
var lastArtist = "";
var lastState = "";
var lastArtUrl = "";
var lastPageUrl = "";
var lastCanonicalUrl = "";
var lastPositionSec = -1;
var lastDurationMs = -1;
var lastAlbum = "";
var lastAlbumArtist = "";
var lastTrackId = "";
var lastRate = 1;
var lastConfidence = "";
var browser = detectBrowser();
var tabId = getTabId();
var sourceIdBase = makeSourceId(browser, tabId, 0);
function getTabId() {
  try {
    if (typeof chrome !== "undefined" && chrome.devtools) {
      return void 0;
    }
  } catch {
  }
  return void 0;
}
window.addEventListener("mprisence-media-state", ((event) => {
  const data = event.detail;
  if (data?.type === "media-state") {
    const result = {
      metadata: data.metadata || { artist: [] },
      playback: data.playback || { status: "stopped", position_ms: 0, duration_ms: 0, rate: 1 },
      capabilities: data.capabilities || { play_pause: true, next: false, previous: false, seek: false, set_position: false, raise: false },
      confidence: data.confidence || "dom"
    };
    const pwTitle = result.metadata.title ?? "";
    const pwArtist = result.metadata.artist.join(",");
    const isNewTrack = lastPageWorldMeta !== null && (pwTitle !== lastPageWorldMeta.title || pwArtist !== lastPageWorldMeta.artist);
    if (!isNewTrack) {
      if (lastPageWorldMeta && !result.metadata.track_id) {
        result.metadata.track_id = lastPageWorldMeta.track_id;
      }
      if (!result.canonicalUrl) {
        result.canonicalUrl = lastCanonicalUrlPageWorld;
      }
    }
    sendUpdate(result);
    lastPageWorldMeta = {
      title: result.metadata.title ?? "",
      artist: result.metadata.artist.join(","),
      album: result.metadata.album ?? "",
      art_url: result.metadata.art_url ?? "",
      track_id: result.metadata.track_id
    };
    if (result.canonicalUrl) {
      lastCanonicalUrlPageWorld = result.canonicalUrl;
    }
  }
}));
var lastPageWorldMeta = null;
var lastCanonicalUrlPageWorld = "";
function extractFromProviders() {
  const url = new URL(window.location.href);
  for (const provider of providers) {
    if (provider.matches(url)) {
      const result = provider.extract();
      if (result) return result;
    }
  }
  return null;
}
function triggerUpdate(force = false) {
  const result = extractFromProviders();
  if (result) sendUpdate(result, force);
}
var MEDIA_EVENTS = [
  "play",
  "pause",
  "ended",
  "ratechange",
  "seeked",
  "loadedmetadata",
  "durationchange"
];
var lastTimeupdate = 0;
function onTimeupdate() {
  const now = Date.now();
  if (now - lastTimeupdate < 900) return;
  lastTimeupdate = now;
  triggerUpdate();
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
var keepaliveInterval = null;
function startObserving() {
  for (const ev of MEDIA_EVENTS) {
    document.addEventListener(ev, () => triggerUpdate(), true);
  }
  document.addEventListener("timeupdate", onTimeupdate, true);
  const onMutation = debounce(() => triggerUpdate(), 500);
  const observer = new MutationObserver(() => onMutation());
  observer.observe(document.documentElement, { childList: true, subtree: true });
  keepaliveInterval = setInterval(() => triggerUpdate(true), 3e4);
}
function sendUpdate(result, force = false) {
  const sourceId = `${sourceIdBase}:frame`;
  const titleKey = result.metadata.title ?? "";
  const artistKey = result.metadata.artist.join(",");
  const identityChanged = lastTitle !== titleKey || lastArtist !== artistKey;
  const url = result.pageUrl || !identityChanged && lastPageUrl || window.location.href;
  const canonicalUrl = result.canonicalUrl || lastCanonicalUrl;
  const origin = window.location.origin;
  const positionSec = Math.floor(result.playback.position_ms / 1e3);
  const albumKey = result.metadata.album ?? "";
  const albumArtistKey = result.metadata.album_artist.join(",");
  const trackIdKey = result.metadata.track_id ?? "";
  const unchanged = lastSourceId === sourceId && lastTitle === titleKey && lastArtist === artistKey && lastState === result.playback.status && lastArtUrl === (result.metadata.art_url ?? "") && lastPageUrl === url && lastCanonicalUrl === (canonicalUrl ?? "") && lastPositionSec === positionSec && lastDurationMs === result.playback.duration_ms && lastAlbum === albumKey && lastAlbumArtist === albumArtistKey && lastTrackId === trackIdKey && lastRate === result.playback.rate && lastConfidence === result.confidence;
  if (!force && unchanged) {
    return;
  }
  lastSourceId = sourceId;
  lastTitle = titleKey;
  lastArtist = artistKey;
  lastState = result.playback.status;
  lastArtUrl = result.metadata.art_url ?? "";
  lastPageUrl = url;
  lastCanonicalUrl = canonicalUrl ?? lastCanonicalUrl;
  lastPositionSec = positionSec;
  lastDurationMs = result.playback.duration_ms;
  lastAlbum = albumKey;
  lastAlbumArtist = albumArtistKey;
  lastTrackId = trackIdKey;
  lastRate = result.playback.rate;
  lastConfidence = result.confidence;
  const urlObj = new URL(url);
  const provider = providers.find((p) => p.matches(urlObj));
  const site = provider?.siteKey ?? provider?.constructor.name.replace("Provider", "").replace(/([A-Z])/g, "_$1").toLowerCase().replace(/^_/, "").replace(/^generic$/, "generic") ?? "generic";
  const msg = {
    type: "update",
    source_id: sourceId,
    url,
    origin,
    site,
    playback: result.playback,
    metadata: result.metadata,
    capabilities: result.capabilities,
    confidence: result.confidence,
    canonical_url: canonicalUrl || void 0,
    _ext_fingerprint: true ? "e3520d1-dirty" : void 0
  };
  chrome.runtime.sendMessage(msg).catch(() => {
  });
}
chrome.runtime.onMessage.addListener(
  (msg, _sender, sendResponse) => {
    if (msg?.type === "command" && msg?.command) {
      const url = new URL(window.location.href);
      for (const provider of providers) {
        if (provider.matches(url)) {
          provider.command(msg.command, msg.position_ms).then(() => sendResponse({ ok: true }));
          return true;
        }
      }
      sendResponse({ ok: false, error: "no matching provider" });
    }
    return true;
  }
);
startObserving();
triggerUpdate();
window.addEventListener("beforeunload", () => {
  if (keepaliveInterval) clearInterval(keepaliveInterval);
  const msg = {
    type: "remove",
    source_id: `${sourceIdBase}:frame`
  };
  chrome.runtime.sendMessage(msg).catch(() => {
  });
});
//# sourceMappingURL=content.js.map
