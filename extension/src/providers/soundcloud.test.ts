import { test } from "node:test";
import assert from "node:assert/strict";
import { SoundCloudProvider } from "./soundcloud.ts";

function text(value: string) {
  return { textContent: value };
}

function installSoundCloudDom(elements: Record<string, unknown>): () => void {
  const previousDocument = globalThis.document;
  Object.defineProperty(globalThis, "document", {
    configurable: true,
    value: {
      querySelector: (selector: string) => elements[selector] ?? null,
      querySelectorAll: (selector: string) =>
        elements[selector] ? [elements[selector]] : [],
    },
  });
  return () => {
    Object.defineProperty(globalThis, "document", {
      configurable: true,
      value: previousDocument,
    });
  };
}

function playerBar(opts: {
  href: string;
  passed: string;
  duration: string;
  playing: boolean;
}): Record<string, unknown> {
  return {
    ".soundTitle__title": text("Some Track"),
    ".soundTitle__username": text("Some Artist"),
    ".playbackSoundBadge__titleLink[href]": { href: opts.href },
    ".playbackTimeline__timePassed span[aria-hidden=true]": text(opts.passed),
    ".playbackTimeline__duration span[aria-hidden=true]": text(opts.duration),
    ".playControls__play.playing": opts.playing ? {} : null,
    ".playControls__play": {},
  };
}

test("reads position and duration from the timeline text", () => {
  const restore = installSoundCloudDom(playerBar({
    href: "https://soundcloud.com/artist/track-a",
    passed: "1:05",
    duration: "8:11",
    playing: true,
  }));
  try {
    const result = new SoundCloudProvider().extract();
    assert.equal(result?.playback.status, "playing");
    assert.equal(result?.playback.position_ms, 65_000);
    assert.equal(result?.playback.duration_ms, 491_000);
  } finally {
    restore();
  }
});

test("parses hour-long durations", () => {
  const restore = installSoundCloudDom(playerBar({
    href: "https://soundcloud.com/artist/mix",
    passed: "1:02:03",
    duration: "2:00:00",
    playing: true,
  }));
  try {
    const result = new SoundCloudProvider().extract();
    assert.equal(result?.playback.position_ms, 3_723_000);
    assert.equal(result?.playback.duration_ms, 7_200_000);
  } finally {
    restore();
  }
});

test("uses the now-playing permalink without query as canonical URL", () => {
  const restore = installSoundCloudDom(playerBar({
    href: "https://soundcloud.com/artist/track-b?in=artist/sets/list#t=0",
    passed: "0:10",
    duration: "3:00",
    playing: false,
  }));
  try {
    const result = new SoundCloudProvider().extract();
    assert.equal(result?.canonicalUrl, "https://soundcloud.com/artist/track-b");
    assert.equal(result?.playback.status, "stopped");
  } finally {
    restore();
  }
});

test("falls back to the progress bar ratio at a zero time label", () => {
  const restore = installSoundCloudDom({
    ...playerBar({
      href: "https://soundcloud.com/artist/track-c",
      passed: "0:00",
      duration: "4:00",
      playing: true,
    }),
    ".playbackTimeline__progressBar": { getBoundingClientRect: () => ({ width: 25 }) },
    ".playbackTimeline__progressWrapper": { getBoundingClientRect: () => ({ width: 100 }) },
  });
  try {
    const result = new SoundCloudProvider().extract();
    assert.equal(result?.playback.position_ms, 60_000);
  } finally {
    restore();
  }
});
