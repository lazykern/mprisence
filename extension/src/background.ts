/**
 * Background script (service worker for Chromium, persistent script for Firefox).
 *
 * Responsibilities:
 *   - Connect to native messaging host (mprisence-web-bridge)
 *   - Forward content script updates to the bridge
 *   - Forward bridge commands to the correct content script
 *   - Handle heartbeat / connection lifecycle
 */

import type { BridgeMessage, ExtMessage } from "./types";
import { NativeMessagingPort } from "./utils/native-messaging";

// Injected by esbuild define at build time
declare var __GIT_SHA__: string | undefined;
import { detectBrowser, makeSourceId } from "./utils/browser-detect";
import { PROTOCOL_VERSION } from "./types";

// ─── Native messaging port ───────────────────────────────────────

const nativePort = new NativeMessagingPort();
const browser = detectBrowser();

// ─── Tab state tracking ──────────────────────────────────────────

/**
 * Map of tab ID → whether the tab has media playing.
 * Used to filter commands to the correct tab.
 */
const activeTabs = new Map<number, string>();

// ─── Native host message handling ─────────────────────────────────

function onBridgeMessage(msg: BridgeMessage): void {
  switch (msg.type) {
    case "hello":
      console.log(
        `[mprisence] Bridge connected: v${msg.bridge_version}, protocol ${msg.protocol}`
      );

      // Protocol version check
      if (msg.protocol !== PROTOCOL_VERSION) {
        console.warn(
          `[mprisence] Protocol MISMATCH: extension=${PROTOCOL_VERSION}, bridge=${msg.protocol}`
        );
      }
      if (msg.git_sha) {
        console.log(`[mprisence] Bridge git SHA: ${msg.git_sha}`);
      }
      break;

    case "command":
      console.log(
        `[mprisence] ← Command from bridge: ${msg.command} source=${msg.source_id}`
      );
      forwardCommandToTab(msg);
      break;

    case "heartbeat":
      // Bridge sends heartbeat to check liveness
      // No response needed — just receiving keeps the connection alive
      break;
  }
}

function onBridgeDisconnect(): void {
  console.log("[mprisence] Bridge disconnected");
}

/**
 * Forward an MPRIS command to the content script in the correct tab.
 *
 * Content scripts cannot know their own tab ID at runtime, so source_id
 * contains tabId=0 as a placeholder. Instead of parsing it, reverse-lookup
 * the real tab ID from activeTabs (keyed by real tab ID, valued by source_id).
 */
function forwardCommandToTab(msg: BridgeMessage & { type: "command" }): void {
  // Reverse-lookup: find tab whose source_id matches command target
  for (const [tabId, sourceId] of activeTabs) {
    if (sourceId === msg.source_id) {
      sendCommandToTab(tabId, msg.command, msg.position_ms);
      return;
    }
  }

  // Fallback: parse tab ID from source_id (Chromium may have real tab ID)
  const parts = msg.source_id.split(":");
  const tabId = parseInt(parts[2] ?? "", 10);
  if (!isNaN(tabId) && tabId > 0) {
    sendCommandToTab(tabId, msg.command, msg.position_ms);
    return;
  }

  // Last resort: broadcast to all active tabs
  console.debug(`[mprisence] No tab match for source_id="${msg.source_id}", broadcasting`);
  for (const [tid] of activeTabs) {
    sendCommandToTab(tid, msg.command, msg.position_ms);
  }
}

function sendCommandToTab(
  tabId: number,
  command: string,
  positionMs?: number
): void {
  chrome.tabs.sendMessage(
    tabId,
    {
      type: "command",
      command,
      position_ms: positionMs,
    },
    (response) => {
      const err = chrome.runtime.lastError;
      if (err) {
        console.debug(`[mprisence] Command to tab ${tabId} failed:`, err.message);
      }
    }
  );
}

// ─── Content script message handling ──────────────────────────────

function sourceIdForSender(
  originalSourceId: string,
  sender: chrome.runtime.MessageSender
): string {
  const tabId = sender.tab?.id;
  if (tabId === undefined) return originalSourceId;

  const frameId = sender.frameId ?? 0;
  return `${makeSourceId(browser, tabId, frameId)}:frame`;
}

function onContentMessage(
  msg: ExtMessage,
  sender: chrome.runtime.MessageSender
): void {
  if (!msg || !msg.type) return;

  // Track active tabs
  if (msg.type === "update") {
    const tabId = sender.tab?.id;
    const bridgeMsg: ExtMessage = {
      ...msg,
      source_id: sourceIdForSender(msg.source_id, sender),
    };
    console.log(
      `[mprisence] ← Update from tab ${tabId}: source=${bridgeMsg.source_id} site=${bridgeMsg.site} "${bridgeMsg.metadata.title ?? "?"}" status=${bridgeMsg.playback.status} pos=${bridgeMsg.playback.position_ms} dur=${bridgeMsg.playback.duration_ms}`
    );
    if (tabId !== undefined) {
      activeTabs.set(tabId, bridgeMsg.source_id);
    }

    // Update badge
    setBadge(bridgeMsg.site, tabId);

    // Forward to native host
    nativePort.send(bridgeMsg);
  }

  if (msg.type === "remove") {
    const tabId = sender.tab?.id;
    const bridgeMsg: ExtMessage = {
      ...msg,
      source_id: sourceIdForSender(msg.source_id, sender),
    };
    console.log(`[mprisence] ← Remove from tab ${tabId}: source=${bridgeMsg.source_id}`);
    if (tabId !== undefined) {
      activeTabs.delete(tabId);
    }

    nativePort.send(bridgeMsg);
  }
}

// ─── Init ─────────────────────────────────────────────────────────

// Connect to native host
nativePort.connect(onBridgeMessage, onBridgeDisconnect);

// Send hello to bridge
nativePort.send({
  type: "hello",
  browser,
  extension_version: chrome.runtime.getManifest().version,
  protocol: PROTOCOL_VERSION,
  git_sha: (typeof __GIT_SHA__ !== "undefined" ? __GIT_SHA__ : undefined) as string | undefined,
});

// Listen for messages from content scripts
chrome.runtime.onMessage.addListener(
  (msg: ExtMessage, sender: chrome.runtime.MessageSender) => {
    onContentMessage(msg, sender);
  }
);

// ─── Badge ────────────────────────────────────────────────────────

const BADGE_COLORS: Record<string, [number, number, number]> = {
  youtube_music: [255, 0, 0],   // red
  generic: [100, 100, 100],     // gray
};

function setBadge(site: string, tabId?: number): void {
  const text = site === "youtube_music" ? "YTM" : site === "generic" ? "MED" : site.slice(0, 4).toUpperCase();
  const color = BADGE_COLORS[site] ?? [0, 150, 0];
  if (tabId !== undefined) {
    chrome.action?.setBadgeText({ tabId, text });
    chrome.action?.setBadgeBackgroundColor({ tabId, color: `rgb(${color[0]},${color[1]},${color[2]})` });
  } else {
    chrome.action?.setBadgeText({ text });
    chrome.action?.setBadgeBackgroundColor({ color: `rgb(${color[0]},${color[1]},${color[2]})` });
  }
}

function clearBadge(): void {
  chrome.action?.setBadgeText({ text: "" });
}

// ─── Clean up ─────────────────────────────────────────────────────

self.addEventListener("unload", () => {
  nativePort.disconnect();
});

console.log("[mprisence] Background script started");
