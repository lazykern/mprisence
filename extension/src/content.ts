/**
 * Content script for mprisence browser extension.
 *
 * Architecture:
 *   content.ts (isolated world, injected by manifest)
 *     ├─ observes media elements and SPA DOM changes directly
 *     └─ receives optional page-world provider results via CustomEvent
 *   page-world.ts (MAIN world, injected by manifest) → CustomEvent
 *   content.ts → chrome.runtime.sendMessage → background.ts → native host
 *
 * CSP-safe: both scripts are manifest-declared content scripts; no runtime
 * injection or dynamic eval.
 */

import { detectBrowser, makeSourceId } from "./utils/browser-detect";
import type { ExtMessage, Capabilities, PlaybackState, MediaMetadata } from "./types";
import type { ProviderResult } from "./providers/base";
import { YouTubeMusicProvider } from "./providers/youtube-music";
import { YouTubeProvider } from "./providers/youtube";
import { SoundCloudProvider } from "./providers/soundcloud";
import { BandcampProvider } from "./providers/bandcamp";
import { TidalProvider } from "./providers/tidal";
import { AppleMusicProvider } from "./providers/apple-music";

// ─── Provider registry ───────────────────────────────────────────

const providers = [
  new YouTubeMusicProvider(),
  new YouTubeProvider(),
  new SoundCloudProvider(),
  new BandcampProvider(),
  new TidalProvider(),
  new AppleMusicProvider(),
];

// ─── State tracking ──────────────────────────────────────────────

let lastSourceId = "";
let lastTitle = "";
let lastArtist = "";
let lastState = "";
let lastArtUrl = "";
let lastPageUrl = "";
let lastCanonicalUrl = "";
let lastPositionSec = -1;
let lastDurationMs = -1;
let lastAlbum = "";
let lastAlbumArtist = "";
let lastTrackId = "";

const browser = detectBrowser();
const tabId = getTabId();
const sourceIdBase = makeSourceId(browser, tabId, 0);

function normalizeStringList(value: unknown): string[] {
  if (Array.isArray(value)) {
    return value.filter((item): item is string => typeof item === "string" && item.length > 0);
  }
  if (typeof value === "string" && value.length > 0) {
    return [value];
  }
  return [];
}

function getTabId(): number | undefined {
  // In content scripts, we can get tab ID from the runtime
  try {
    // Chromium: chrome.devtools.inspectedWindow.tabId — not applicable
    // Firefox: tabId is available via browser.runtime
    // Best effort: Content scripts don't have direct tab ID access
    // We use 0 as fallback
    if (typeof chrome !== "undefined" && chrome.devtools) {
      return undefined;
    }
  } catch {
    // ignore
  }
  return undefined;
}

// Listen for CustomEvent from page-world script.
// The page-world only dispatches when MediaSession metadata identity
// (title, artist, album, artwork src) changes — NOT on every position
// update. The isolated world handles position updates via timeupdate.
//
// It does NOT supply track_id, canonical_url or pageUrl fields.
// We preserve those from the last isolated-world update to prevent
// the bridge from seeing alternating /mismatched track IDs.
//
// Art-only events: page-world also dispatches YTM square cover art
// (yt3.googleusercontent.com) fetched via InnerTube API. These have
// empty title/artist — merge art_url into last provider metadata.
window.addEventListener("mprisence-media-state", ((event: CustomEvent) => {
  const data = event.detail;
  if (data?.type === "media-state") {
    const metadata: MediaMetadata = {
      ...(data.metadata || {}),
      artist: normalizeStringList(data?.metadata?.artist),
      album_artist: normalizeStringList(data?.metadata?.album_artist),
    };
    const result: ProviderResult = {
      metadata,
      playback: data.playback || { status: "stopped", position_ms: 0, duration_ms: 0 },
      capabilities: data.capabilities || { play_pause: true, next: false, previous: false, seek: false, set_position: false },
    };

    // Art-only merge: page-world sends square yt3 cover art from
    // InnerTube API with empty title/artist. Merge art_url into
    // last provider metadata so we don't overwrite title/artist
    // with empty values.
    const pwTitle = result.metadata.title ?? "";
    const pwArtist = result.metadata.artist.join(",");
    const pwArtUrl = result.metadata.art_url ?? "";
    const isArtOnly = !pwTitle && !pwArtist && !!pwArtUrl &&
      lastProviderMetadata !== null;
    if (isArtOnly) {
      result.metadata = {
        ...lastProviderMetadata,
        art_url: pwArtUrl,
      };
    }

    // Merge stable fields from the last isolated-world update so the
    // bridge sees a consistent track_id & canonical_url.
    // These only exist in the isolated-world path (provider extract).
    //
    // BUT: when the page-world fires on a GENUINE track change (title
    // or artist differs from last page-world send), DON'T carry over
    // the old track_id — let the next isolated-world update supply the
    // correct one instead of sending stale data.
    const isNewTrack =
      lastPageWorldMeta !== null &&
      (pwTitle !== lastPageWorldMeta.title || pwArtist !== lastPageWorldMeta.artist);

    if (!isNewTrack) {
      if (lastPageWorldMeta && !result.metadata.track_id) {
        result.metadata.track_id = lastPageWorldMeta.track_id;
      }
      if (!result.canonicalUrl) {
        result.canonicalUrl = lastCanonicalUrlPageWorld;
      }
    }

    sendUpdate(result);

    // Snapshot the fields we preserved so consecutive page-world
    // dispatches (same track) produce no-op dedup.
    lastPageWorldMeta = {
      title: result.metadata.title ?? "",
      artist: result.metadata.artist.join(","),
      album: result.metadata.album ?? "",
      art_url: result.metadata.art_url ?? "",
      track_id: result.metadata.track_id,
    };
    if (result.canonicalUrl) {
      lastCanonicalUrlPageWorld = result.canonicalUrl;
    }
  }
}) as EventListener);

// ─── Last known stable fields from page-world path ───────────

let lastPageWorldMeta: {
  title: string;
  artist: string;
  album: string;
  art_url: string;
  track_id: string | undefined;
} | null = null;
let lastCanonicalUrlPageWorld = "";

// Provider metadata snapshot for art-only merge (page-world InnerTube)
let lastProviderMetadata: MediaMetadata | null = null;
let extensionContextAlive = true;

function isContextInvalidatedError(err: unknown): boolean {
  if (!(err instanceof Error)) return false;
  return /Extension context invalidated/i.test(err.message)
    || /context invalidated/i.test(err.message);
}

function markExtensionContextDead(err: unknown): void {
  if (!isContextInvalidatedError(err)) return;
  extensionContextAlive = false;
  if (keepaliveInterval) {
    clearInterval(keepaliveInterval);
    keepaliveInterval = null;
  }
}

function safeSendMessage(msg: ExtMessage): void {
  if (!extensionContextAlive) return;
  try {
    chrome.runtime.sendMessage(msg).catch((err) => {
      markExtensionContextDead(err);
    });
  } catch (err) {
    markExtensionContextDead(err);
  }
}

// ─── Event-driven observation (Layer 1: isolated world) ──────────

function extractFromProviders(): ProviderResult | null {
  const url = new URL(window.location.href);
  for (const provider of providers) {
    if (provider.matches(url)) {
      const result = provider.extract();
      if (result) return result;
    }
  }
  return null;
}

function triggerUpdate(force = false): void {
  const result = extractFromProviders();
  if (result) sendUpdate(result, force);
}

/** Media-element events that always warrant an immediate update. */
const MEDIA_EVENTS = [
  "play",
  "pause",
  "ended",
  "ratechange",
  "seeked",
  "loadedmetadata",
  "durationchange",
];

/** `timeupdate` fires ~4x/s; throttle to ~1/s. */
let lastTimeupdate = 0;
function onTimeupdate(): void {
  const now = Date.now();
  if (now - lastTimeupdate < 900) return;
  lastTimeupdate = now;
  triggerUpdate();
}

function debounce<T extends (...args: any[]) => void>(fn: T, ms: number): T {
  let timer: ReturnType<typeof setTimeout> | null = null;
  return ((...args: any[]) => {
    if (timer) clearTimeout(timer);
    timer = setTimeout(() => {
      timer = null;
      fn(...args);
    }, ms);
  }) as T;
}

let keepaliveInterval: ReturnType<typeof setInterval> | null = null;

function startObserving(): void {
  // Capture-phase listeners on `document` catch media events from every
  // element — including ones added later — and catch non-bubbling events
  // such as `timeupdate`. These fire even while the tab is backgrounded.
  for (const ev of MEDIA_EVENTS) {
    document.addEventListener(ev, () => triggerUpdate(), true);
  }
  document.addEventListener("timeupdate", onTimeupdate, true);

  // SPA player-bar DOM changes (e.g. YouTube Music switching track without a
  // media-element event) fire no media events — observe the DOM too.
  const onMutation = debounce(() => triggerUpdate(), 500);
  const observer = new MutationObserver(() => onMutation());
  observer.observe(document.documentElement, { childList: true, subtree: true });

  // Keepalive: force-resend the current state so a paused, backgrounded tab is
  // not stale-pruned by the bridge. The browser throttles this to ~1/min in
  // background tabs; the bridge STALE_TIMEOUT (90s) tolerates that. An
  // unchanged re-send emits no D-Bus signal — the bridge's diffing publisher
  // drops it — it only refreshes the source's last_seen.
  keepaliveInterval = setInterval(() => triggerUpdate(true), 30_000);
}

// ─── Message sending ─────────────────────────────────────────────

function sendUpdate(result: ProviderResult, force = false): void {
  const sourceId = `${sourceIdBase}:frame`;
  // Prefer provider-supplied URL. When the update source provides no
  // URL fields (e.g. page-world script), reuse last known good URL —
  // but only if the content identity (title + artist) hasn't changed.
  // If identity changed and we still have no fresh URL, fall back to
  // window.location.href so the bridge sees the new page.
  const titleKey = result.metadata.title ?? "";
  const artistKey = normalizeStringList(result.metadata.artist).join(",");
  const identityChanged =
    lastTitle !== titleKey || lastArtist !== artistKey;
  const url = result.pageUrl
    || (!identityChanged && lastPageUrl)
    || window.location.href;
  const canonicalUrl = result.canonicalUrl
    || lastCanonicalUrl;
  const origin = window.location.origin;

  // Deduplicate: skip if nothing changed (unless forced refresh)
  const positionSec = Math.floor(result.playback.position_ms / 1000);
  const albumKey = result.metadata.album ?? "";
  const albumArtistKey = normalizeStringList(result.metadata.album_artist).join(",");
  const trackIdKey = result.metadata.track_id ?? "";
  const unchanged =
    lastSourceId === sourceId &&
    lastTitle === titleKey &&
    lastArtist === artistKey &&
    lastState === result.playback.status &&
    lastArtUrl === (result.metadata.art_url ?? "") &&
    lastPageUrl === url &&
    lastCanonicalUrl === (canonicalUrl ?? "") &&
    lastPositionSec === positionSec &&
    lastDurationMs === result.playback.duration_ms &&
    lastAlbum === albumKey &&
    lastAlbumArtist === albumArtistKey &&
    lastTrackId === trackIdKey;
  // Event-driven: drop a send only when nothing changed. The keepalive uses
  // force=true to refresh the bridge's last_seen even when unchanged.
  if (!force && unchanged) {
    return;
  }

  // Update provider metadata snapshot for page-world art-only merge.
  // Page-world InnerTube events have empty title, so this preserves
  // the provider's title/artist while allowing art_url to be replaced.
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

  // Detect stable site key from first matching provider.
  const urlObj = new URL(url);
  const provider = providers.find((p) => p.matches(urlObj));
  const site = provider?.siteKey ?? provider?.constructor.name
    .replace("Provider", "")
    .replace(/([A-Z])/g, "_$1")
    .toLowerCase()
    .replace(/^_/, "")
    .replace(/^generic$/, "generic") ?? "generic";

  const msg: ExtMessage = {
    type: "update",
    source_id: sourceId,
    url,
    origin,
    site: site,
    playback: result.playback,
    metadata: result.metadata,
    capabilities: result.capabilities,
    canonical_url: canonicalUrl || undefined,
  };

  safeSendMessage(msg);
}

// ─── Listen for commands from background script ───────────────────

try {
  chrome.runtime.onMessage.addListener(
    (msg: any, _sender: chrome.runtime.MessageSender, sendResponse: (response?: any) => void) => {
      if (msg?.type === "command" && msg?.command) {
        const url = new URL(window.location.href);
        for (const provider of providers) {
          if (provider.matches(url)) {
            provider.command(msg.command, msg.position_ms).then(() => sendResponse({ ok: true }));
            return true; // keep channel open for async response
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

// ─── Init ────────────────────────────────────────────────────────

function isSupportedPage(): boolean {
  const url = new URL(window.location.href);
  return providers.some((p) => p.matches(url));
}

if (isSupportedPage()) {
  startObserving();
  triggerUpdate();
  window.addEventListener("beforeunload", () => {
    if (keepaliveInterval) clearInterval(keepaliveInterval);
    const msg: ExtMessage = {
      type: "remove",
      source_id: `${sourceIdBase}:frame`,
    };
    safeSendMessage(msg);
  });
}
