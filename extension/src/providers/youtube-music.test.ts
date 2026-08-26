import { test } from "node:test";
import assert from "node:assert/strict";
import {
  isYouTubeMusicPlaying,
  YouTubeMusicProvider,
} from "./youtube-music.ts";

function installYouTubeMusicDom(
  elements: Record<string, unknown>,
  search = "?v=dQw4w9WgXcQ",
): () => void {
  const previousDocument = globalThis.document;
  const previousWindow = globalThis.window;

  Object.defineProperty(globalThis, "document", {
    configurable: true,
    value: {
      title: "Never Gonna Give You Up - YouTube Music",
      querySelector: (selector: string) => elements[selector] ?? null,
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
