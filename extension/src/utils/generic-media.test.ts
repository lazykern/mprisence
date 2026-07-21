import { test } from "node:test";
import assert from "node:assert/strict";
import {
  pickArtwork,
  isNotificationSound,
  hasPublishableIdentity,
} from "./generic-media.ts";

test("pickArtwork: largest WxH area wins (not string compare)", () => {
  // "96x96" > "512x512" under naive string compare ('9' > '5'); area must win.
  const chosen = pickArtwork([
    { src: "small", sizes: "96x96" },
    { src: "big", sizes: "512x512" },
  ]);
  assert.equal(chosen, "big");
});

test("pickArtwork: 'any' (scalable) beats fixed sizes", () => {
  const chosen = pickArtwork([
    { src: "huge", sizes: "1024x1024" },
    { src: "vector", sizes: "any" },
  ]);
  assert.equal(chosen, "vector");
});

test("pickArtwork: multi-token sizes uses biggest token", () => {
  const chosen = pickArtwork([
    { src: "a", sizes: "48x48 96x96" },
    { src: "b", sizes: "200x200" },
  ]);
  assert.equal(chosen, "b");
});

test("pickArtwork: missing sizes still returns a src", () => {
  assert.equal(pickArtwork([{ src: "only" }]), "only");
});

test("pickArtwork: skips entries without src", () => {
  assert.equal(pickArtwork([{ sizes: "512x512" }, { src: "real", sizes: "10x10" }]), "real");
});

test("pickArtwork: empty / undefined → undefined", () => {
  assert.equal(pickArtwork([]), undefined);
  assert.equal(pickArtwork(undefined), undefined);
});

test("isNotificationSound: short finite clip is filtered", () => {
  assert.equal(isNotificationSound(3), true);
  assert.equal(isNotificationSound(7.9), true);
});

test("isNotificationSound: >=8s real media passes", () => {
  assert.equal(isNotificationSound(8), false);
  assert.equal(isNotificationSound(210), false);
});

test("isNotificationSound: live stream (Infinity/NaN/0) is NOT filtered", () => {
  assert.equal(isNotificationSound(Infinity), false);
  assert.equal(isNotificationSound(NaN), false);
  assert.equal(isNotificationSound(0), false);
});

test("hasPublishableIdentity: needs a title or an artist", () => {
  assert.equal(hasPublishableIdentity("Song", []), true);
  assert.equal(hasPublishableIdentity(undefined, ["Artist"]), true);
  assert.equal(hasPublishableIdentity("", []), false);
  assert.equal(hasPublishableIdentity("   ", []), false);
  assert.equal(hasPublishableIdentity(undefined, []), false);
});
