import { test } from "node:test";
import assert from "node:assert/strict";
import { isYouTubeMusicPlaying } from "./youtube-music.ts";

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
