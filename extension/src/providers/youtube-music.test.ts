import { test } from "node:test";
import assert from "node:assert/strict";
import {
  isYouTubeMusicPlaying,
  YouTubeMusicProvider,
} from "./youtube-music.ts";

function installYouTubeMusicDom(
  elements: Record<string, unknown | unknown[]>,
  search = "?v=dQw4w9WgXcQ",
): () => void {
  const previousDocument = globalThis.document;
  const previousWindow = globalThis.window;

  Object.defineProperty(globalThis, "document", {
    configurable: true,
    value: {
      title: "Never Gonna Give You Up - YouTube Music",
      querySelector: (selector: string) => {
        const value = elements[selector];
        return Array.isArray(value) ? value[0] ?? null : value ?? null;
      },
      querySelectorAll: (selector: string) => {
        const value = elements[selector];
        return Array.isArray(value) ? value : value ? [value] : [];
      },
    },
  });
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: { location: { search } },
  });

  return () => {
    Object.defineProperty(globalThis, "document", {
      configurable: true,
      value: previousDocument,
    });
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: previousWindow,
    });
  };
}

test("uses active media state with a localized play button title", () => {
  assert.equal(
    isYouTubeMusicPlaying({ paused: false, ended: false }, "一時停止"),
    true,
  );
});

test("uses paused media state instead of a stale play button title", () => {
  assert.equal(
    isYouTubeMusicPlaying({ paused: true, ended: false }, "Pause"),
    false,
  );
});

test("treats ended media as not playing", () => {
  assert.equal(
    isYouTubeMusicPlaying({ paused: false, ended: true }, "Pause"),
    false,
  );
});

test("falls back to the play button when media is unavailable", () => {
  assert.equal(isYouTubeMusicPlaying(null, "Pause"), true);
  assert.equal(isYouTubeMusicPlaying(null, "Play"), false);
});

test("ignores the startup progress placeholder until media is ready", () => {
  const restore = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "Never Gonna Give You Up" },
    ".byline.ytmusic-player-bar": { textContent: "Rick Astley" },
    "#progress-bar": {
      getAttribute: (name: string) => name === "aria-valuemax" ? "100" : "0",
    },
  });

  try {
    assert.equal(new YouTubeMusicProvider().extract(), null);
  } finally {
    restore();
  }
});

test("ignores the startup progress placeholder even when media is ready", () => {
  const restore = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "Never Gonna Give You Up" },
    ".byline.ytmusic-player-bar": { textContent: "Rick Astley" },
    "#progress-bar": {
      getAttribute: (name: string) => name === "aria-valuemax" ? "100" : "0",
    },
    video: {
      paused: false,
      ended: false,
      readyState: 4,
      currentTime: 2,
      duration: 213.061,
    },
  });

  try {
    assert.equal(new YouTubeMusicProvider().extract(), null);
  } finally {
    restore();
  }
});

test("ignores the progress placeholder on an SPA track change", () => {
  const provider = new YouTubeMusicProvider();
  const restoreFirstTrack = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "First Track" },
    ".byline.ytmusic-player-bar": { textContent: "First Artist" },
    "#progress-bar": {
      getAttribute: (name: string) => name === "aria-valuemax" ? "300" : "10",
    },
    video: {
      paused: false,
      ended: false,
      readyState: 4,
      currentTime: 10,
      duration: 300,
    },
  }, "?v=firstTrack1");

  try {
    assert.equal(provider.extract()?.playback.duration_ms, 300_000);
  } finally {
    restoreFirstTrack();
  }

  const restoreNextTrack = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "Next Track" },
    ".byline.ytmusic-player-bar": { textContent: "Next Artist" },
    "#progress-bar": {
      getAttribute: (name: string) => name === "aria-valuemax" ? "100" : "0",
    },
    video: {
      paused: false,
      ended: false,
      readyState: 4,
      currentTime: 10,
      duration: 300,
    },
  }, "?v=nextTrack02");

  try {
    assert.equal(provider.extract(), null);
  } finally {
    restoreNextTrack();
  }
});

test("ignores the progress placeholder for a track near 100 seconds", () => {
  const restore = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "Short Track" },
    ".byline.ytmusic-player-bar": { textContent: "Short Artist" },
    "#progress-bar": {
      getAttribute: (name: string) => name === "aria-valuemax" ? "100" : "0",
    },
    video: {
      paused: false,
      ended: false,
      readyState: 4,
      currentTime: 0,
      duration: 107,
    },
  });

  try {
    assert.equal(new YouTubeMusicProvider().extract(), null);
  } finally {
    restore();
  }
});

test("publishes a real 100-second track", () => {
  const restore = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "Exactly Short Track" },
    ".byline.ytmusic-player-bar": { textContent: "Short Artist" },
    "#progress-bar": {
      getAttribute: (name: string) => name === "aria-valuemax" ? "100" : "0",
    },
    video: {
      paused: false,
      ended: false,
      readyState: 4,
      currentTime: 0,
      duration: 100,
    },
  });

  try {
    assert.equal(new YouTubeMusicProvider().extract()?.playback.duration_ms, 100_000);
  } finally {
    restore();
  }
});

test("preserves a YTM loop reset on the same track", () => {
  const provider = new YouTubeMusicProvider();
  const restoreNearEnd = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "Looping Track" },
    ".byline.ytmusic-player-bar": { textContent: "Looping Artist" },
    "#progress-bar": {
      getAttribute: (name: string) => name === "aria-valuemax" ? "214" : "212",
    },
    video: {
      paused: false,
      ended: false,
      readyState: 4,
      currentTime: 212,
      duration: 213.061,
    },
  });

  try {
    assert.equal(provider.extract()?.playback.position_ms, 212_000);
  } finally {
    restoreNearEnd();
  }

  const restoreLoopReset = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "Looping Track" },
    ".byline.ytmusic-player-bar": { textContent: "Looping Artist" },
    "#progress-bar": {
      getAttribute: (name: string) => name === "aria-valuemax" ? "214" : "0",
    },
    video: {
      paused: false,
      ended: false,
      readyState: 4,
      currentTime: 0,
      duration: 213.061,
    },
  });

  try {
    assert.equal(provider.extract()?.playback.position_ms, 0);
  } finally {
    restoreLoopReset();
  }
});

test("uses hqdefault when no YTM thumbnail element is available", () => {
  const restore = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "Fallback Art" },
    ".byline.ytmusic-player-bar": { textContent: "Artist" },
    "#progress-bar": {
      getAttribute: (name: string) => name === "aria-valuemax" ? "214" : "10",
    },
    video: {
      paused: false,
      ended: false,
      readyState: 4,
      currentTime: 10,
      duration: 213.061,
    },
  }, "?v=jNQXAC9IVRw");

  try {
    assert.equal(
      new YouTubeMusicProvider().extract()?.metadata.art_url,
      "https://i.ytimg.com/vi/jNQXAC9IVRw/hqdefault.jpg",
    );
  } finally {
    restore();
  }
});

test("prefers a player thumbnail with a YTM video ID over a channel avatar", () => {
  const restore = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "Compact Track" },
    ".byline.ytmusic-player-bar": { textContent: "Compact Artist" },
    "ytmusic-player-bar img": [
      { src: "https://yt3.googleusercontent.com/channel-avatar" },
      { src: "https://i.ytimg.com/vi/videoId12345/hqdefault.jpg" },
    ],
    "#progress-bar": {
      getAttribute: (name: string) => name === "aria-valuemax" ? "149" : "13",
    },
    video: {
      paused: false,
      ended: false,
      readyState: 4,
      currentTime: 13,
      duration: 149,
    },
  });

  try {
    const result = new YouTubeMusicProvider().extract();
    assert.equal(result?.metadata.track_id, "ytm:videoId12345");
    assert.equal(result?.metadata.art_url, "https://i.ytimg.com/vi/videoId12345/hqdefault.jpg");
  } finally {
    restore();
  }
});

test("uses the active queue item's video ID when compact art is a channel avatar", () => {
  const restore = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "Compact Track" },
    ".byline.ytmusic-player-bar": { textContent: "Compact Artist" },
    "ytmusic-player-bar img": [{ src: "https://yt3.googleusercontent.com/channel-avatar" }],
    "ytmusic-player-queue-item[video-id][play-button-state='playing']": {
      getAttribute: (name: string) => name === "video-id" ? "queueVideo123" : null,
    },
    "#progress-bar": {
      getAttribute: (name: string) => name === "aria-valuemax" ? "149" : "13",
    },
    video: {
      paused: false,
      ended: false,
      readyState: 4,
      currentTime: 13,
      duration: 149,
    },
  });

  try {
    const result = new YouTubeMusicProvider().extract();
    assert.equal(result?.metadata.track_id, "ytm:queueVideo123");
    assert.equal(result?.metadata.art_url, "https://i.ytimg.com/vi/queueVideo123/hqdefault.jpg");
  } finally {
    restore();
  }
});

test("uses the visible YTM progress bar in compact mode", () => {
  const restore = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "Compact Track" },
    ".byline.ytmusic-player-bar": { textContent: "Compact Artist" },
    "#progress-bar": [
      {
        getClientRects: () => [],
        getAttribute: (name: string) => name === "aria-valuemax" ? "149" : "8",
      },
      {
        getClientRects: () => [{}],
        getAttribute: (name: string) => name === "aria-valuemax" ? "149" : "13",
      },
    ],
    video: {
      paused: false,
      ended: false,
      readyState: 4,
      currentTime: 13,
      duration: 149,
    },
  });

  try {
    assert.equal(new YouTubeMusicProvider().extract()?.playback.position_ms, 13_000);
  } finally {
    restore();
  }
});

test("uses the page-world video ID when the collapsed player drops ?v=", () => {
  const restore = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "Calamity" },
    ".byline.ytmusic-player-bar": { textContent: "Yakui The Maid • Goodnight World • 2011" },
    "ytmusic-player-bar img": [{ src: "https://yt3.googleusercontent.com/album-art=w60-h60-l90-rj" }],
    "#progress-bar": {
      getAttribute: (name: string) => name === "aria-valuemax" ? "220" : "13",
    },
    video: {
      paused: false,
      ended: false,
      readyState: 4,
      currentTime: 13,
      duration: 220,
    },
  }, "");
  (globalThis.document as any).documentElement = {
    getAttribute: (name: string) =>
      name === "data-mprisence-ytm-video-id" ? "HHjdNFdinUg" : null,
  };

  try {
    const result = new YouTubeMusicProvider().extract();
    assert.equal(result?.metadata.track_id, "ytm:HHjdNFdinUg");
    assert.equal(result?.canonicalUrl, "https://music.youtube.com/watch?v=HHjdNFdinUg");
  } finally {
    restore();
  }
});
