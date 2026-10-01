import { test } from "node:test";
import assert from "node:assert/strict";
import {
  isYouTubeMusicPlaying,
  sameYouTubeMusicTitle,
  YouTubeMusicProvider,
} from "./youtube-music.ts";

function installYouTubeMusicDom(
  elements: Record<string, unknown | unknown[]>,
  search = "?v=dQw4w9WgXcQ",
  page: {
    title?: string;
    rootAttrs?: Record<string, string>;
    onEvent?: (event: Event) => void;
  } = {},
): () => void {
  const previousDocument = globalThis.document;
  const previousWindow = globalThis.window;

  Object.defineProperty(globalThis, "document", {
    configurable: true,
    value: {
      title: page.title ?? "Never Gonna Give You Up - YouTube Music",
      documentElement: {
        getAttribute: (name: string) => page.rootAttrs?.[name] ?? null,
        hasAttribute: (name: string) => page.rootAttrs?.[name] !== undefined,
      },
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
    value: {
      location: { search },
      dispatchEvent: (event: Event) => {
        page.onEvent?.(event);
        return true;
      },
    },
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

test("publishes player-bar timing when the video element is absent", () => {
  const restore = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "New Player Track" },
    ".byline.ytmusic-player-bar": { textContent: "Artist" },
    "#play-pause-button": { getAttribute: () => "Pause" },
    "#progress-bar": {
      getAttribute: (name: string) => name === "aria-valuemax" ? "214" : "13",
    },
  });

  try {
    const result = new YouTubeMusicProvider().extract();
    assert.equal(result?.metadata.title, "New Player Track");
    assert.equal(result?.playback.status, "playing");
    assert.equal(result?.playback.position_ms, 13_000);
    assert.equal(result?.playback.duration_ms, 214_000);
  } finally {
    restore();
  }
});

test("uses player-bar timing while video metadata is unavailable", () => {
  const restore = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "New Player Track" },
    ".byline.ytmusic-player-bar": { textContent: "Artist" },
    "#progress-bar": {
      getAttribute: (name: string) => name === "aria-valuemax" ? "214" : "13",
    },
    video: {
      paused: false,
      ended: false,
      readyState: 0,
      currentTime: 0,
      duration: NaN,
    },
  });

  try {
    assert.equal(new YouTubeMusicProvider().extract()?.playback.duration_ms, 214_000);
  } finally {
    restore();
  }
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

test("prefers the enabled progress bar in the new player layout", () => {
  const restore = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "New Player Track" },
    ".byline.ytmusic-player-bar": { textContent: "Artist" },
    "#progress-bar": [
      {
        getClientRects: () => [{}],
        getAttribute: (name: string) => name === "aria-disabled" ? "true" : name === "aria-valuemax" ? "100" : "0",
      },
      {
        getClientRects: () => [{}],
        getAttribute: (name: string) => name === "aria-disabled" ? "false" : name === "aria-valuemax" ? "214" : "13",
      },
    ],
    video: {
      paused: false,
      ended: false,
      readyState: 4,
      currentTime: 13,
      duration: 214,
    },
  });

  try {
    assert.equal(new YouTubeMusicProvider().extract()?.playback.position_ms, 13_000);
  } finally {
    restore();
  }
});

test("sends play command to the visible player button", async () => {
  let hiddenClicks = 0;
  let visibleClicks = 0;
  const restore = installYouTubeMusicDom({
    "#play-pause-button": [
      { getClientRects: () => [], click: () => hiddenClicks++ },
      { getClientRects: () => [{}], click: () => visibleClicks++ },
    ],
  });

  try {
    await new YouTubeMusicProvider().command("play_pause");
    assert.equal(hiddenClicks, 0);
    assert.equal(visibleClicks, 1);
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

test("waits while the page-world video ID still belongs to the previous track", () => {
  const restore = installYouTubeMusicDom({
    ".title.ytmusic-player-bar": { textContent: "Clutter" },
    ".byline.ytmusic-player-bar": { textContent: "Yakui The Maid • Goodnight World • 2011" },
    "ytmusic-player-bar img": [{ src: "https://yt3.googleusercontent.com/album-art=w60-h60-l90-rj" }],
    "#progress-bar": {
      getAttribute: (name: string) => name === "aria-valuemax" ? "180" : "1",
    },
    video: {
      paused: false,
      ended: false,
      readyState: 4,
      currentTime: 1,
      duration: 180,
    },
  }, "");
  const attributes: Record<string, string> = {
    "data-mprisence-ytm-video-id": "HHjdNFdinUg",
    "data-mprisence-ytm-video-title": "Calamity",
  };
  (globalThis.document as any).documentElement = {
    getAttribute: (name: string) => attributes[name] ?? null,
  };

  try {
    const provider = new YouTubeMusicProvider();
    assert.equal(provider.extract(), null);

    attributes["data-mprisence-ytm-video-id"] = "UKP3I2Tot8s";
    attributes["data-mprisence-ytm-video-title"] = "Clutter";
    assert.equal(provider.extract()?.metadata.track_id, "ytm:UKP3I2Tot8s");
  } finally {
    restore();
  }
});

// Wiz miniplayer layout (`music_web_enable_wiz_miniplayer`): ytmusic-miniplayer
// replaces ytmusic-player-bar and #progress-bar in both player states.
function miniplayerDom(overrides: Record<string, unknown> = {}): Record<string, unknown> {
  return {
    "ytmusic-miniplayer .ytmusicTrackInfoTitle": {
      textContent: "Never Did Coke (feat. Swae Lee)",
      getAttribute: () => "Never Did Coke (feat. Swae Lee)",
    },
    "ytmusic-miniplayer .ytmusicTrackInfoBylineItem": {
      textContent: "Lil Yachty • Michigan Boy Boat • 2021",
      getAttribute: () => "Lil Yachty • Michigan Boy Boat • 2021",
    },
    "ytmusic-miniplayer img.ytmusicTrackInfoThumbnail": {
      src: "https://yt3.googleusercontent.com/aQCe2PgsGlQAw49Xu9prK8p4Le=w544-h544-l90-rj",
    },
    "ytmusic-miniplayer .ytmusicPlayerControlsPlayPauseButton button": {
      getAttribute: () => "Pause",
    },
    "ytmusic-miniplayer input.ytMusicMiniPlayerProgressBar": { value: "25", max: "184" },
    video: {
      paused: false,
      ended: false,
      readyState: 4,
      currentTime: 25.4,
      duration: 183,
    },
    ...overrides,
  };
}

test("reads track info and timing from the miniplayer layout", () => {
  const restore = installYouTubeMusicDom(miniplayerDom(), "?v=8eS0ehGtRy0", {
    title: "Never Did Coke (feat. Swae Lee) | YouTube Music",
  });

  try {
    const result = new YouTubeMusicProvider().extract();
    assert.equal(result?.metadata.title, "Never Did Coke (feat. Swae Lee)");
    assert.deepEqual(result?.metadata.artist, ["Lil Yachty"]);
    assert.equal(result?.metadata.album, "Michigan Boy Boat");
    assert.equal(result?.metadata.track_id, "ytm:8eS0ehGtRy0");
    assert.equal(result?.playback.status, "playing");
    assert.equal(result?.playback.position_ms, 25_000);
    assert.equal(result?.playback.duration_ms, 184_000);
  } finally {
    restore();
  }
});

test("uses the page-world video ID in the compact miniplayer", () => {
  const restore = installYouTubeMusicDom(miniplayerDom(), "", {
    rootAttrs: {
      "data-mprisence-ytm-video-id": "igpHMXzXJE0",
      "data-mprisence-ytm-video-title": "Never Did Coke",
    },
  });

  try {
    const result = new YouTubeMusicProvider().extract();
    assert.equal(result?.metadata.track_id, "ytm:igpHMXzXJE0");
    assert.equal(result?.canonicalUrl, "https://music.youtube.com/watch?v=igpHMXzXJE0");
  } finally {
    restore();
  }
});

test("falls back to the page title and player API author without a player bar", () => {
  const restore = installYouTubeMusicDom({
    video: { paused: false, ended: false, readyState: 4, currentTime: 5, duration: 183 },
  }, "?v=8eS0ehGtRy0", {
    title: "Never Did Coke (feat. Swae Lee) | YouTube Music",
    rootAttrs: {
      "data-mprisence-ytm-video-id": "8eS0ehGtRy0",
      "data-mprisence-ytm-video-title": "Never Did Coke",
      "data-mprisence-ytm-video-author": "Lil Yachty",
    },
  });

  try {
    const result = new YouTubeMusicProvider().extract();
    assert.equal(result?.metadata.title, "Never Did Coke (feat. Swae Lee)");
    assert.deepEqual(result?.metadata.artist, ["Lil Yachty"]);
  } finally {
    restore();
  }
});

for (const [command, selector] of [
  ["play_pause", "ytmusic-miniplayer .ytmusicPlayerControlsPlayPauseButton button"],
  ["next", "ytmusic-miniplayer .ytmusicPlayerControlsNextButton button"],
  ["previous", "ytmusic-miniplayer .ytmusicPlayerControlsPreviousButton button"],
] as const) {
  test(`sends ${command} to the miniplayer button`, async () => {
    let clicks = 0;
    const restore = installYouTubeMusicDom({
      [selector]: { getClientRects: () => [{}], click: () => clicks++ },
    });

    try {
      await new YouTubeMusicProvider().command(command);
      assert.equal(clicks, 1);
    } finally {
      restore();
    }
  });
}

test("matches player-bar titles with a featured-artist suffix", () => {
  assert.equal(sameYouTubeMusicTitle("Never Did Coke", "Never Did Coke (feat. Swae Lee)"), true);
  assert.equal(sameYouTubeMusicTitle("never did coke", "Never Did Coke"), true);
  // A stale API title with a suffix must not match a newly shown shorter title.
  assert.equal(sameYouTubeMusicTitle("Song (remix)", "Song"), false);
  assert.equal(sameYouTubeMusicTitle("Concrete Goonies", "Never Did Coke"), false);
  assert.equal(sameYouTubeMusicTitle("", "Never Did Coke"), false);
});

test("seeks through the page-world player API when it is available", async () => {
  const events: Event[] = [];
  const video = { currentTime: 214 };
  const restore = installYouTubeMusicDom({ video }, "", {
    rootAttrs: { "data-mprisence-ytm-video-id": "tbmKG-vJank" },
    onEvent: (event) => events.push(event),
  });

  try {
    await new YouTubeMusicProvider().command("set_position", 30_000);
    assert.equal(events.length, 1);
    assert.equal(events[0].type, "mprisence-ytm-seek");
    assert.equal((events[0] as CustomEvent).detail, 30_000);
    assert.equal(video.currentTime, 214);
  } finally {
    restore();
  }
});

test("seeks the video element when page-world is absent", async () => {
  const video = { currentTime: 0 };
  const restore = installYouTubeMusicDom({ video });

  try {
    await new YouTubeMusicProvider().command("set_position", 30_000);
    assert.equal(video.currentTime, 30);
  } finally {
    restore();
  }
});
