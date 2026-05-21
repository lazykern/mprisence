/**
 * Content script for mprisence browser extension.
 *
 * Architecture:
 *   content.ts (injected by manifest)
 *     └─ Injects a static <script> into page world for DOM access
 *        └─ Provider dispatcher runs in page world
 *           └─ Sends results via CustomEvent to content script
 *   content.ts → chrome.runtime.sendMessage → background.ts → native host
 *
 * CSP-safe: uses static injection + CustomEvent, not dynamic eval.
 */

import { detectBrowser, makeSourceId } from "./utils/browser-detect";
import type { ConfidenceLevel, ExtMessage, Capabilities, PlaybackState, MediaMetadata } from "./types";
import type { ProviderResult } from "./providers/base";
import { GenericMediaProvider } from "./providers/base";
import { YouTubeMusicProvider } from "./providers/youtube-music";

// ─── Provider registry ───────────────────────────────────────────

const providers = [
  new YouTubeMusicProvider(),
  new GenericMediaProvider(),
];

// ─── State tracking ──────────────────────────────────────────────

let lastSourceId = "";
let lastTitle = "";
let lastArtist = "";
let lastState = "";
let lastArtUrl = "";
let lastSentTime = 0;
const FORCE_RESEND_INTERVAL = 5000; // ms — re-send even if nothing changed (must be < bridge stale timeout)

const browser = detectBrowser();
const tabId = getTabId();
const sourceIdBase = makeSourceId(browser, tabId, 0);

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

// ─── Page-world script injection ─────────────────────────────────

/**
 * Inject a static script into page world that observes media state
 * and dispatches it via CustomEvent.
 *
 * This approach is CSP-safe: the script is loaded as a static resource,
 * not generated inline.
 */
function injectPageWorldScript(): void {
  const script = document.createElement("script");
  script.src = chrome.runtime.getURL("page-world.js");
  script.onload = () => script.remove();
  (document.head || document.documentElement).appendChild(script);
}

// Listen for CustomEvent from page-world script
window.addEventListener("mprisence-media-state", ((event: CustomEvent) => {
  const data = event.detail;
  if (data?.type === "media-state") {
    const result: ProviderResult = {
      metadata: data.metadata || { artist: [] },
      playback: data.playback || { status: "stopped", position_ms: 0, duration_ms: 0, rate: 1.0 },
      capabilities: data.capabilities || { play_pause: true, next: false, previous: false, seek: false, set_position: false, raise: false },
      confidence: (data.confidence as ConfidenceLevel) || "dom",
    };
    sendUpdate(result);
  }
}) as EventListener);

// ─── Direct DOM polling (fallback) ───────────────────────────────

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

let pollInterval: ReturnType<typeof setInterval> | null = null;

function startPolling(): void {
  if (pollInterval) return;

  // Poll every second for state changes
  pollInterval = setInterval(() => {
    const result = extractFromProviders();
    if (result) {
      sendUpdate(result);
    }
  }, 1000);
}

function stopPolling(): void {
  if (pollInterval) {
    clearInterval(pollInterval);
    pollInterval = null;
  }
}

// ─── Message sending ─────────────────────────────────────────────

function sendUpdate(result: ProviderResult): void {
  const sourceId = `${sourceIdBase}:frame`;
  const url = window.location.href;
  const origin = window.location.origin;

  // Deduplicate: skip if nothing changed (unless forced refresh)
  const now = Date.now();
  const titleKey = result.metadata.title ?? "";
  const artistKey = result.metadata.artist.join(",");
  const unchanged =
    lastSourceId === sourceId &&
    lastTitle === titleKey &&
    lastArtist === artistKey &&
    lastState === result.playback.status &&
    lastArtUrl === (result.metadata.art_url ?? "");
  if (unchanged && now - lastSentTime < FORCE_RESEND_INTERVAL) {
    return;
  }

  lastSourceId = sourceId;
  lastTitle = titleKey;
  lastArtist = artistKey;
  lastState = result.playback.status;
  lastArtUrl = result.metadata.art_url ?? "";
  lastSentTime = now;

  // Detect the site name from the first matching provider
  const urlObj = new URL(url);
  const site = providers.find((p) => p.matches(urlObj))?.constructor.name
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
    confidence: result.confidence,
  };

  chrome.runtime.sendMessage(msg).catch(() => {
    // Background might not be ready yet
  });
}

// ─── Listen for commands from background script ───────────────────

chrome.runtime.onMessage.addListener(
  (msg: any, _sender: chrome.runtime.MessageSender, sendResponse: (response?: any) => void) => {
    if (msg?.type === "command" && msg?.command) {
      const url = new URL(window.location.href);
      for (const provider of providers) {
        if (provider.matches(url)) {
          provider.command(msg.command).then(() => sendResponse({ ok: true }));
          return true; // keep channel open for async response
        }
      }
      sendResponse({ ok: false, error: "no matching provider" });
    }
    return true;
  }
);

// ─── Init ────────────────────────────────────────────────────────

// Start polling for media state
startPolling();

// Also inject page-world script for richer metadata
injectPageWorldScript();

// Clean up on page unload
window.addEventListener("beforeunload", () => {
  stopPolling();

  const msg: ExtMessage = {
    type: "remove",
    source_id: `${sourceIdBase}:frame`,
  };
  chrome.runtime.sendMessage(msg).catch(() => {});
});
