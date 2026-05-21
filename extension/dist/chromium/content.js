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
    const meta = {
      title: document.title || void 0,
      artist: []
    };
    const playback = {
      status: media.paused ? "paused" : media.ended ? "stopped" : "playing",
      position_ms: Math.floor(media.currentTime * 1e3),
      duration_ms: Math.floor((media.duration || 0) * 1e3),
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
  async command(_cmd) {
  }
};

// src/providers/youtube-music.ts
var YouTubeMusicProvider = class {
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
      artUrl = artUrl.replace("/sddefault.", "/hqdefault.");
    }
    const thumbSrc = artImg?.src || "";
    const videoIdMatch = thumbSrc.match(this.videoIdRegex);
    const videoId = videoIdMatch?.[1] || "";
    const trackId = videoId ? `ytm:${videoId}` : void 0;
    const currentSec = video?.currentTime || 0;
    const totalSec = video?.duration || 0;
    const isPaused = video?.paused ?? true;
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
      confidence: "provider"
    };
  }
  async command(cmd) {
    const btnMap = {
      play_pause: "#play-pause-button",
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
  /** Convert sddefault → hqdefault for better quality art */
  static upgradeArtQuality(url) {
    return url.replace("/sddefault.", "/hqdefault.");
  }
};

// src/content.ts
var providers = [
  new YouTubeMusicProvider(),
  new GenericMediaProvider()
];
var lastSourceId = "";
var lastTitle = "";
var lastArtist = "";
var lastState = "";
var lastArtUrl = "";
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
function injectPageWorldScript() {
  const script = document.createElement("script");
  script.src = chrome.runtime.getURL("page-world.js");
  script.onload = () => script.remove();
  (document.head || document.documentElement).appendChild(script);
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
    sendUpdate(result);
  }
}));
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
var pollInterval = null;
function startPolling() {
  if (pollInterval) return;
  pollInterval = setInterval(() => {
    const result = extractFromProviders();
    if (result) {
      sendUpdate(result);
    }
  }, 1e3);
}
function stopPolling() {
  if (pollInterval) {
    clearInterval(pollInterval);
    pollInterval = null;
  }
}
function sendUpdate(result) {
  const sourceId = `${sourceIdBase}:frame`;
  const url = window.location.href;
  const origin = window.location.origin;
  const titleKey = result.metadata.title ?? "";
  const artistKey = result.metadata.artist.join(",");
  if (lastSourceId === sourceId && lastTitle === titleKey && lastArtist === artistKey && lastState === result.playback.status && lastArtUrl === (result.metadata.art_url ?? "")) {
    return;
  }
  lastSourceId = sourceId;
  lastTitle = titleKey;
  lastArtist = artistKey;
  lastState = result.playback.status;
  lastArtUrl = result.metadata.art_url ?? "";
  const urlObj = new URL(url);
  const site = providers.find((p) => p.matches(urlObj))?.constructor.name.replace("Provider", "").replace(/([A-Z])/g, "_$1").toLowerCase().replace(/^_/, "").replace(/^generic$/, "generic") ?? "generic";
  const msg = {
    type: "update",
    source_id: sourceId,
    url,
    origin,
    site,
    playback: result.playback,
    metadata: result.metadata,
    capabilities: result.capabilities,
    confidence: result.confidence
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
          provider.command(msg.command).then(() => sendResponse({ ok: true }));
          return true;
        }
      }
      sendResponse({ ok: false, error: "no matching provider" });
    }
    return true;
  }
);
startPolling();
injectPageWorldScript();
window.addEventListener("beforeunload", () => {
  stopPolling();
  const msg = {
    type: "remove",
    source_id: `${sourceIdBase}:frame`
  };
  chrome.runtime.sendMessage(msg).catch(() => {
  });
});
//# sourceMappingURL=content.js.map
