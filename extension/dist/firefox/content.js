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

// src/providers/youtube-music.ts
var YouTubeMusicProvider = class {
  siteKey = "youtube_music";
  origin = "https://music.youtube.com";
  videoIdRegex = /\/vi\/([a-zA-Z0-9_-]+)\//;
  stablePlayback = null;
  matches(url) {
    return url.origin === this.origin;
  }
  extract() {
    if (document.querySelector(".ad-showing")) return null;
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
    const parts = byline.split("\u2022").map((s) => s.trim()).filter(Boolean);
    const artist = parts[0] || "";
    let album = void 0;
    if (parts.length >= 3) {
      const mid = parts[1];
      if (mid && !/\b(view|like)s?\b/i.test(mid)) {
        album = mid;
      }
    }
    const thumbSrc = artImg?.src || "";
    let videoId = (thumbSrc.match(this.videoIdRegex) || [])[1] || "";
    if (!videoId) {
      videoId = new URLSearchParams(window.location.search).get("v") || "";
    }
    const trackId = videoId ? `ytm:${videoId}` : void 0;
    let artUrl = artImg?.src || void 0;
    if (artUrl && artUrl.startsWith("data:")) artUrl = void 0;
    if (artUrl) {
      if (artUrl.includes("yt3.googleusercontent.com")) {
        if (videoId) {
          artUrl = `https://i.ytimg.com/vi/${videoId}/maxresdefault.jpg`;
        } else {
          artUrl = artUrl.replace(/=[a-z0-9-]+$/, "");
        }
      } else {
        artUrl = artUrl.replace(/\/[a-z]+default\./g, "/maxresdefault.");
      }
    } else if (videoId) {
      artUrl = `https://i.ytimg.com/vi/${videoId}/maxresdefault.jpg`;
    }
    const isPaused = video?.paused ?? true;
    const progressBar = this.qs("#progress-bar");
    const progressNow = progressBar ? parseFloat(progressBar.getAttribute("aria-valuenow") ?? "") : NaN;
    const progressMax = progressBar ? parseFloat(progressBar.getAttribute("aria-valuemax") ?? "") : NaN;
    const trackPositionSec = isFinite(progressNow) && progressNow >= 0 ? progressNow : void 0;
    const trackDurationSec = isFinite(progressMax) && progressMax > 0 ? progressMax : void 0;
    if (video && (trackPositionSec === void 0 || trackDurationSec === void 0) && video.duration > 600) {
      return null;
    }
    let currentSec = trackPositionSec ?? (video?.currentTime || 0);
    let totalSec = trackDurationSec ?? (video?.duration || 0);
    ({ positionSec: currentSec, durationSec: totalSec } = this.stabilizePlayback(
      trackId,
      currentSec,
      totalSec
    ));
    if (video && (totalSec === 0 || !isFinite(totalSec))) {
      return null;
    }
    const isPlaying = playBtn ? playBtn.getAttribute("title")?.toLowerCase().includes("pause") : !isPaused;
    const status = isPlaying ? "playing" : "paused";
    const metadata = {
      title,
      artist: artist ? [artist] : [],
      album,
      // extracted from byline when present
      album_artist: [],
      art_url: artUrl,
      track_id: trackId
    };
    const playback = {
      status,
      position_ms: Math.floor(currentSec * 1e3),
      duration_ms: Math.floor(totalSec * 1e3)
    };
    const capabilities = {
      play_pause: true,
      next: true,
      previous: true,
      seek: true,
      set_position: true
    };
    return {
      metadata,
      playback,
      capabilities,
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
  stabilizePlayback(trackId, positionSec, durationSec) {
    if (!trackId || durationSec <= 0 || !isFinite(durationSec)) {
      return { positionSec, durationSec };
    }
    const prev = this.stablePlayback;
    if (!prev || prev.trackId !== trackId) {
      this.stablePlayback = { trackId, positionSec, durationSec };
      return { positionSec, durationSec };
    }
    let pos = positionSec;
    let dur = durationSec;
    const durDiff = Math.abs(dur - prev.durationSec);
    if (prev.durationSec > 0 && durDiff > 10 && durDiff / prev.durationSec > 0.15) {
      dur = prev.durationSec;
    }
    if (prev.positionSec > 5 && pos + 3 < prev.positionSec) {
      pos = prev.positionSec;
    }
    if (prev.positionSec > 30 && pos === 0) {
      pos = prev.positionSec;
    }
    if (dur > 0 && pos > dur) {
      pos = Math.min(prev.positionSec, dur);
    }
    this.stablePlayback = { trackId, positionSec: pos, durationSec: dur };
    return { positionSec: pos, durationSec: dur };
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
    if (document.querySelector(".ad-showing")) return null;
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
        artUrl = `https://i.ytimg.com/vi/${vid}/hqdefault.jpg`;
      }
    }
    if (artUrl) {
      if (artUrl.includes("yt3.googleusercontent.com")) {
        artUrl = artUrl.replace(/=[a-z0-9-]+$/, "");
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
      duration_ms: Math.floor(dur * 1e3)
    };
    const capabilities = {
      play_pause: true,
      next: false,
      previous: false,
      seek: true,
      set_position: true
    };
    return {
      metadata,
      playback,
      capabilities,
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
      duration_ms: 0
    };
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
      duration_ms: durationMs
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
      set_position: false
    };
    if (!meta.title && !hasMs) return null;
    return {
      metadata: meta,
      playback,
      capabilities,
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

// src/providers/bandcamp.ts
var BandcampProvider = class {
  siteKey = "bandcamp";
  trackIdParam = /track_id=(\d+)/;
  matches(url) {
    const host = url.hostname;
    return host === "bandcamp.com" || host.endsWith(".bandcamp.com");
  }
  extract() {
    const carouselResult = this.extractCarousel();
    if (carouselResult) return carouselResult;
    const inlineResult = this.extractInline();
    if (inlineResult) return inlineResult;
    return null;
  }
  async command(cmd, positionMs) {
    if (document.querySelector(".carousel-player.show")) {
      await this.commandCarousel(cmd, positionMs);
      return;
    }
    if (document.querySelector(".inline_player")) {
      await this.commandInline(cmd, positionMs);
      return;
    }
  }
  // ── Carousel player (collection pages) ────────────────────────
  extractCarousel() {
    const player = this.qs(".carousel-player.show");
    if (!player) return null;
    const trackTitleEl = this.qs(
      ".carousel-player .info-progress .info .title span:last-child"
    );
    const trackTitle = trackTitleEl?.textContent?.trim() || "";
    const albumTitle = this.qsText(
      ".carousel-player .now-playing .title"
    );
    const title = trackTitle || albumTitle;
    if (!title) return null;
    const meta = {
      title,
      artist: [],
      album: void 0,
      album_artist: [],
      art_url: void 0,
      track_id: void 0
    };
    if (albumTitle && albumTitle !== title) {
      meta.album = albumTitle;
    }
    const artistRaw = this.qsText(
      ".carousel-player .now-playing .artist"
    );
    if (artistRaw) {
      const cleaned = artistRaw.replace(/^by\s+/i, "").trim();
      if (cleaned) meta.artist = [cleaned];
    }
    meta.art_url = this.resolveArtwork(
      this.qs(".carousel-player .now-playing img")?.src
    );
    const audio = document.querySelector("audio");
    const srcMatch = audio?.getAttribute("src")?.match(this.trackIdParam);
    if (srcMatch) meta.track_id = `bc:${srcMatch[1]}`;
    const isPlaying = audio ? !audio.paused : false;
    let positionMs = 0;
    let durationMs = 0;
    const posDur = this.qs(".carousel-player .pos-dur");
    if (posDur) {
      const spans = posDur.querySelectorAll("span");
      if (spans.length >= 2) {
        positionMs = parseTimeSpan(spans[0]?.textContent);
        durationMs = parseTimeSpan(spans[1]?.textContent);
      }
    }
    if (durationMs === 0 && audio && isFinite(audio.duration) && audio.duration > 0) {
      durationMs = Math.floor(audio.duration * 1e3);
      positionMs = Math.floor((audio.currentTime || 0) * 1e3);
    }
    if (durationMs === 0) return null;
    const playback = {
      status: isPlaying ? "playing" : "paused",
      position_ms: positionMs,
      duration_ms: durationMs
    };
    const prevIcon = this.qs(
      ".carousel-player .prev-icon"
    );
    const nextIcon = this.qs(
      ".carousel-player .next-icon"
    );
    const capabilities = {
      play_pause: true,
      next: nextIcon ? !nextIcon.classList.contains("disabled") : false,
      previous: prevIcon ? !prevIcon.classList.contains("disabled") : false,
      seek: true,
      set_position: true
    };
    return { metadata: meta, playback, capabilities };
  }
  async commandCarousel(cmd, positionMs) {
    switch (cmd) {
      case "play_pause":
      case "play":
      case "pause": {
        const a = document.querySelector("audio");
        if (!a) break;
        const isPlaying = !a.paused;
        if (cmd === "play" && isPlaying) break;
        if (cmd === "pause" && !isPlaying) break;
        if (isPlaying) {
          a.pause();
        } else {
          a.play().catch(() => {
          });
        }
        break;
      }
      case "next": {
        const btn = this.qs(
          ".carousel-player .next .next-icon"
        );
        if (btn && !btn.classList.contains("disabled")) btn.click();
        break;
      }
      case "previous": {
        const btn = this.qs(
          ".carousel-player .prev .prev-icon"
        );
        if (btn && !btn.classList.contains("disabled")) btn.click();
        break;
      }
      case "set_position":
      case "seek": {
        if (typeof positionMs === "number" && isFinite(positionMs)) {
          const a = document.querySelector("audio");
          if (a) a.currentTime = Math.max(0, positionMs / 1e3);
        }
        break;
      }
    }
  }
  // ── Inline player (album/track pages) ─────────────────────────
  extractInline() {
    const player = this.qs(".inline_player");
    if (!player) return null;
    const audio = document.querySelector("audio");
    if (!audio || !isFinite(audio.duration) || audio.duration <= 0) return null;
    const meta = {
      title: void 0,
      artist: [],
      album: void 0,
      album_artist: [],
      art_url: void 0,
      track_id: void 0
    };
    meta.title = this.qsText(".inline_player .title_link .title") || this.qsText(".inline_player .title") || void 0;
    if (!meta.title) return null;
    const pageArtist = this.qsText("#name-section h3 a") || this.qsText("#name-section span") || this.qsText('.detail_item a[href*="/music"]');
    if (pageArtist) meta.artist = [pageArtist.trim()];
    meta.album = this.qsText("h2.trackTitle") || this.qsText(".trackTitle") || void 0;
    meta.art_url = this.resolveArtwork(
      document.querySelector(
        "#tralbumArt img, a.popupImage img"
      )?.src
    );
    const srcMatch = audio.getAttribute("src")?.match(this.trackIdParam);
    if (srcMatch) meta.track_id = `bc:${srcMatch[1]}`;
    const isPlaying = !audio.paused;
    let positionMs = Math.floor((audio.currentTime || 0) * 1e3);
    let durationMs = Math.floor(audio.duration * 1e3);
    if (positionMs === 0 && durationMs > 0) {
      const elapsed = this.qsText(".inline_player .time_elapsed");
      if (elapsed) positionMs = parseTimeSpan(elapsed);
    }
    const playback = {
      status: isPlaying ? "playing" : "paused",
      position_ms: positionMs,
      duration_ms: durationMs
    };
    const capabilities = {
      play_pause: true,
      next: !!this.qs(".inline_player .nextbutton"),
      previous: !!this.qs(".inline_player .prevbutton"),
      seek: true,
      set_position: true
    };
    return { metadata: meta, playback, capabilities };
  }
  async commandInline(cmd, positionMs) {
    switch (cmd) {
      case "play_pause":
      case "play":
      case "pause": {
        const a = document.querySelector("audio");
        if (!a) break;
        const isPlaying = !a.paused;
        if (cmd === "play" && isPlaying) break;
        if (cmd === "pause" && !isPlaying) break;
        if (isPlaying) {
          a.pause();
        } else {
          a.play().catch(() => {
          });
        }
        break;
      }
      case "next": {
        const btn = this.qs(".inline_player .nextbutton");
        btn?.click();
        break;
      }
      case "previous": {
        const btn = this.qs(".inline_player .prevbutton");
        btn?.click();
        break;
      }
      case "set_position":
      case "seek": {
        if (typeof positionMs === "number" && isFinite(positionMs)) {
          const a = document.querySelector("audio");
          if (a) a.currentTime = Math.max(0, positionMs / 1e3);
        }
        break;
      }
    }
  }
  // ── Helpers ──────────────────────────────────────────────────
  qs(selector) {
    return document.querySelector(selector);
  }
  qsText(selector) {
    return document.querySelector(selector)?.textContent?.trim() || "";
  }
  /**
   * Upgrade Bandcamp artwork to highest available resolution.
   * _16 = 700×700, _10 = 1200×1200, _2 = 350×350
   */
  resolveArtwork(url) {
    if (!url) return void 0;
    if (url.includes("bcbits.com")) {
      url = url.replace(/_16\./, "_10.");
      url = url.replace(/_2\./, "_10.");
    }
    return url;
  }
};
function parseTimeSpan(text) {
  if (!text) return 0;
  const parts = text.trim().split(":");
  if (parts.length === 2) {
    const mins = parseInt(parts[0], 10) || 0;
    const secs = parseInt(parts[1], 10) || 0;
    return (mins * 60 + secs) * 1e3;
  }
  if (parts.length === 3) {
    const hrs = parseInt(parts[0], 10) || 0;
    const mins = parseInt(parts[1], 10) || 0;
    const secs = parseInt(parts[2], 10) || 0;
    return (hrs * 3600 + mins * 60 + secs) * 1e3;
  }
  return 0;
}

// src/providers/tidal.ts
var TidalProvider = class {
  siteKey = "tidal";
  matches(url) {
    return url.hostname === "tidal.com" || url.hostname.endsWith(".tidal.com");
  }
  extract() {
    const video = document.querySelector("video");
    if (!video) return null;
    if (!isFinite(video.duration) || video.duration <= 0) return null;
    const meta = {
      title: void 0,
      artist: [],
      album: void 0,
      album_artist: [],
      art_url: void 0,
      track_id: void 0
    };
    try {
      const ms = navigator.mediaSession;
      if (ms?.metadata) {
        const md = ms.metadata;
        if (md.title) meta.title = md.title;
        if (md.artist) meta.artist = [md.artist];
        if (md.album) meta.album = md.album;
        if (md.artwork?.length > 0) {
          const best = md.artwork.reduce((a, b) => {
            const aSize = parseInt(a.sizes) || 0;
            const bSize = parseInt(b.sizes) || 0;
            return aSize > bSize ? a : b;
          });
          meta.art_url = this.resolveArtwork(best.src || void 0);
        }
      }
    } catch {
    }
    if (!meta.title) {
      meta.title = document.title.replace(/ \| TIDAL$/, "").trim() || void 0;
    }
    if (!meta.title) return null;
    const isPaused = video.paused;
    const status = isPaused ? "paused" : "playing";
    let positionMs = Math.floor((video.currentTime || 0) * 1e3);
    let durationMs = Math.floor(video.duration * 1e3);
    const progressSlider = document.querySelector(
      '[role="slider"][aria-label="Progress bar"]'
    );
    if (progressSlider) {
      const now = parseFloat(progressSlider.getAttribute("aria-valuenow") ?? "");
      const max = parseFloat(progressSlider.getAttribute("aria-valuemax") ?? "");
      if (isFinite(now) && isFinite(max) && max > 0) {
        positionMs = Math.floor(now * 1e3);
        durationMs = Math.floor(max * 1e3);
      }
    }
    const playback = {
      status,
      position_ms: positionMs,
      duration_ms: durationMs
    };
    const nextBtn = document.querySelector(
      'button[aria-label="Next"]'
    );
    const prevBtn = document.querySelector(
      'button[aria-label="Previous"]'
    );
    const capabilities = {
      play_pause: true,
      next: nextBtn ? !nextBtn.disabled : false,
      previous: prevBtn ? !prevBtn.disabled : false,
      seek: !!progressSlider,
      set_position: !!progressSlider
    };
    return {
      metadata: meta,
      playback,
      capabilities
    };
  }
  async command(cmd, positionMs) {
    switch (cmd) {
      case "play_pause":
      case "play":
      case "pause": {
        const video = document.querySelector("video");
        if (!video) break;
        const isPlaying = !video.paused;
        if (cmd === "play" && isPlaying) break;
        if (cmd === "pause" && !isPlaying) break;
        if (isPlaying) {
          video.pause();
        } else {
          video.play().catch(() => {
          });
        }
        break;
      }
      case "next": {
        const btn = document.querySelector(
          'button[aria-label="Next"]'
        );
        if (btn && !btn.disabled) btn.click();
        break;
      }
      case "previous": {
        const btn = document.querySelector(
          'button[aria-label="Previous"]'
        );
        if (btn && !btn.disabled) btn.click();
        break;
      }
      case "set_position":
      case "seek": {
        if (typeof positionMs === "number" && isFinite(positionMs)) {
          const video = document.querySelector("video");
          if (video) {
            video.currentTime = Math.max(0, positionMs / 1e3);
          }
        }
        break;
      }
    }
  }
  // ── Helpers ──────────────────────────────────────────────────
  /**
   * Upgrade Tidal CDN artwork to higher resolution.
   * Pattern: resources.tidal.com/images/<uuid>/<size>.jpg
   * Available sizes: 80x80, 320x320, 640x640, 1080x1080, 1280x1280
   */
  resolveArtwork(url) {
    if (!url) return void 0;
    if (url.includes("resources.tidal.com")) {
      url = url.replace(/\/\d+x\d+\./g, "/1280x1280.");
    }
    return url;
  }
};

// src/providers/apple-music.ts
var AppleMusicProvider = class {
  siteKey = "apple_music";
  matches(url) {
    return url.hostname === "music.apple.com";
  }
  extract() {
    const audio = document.querySelector("audio");
    if (!audio) return null;
    if (!isFinite(audio.duration) || audio.duration <= 0) return null;
    const meta = {
      title: void 0,
      artist: [],
      album: void 0,
      album_artist: [],
      art_url: void 0,
      track_id: void 0
    };
    let pageUrl;
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
            meta.art_url = this.resolveArtwork(best.src || void 0);
          }
        }
      }
    } catch {
    }
    if (!meta.title) {
      meta.title = document.title.replace(/ — Apple Music$/, "").replace(/^(.+?) — (.+)$/, "$1").trim() || void 0;
    }
    if (meta.artist.length === 0) {
      const titleMatch = document.title.match(/^.+? — (.+?) — Apple Music$/);
      if (titleMatch) {
        meta.artist = [titleMatch[1]];
      }
    }
    const urlParams = new URLSearchParams(window.location.search);
    const trackId = urlParams.get("i");
    if (trackId) {
      meta.track_id = `am:${trackId}`;
      pageUrl = window.location.href.split("?")[0] + `?i=${trackId}`;
    }
    if (!meta.title) return null;
    const isPaused = audio.paused;
    const status = isPaused ? "paused" : "playing";
    const playback = {
      status,
      position_ms: Math.floor((audio.currentTime || 0) * 1e3),
      duration_ms: Math.floor(audio.duration * 1e3)
    };
    const playBtn = document.querySelector(
      'button[aria-label="Play"], button[data-testid="play"]'
    );
    const nextBtn = document.querySelector(
      'button[aria-label="Next"], button[data-testid="next"]'
    );
    const prevBtn = document.querySelector(
      'button[aria-label="Previous"], button[data-testid="previous"]'
    );
    const capabilities = {
      play_pause: !!playBtn || !!audio,
      next: nextBtn ? !nextBtn.disabled : false,
      previous: prevBtn ? !prevBtn.disabled : false,
      seek: true,
      set_position: true
    };
    return {
      metadata: meta,
      playback,
      capabilities,
      pageUrl
    };
  }
  async command(cmd, positionMs) {
    switch (cmd) {
      case "play_pause":
      case "play":
      case "pause": {
        const audio = document.querySelector("audio");
        if (!audio) break;
        const isPlaying = !audio.paused;
        if (cmd === "play" && isPlaying) break;
        if (cmd === "pause" && !isPlaying) break;
        if (isPlaying) {
          audio.pause();
          this.clickBtn('button[aria-label="Pause"]');
        } else {
          await audio.play().catch(() => {
          });
          this.clickBtn('button[aria-label="Play"]');
        }
        break;
      }
      case "next": {
        const btn = document.querySelector(
          'button[aria-label="Next"], button[data-testid="next"]'
        );
        if (btn && !btn.disabled) btn.click();
        break;
      }
      case "previous": {
        const btn = document.querySelector(
          'button[aria-label="Previous"], button[data-testid="previous"]'
        );
        if (btn && !btn.disabled) btn.click();
        break;
      }
      case "set_position":
      case "seek": {
        if (typeof positionMs === "number" && isFinite(positionMs)) {
          const audio = document.querySelector("audio");
          if (audio) {
            audio.currentTime = Math.max(0, positionMs / 1e3);
          }
        }
        break;
      }
    }
  }
  // ── Helpers ──────────────────────────────────────────────────
  clickBtn(selector) {
    const btn = document.querySelector(selector);
    if (btn && !btn.disabled) btn.click();
  }
  /**
   * Upgrade Apple Music artwork to higher resolution.
   *
   * Apple CDN URL pattern:
   *   https://is{1-5}-ssl.mzstatic.com/image/thumb/Music{id}/{uuid}/{name}.{w}x{h}bb.{ext}
   *
   * MediaSession typically provides small sizes (50x50, 100x100, 200x200).
   * Upgrade to 600x600bb for good quality without being too large.
   */
  resolveArtwork(url) {
    if (!url) return void 0;
    if (url.includes("mzstatic.com")) {
      url = url.replace(/\d+x\d+bb(?=\.[a-z]+)/i, "600x600bb");
    }
    return url;
  }
};

// src/content.ts
var providers = [
  new YouTubeMusicProvider(),
  new YouTubeProvider(),
  new SoundCloudProvider(),
  new BandcampProvider(),
  new TidalProvider(),
  new AppleMusicProvider()
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
var browser = detectBrowser();
var tabId = getTabId();
var sourceIdBase = makeSourceId(browser, tabId, 0);
function normalizeStringList(value) {
  if (Array.isArray(value)) {
    return value.filter((item) => typeof item === "string" && item.length > 0);
  }
  if (typeof value === "string" && value.length > 0) {
    return [value];
  }
  return [];
}
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
    const metadata = {
      ...data.metadata || {},
      artist: normalizeStringList(data?.metadata?.artist),
      album_artist: normalizeStringList(data?.metadata?.album_artist)
    };
    const result = {
      metadata,
      playback: data.playback || { status: "stopped", position_ms: 0, duration_ms: 0 },
      capabilities: data.capabilities || { play_pause: true, next: false, previous: false, seek: false, set_position: false }
    };
    const pwTitle = result.metadata.title ?? "";
    const pwArtist = result.metadata.artist.join(",");
    const pwArtUrl = result.metadata.art_url ?? "";
    const isArtOnly = !pwTitle && !pwArtist && !!pwArtUrl && lastProviderMetadata !== null;
    if (isArtOnly) {
      result.metadata = {
        ...lastProviderMetadata,
        art_url: pwArtUrl
      };
    }
    if (lastDurationMs > 0 && (result.playback.duration_ms === 0 || isArtOnly && result.playback.position_ms === 0)) {
      result.playback = {
        status: lastState || result.playback.status,
        position_ms: lastPositionSec >= 0 ? lastPositionSec * 1e3 : result.playback.position_ms,
        duration_ms: lastDurationMs
      };
    }
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
var lastProviderMetadata = null;
var extensionContextAlive = true;
function isContextInvalidatedError(err) {
  if (!(err instanceof Error)) return false;
  return /Extension context invalidated/i.test(err.message) || /context invalidated/i.test(err.message);
}
function markExtensionContextDead(err) {
  if (!isContextInvalidatedError(err)) return;
  extensionContextAlive = false;
  if (keepaliveInterval) {
    clearInterval(keepaliveInterval);
    keepaliveInterval = null;
  }
}
function safeSendMessage(msg) {
  if (!extensionContextAlive) return;
  try {
    chrome.runtime.sendMessage(msg).catch((err) => {
      markExtensionContextDead(err);
    });
  } catch (err) {
    markExtensionContextDead(err);
  }
}
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
  const artistKey = normalizeStringList(result.metadata.artist).join(",");
  const identityChanged = lastTitle !== titleKey || lastArtist !== artistKey;
  const url = result.pageUrl || !identityChanged && lastPageUrl || window.location.href;
  const canonicalUrl = result.canonicalUrl || lastCanonicalUrl;
  const origin = window.location.origin;
  const positionSec = Math.floor(result.playback.position_ms / 1e3);
  const albumKey = result.metadata.album ?? "";
  const albumArtistKey = normalizeStringList(result.metadata.album_artist).join(",");
  const trackIdKey = result.metadata.track_id ?? "";
  const unchanged = lastSourceId === sourceId && lastTitle === titleKey && lastArtist === artistKey && lastState === result.playback.status && lastArtUrl === (result.metadata.art_url ?? "") && lastPageUrl === url && lastCanonicalUrl === (canonicalUrl ?? "") && lastPositionSec === positionSec && lastDurationMs === result.playback.duration_ms && lastAlbum === albumKey && lastAlbumArtist === albumArtistKey && lastTrackId === trackIdKey;
  if (!force && unchanged) {
    return;
  }
  if (titleKey) {
    lastProviderMetadata = result.metadata;
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
    canonical_url: canonicalUrl || void 0
  };
  safeSendMessage(msg);
}
try {
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
} catch (err) {
  markExtensionContextDead(err);
}
function isSupportedPage() {
  const url = new URL(window.location.href);
  return providers.some((p) => p.matches(url));
}
if (isSupportedPage()) {
  startObserving();
  triggerUpdate();
  window.addEventListener("beforeunload", () => {
    if (keepaliveInterval) clearInterval(keepaliveInterval);
    const msg = {
      type: "remove",
      source_id: `${sourceIdBase}:frame`
    };
    safeSendMessage(msg);
  });
}
