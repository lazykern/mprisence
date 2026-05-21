// src/utils/native-messaging.ts
var NATIVE_HOST_NAME = "mprisence.web.bridge";
var NativeMessagingPort = class {
  port = null;
  onMessage = null;
  onDisconnect = null;
  reconnectTimer = null;
  /** Connect (or reconnect) to the native host. */
  connect(onMessage, onDisconnect) {
    this.onMessage = onMessage;
    this.onDisconnect = onDisconnect;
    this.doConnect();
  }
  doConnect() {
    if (this.port) {
      try {
        this.port.disconnect();
      } catch {
      }
    }
    try {
      this.port = chrome.runtime.connectNative(NATIVE_HOST_NAME);
    } catch (err) {
      console.error("[mprisence] Failed to connect native host:", err);
      this.scheduleReconnect();
      return;
    }
    this.port.onMessage.addListener((msg) => {
      this.onMessage?.(msg);
    });
    this.port.onDisconnect.addListener(() => {
      const error = chrome.runtime.lastError;
      if (error) {
        console.warn("[mprisence] Native host disconnected:", error.message);
      } else {
        console.log("[mprisence] Native host closed connection");
      }
      this.port = null;
      this.onDisconnect?.();
      this.scheduleReconnect();
    });
  }
  /** Send a message to the native host. */
  send(msg) {
    if (!this.port) {
      console.warn("[mprisence] Cannot send \u2014 no native host connection");
      return;
    }
    try {
      this.port.postMessage(msg);
    } catch (err) {
      console.error("[mprisence] Failed to send message:", err);
    }
  }
  /** Disconnect and stop reconnection. */
  disconnect() {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.port) {
      try {
        this.port.disconnect();
      } catch {
      }
      this.port = null;
    }
    this.onMessage = null;
    this.onDisconnect = null;
  }
  scheduleReconnect() {
    if (this.reconnectTimer) return;
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      if (this.onMessage && this.onDisconnect) {
        console.log("[mprisence] Reconnecting to native host...");
        this.doConnect();
      }
    }, 3e3);
  }
  get connected() {
    return this.port !== null;
  }
};

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

// src/types.ts
var PROTOCOL_VERSION = 1;

// src/background.ts
var nativePort = new NativeMessagingPort();
var activeTabs = /* @__PURE__ */ new Map();
function onBridgeMessage(msg) {
  switch (msg.type) {
    case "hello":
      console.log(
        `[mprisence] Bridge connected: v${msg.bridge_version}, protocol ${msg.protocol}`
      );
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
        `[mprisence] \u2190 Command from bridge: ${msg.command} source=${msg.source_id}`
      );
      forwardCommandToTab(msg);
      break;
    case "heartbeat":
      break;
  }
}
function onBridgeDisconnect() {
  console.log("[mprisence] Bridge disconnected");
}
function forwardCommandToTab(msg) {
  for (const [tabId2, sourceId] of activeTabs) {
    if (sourceId === msg.source_id) {
      sendCommandToTab(tabId2, msg.command, msg.position_ms);
      return;
    }
  }
  const parts = msg.source_id.split(":");
  const tabId = parseInt(parts[2] ?? "", 10);
  if (!isNaN(tabId) && tabId > 0) {
    sendCommandToTab(tabId, msg.command, msg.position_ms);
    return;
  }
  console.debug(`[mprisence] No tab match for source_id="${msg.source_id}", broadcasting`);
  for (const [tid] of activeTabs) {
    sendCommandToTab(tid, msg.command, msg.position_ms);
  }
}
function sendCommandToTab(tabId, command, positionMs) {
  chrome.tabs.sendMessage(
    tabId,
    {
      type: "command",
      command,
      position_ms: positionMs
    },
    (response) => {
      const err = chrome.runtime.lastError;
      if (err) {
        console.debug(`[mprisence] Command to tab ${tabId} failed:`, err.message);
      }
    }
  );
}
function onContentMessage(msg, sender) {
  if (!msg || !msg.type) return;
  if (msg.type === "update") {
    const tabId = sender.tab?.id;
    console.log(
      `[mprisence] \u2190 Update from tab ${tabId}: site=${msg.site} "${msg.metadata.title ?? "?"}" status=${msg.playback.status} pos=${msg.playback.position_ms} dur=${msg.playback.duration_ms}`
    );
    if (tabId !== void 0) {
      activeTabs.set(tabId, msg.source_id);
    }
    setBadge(msg.site, tabId);
    nativePort.send(msg);
  }
  if (msg.type === "remove") {
    const tabId = sender.tab?.id;
    console.log(`[mprisence] \u2190 Remove from tab ${tabId}`);
    if (tabId !== void 0) {
      activeTabs.delete(tabId);
    }
    nativePort.send(msg);
  }
}
nativePort.connect(onBridgeMessage, onBridgeDisconnect);
var browser = detectBrowser();
nativePort.send({
  type: "hello",
  browser,
  extension_version: chrome.runtime.getManifest().version,
  protocol: PROTOCOL_VERSION,
  git_sha: true ? "5fe7a39-dirty" : void 0
});
chrome.runtime.onMessage.addListener(
  (msg, sender) => {
    onContentMessage(msg, sender);
  }
);
var BADGE_COLORS = {
  youtube_music: [255, 0, 0],
  // red
  generic: [100, 100, 100]
  // gray
};
function setBadge(site, tabId) {
  const text = site === "youtube_music" ? "YTM" : site === "generic" ? "MED" : site.slice(0, 4).toUpperCase();
  const color = BADGE_COLORS[site] ?? [0, 150, 0];
  if (tabId !== void 0) {
    chrome.action?.setBadgeText({ tabId, text });
    chrome.action?.setBadgeBackgroundColor({ tabId, color: `rgb(${color[0]},${color[1]},${color[2]})` });
  } else {
    chrome.action?.setBadgeText({ text });
    chrome.action?.setBadgeBackgroundColor({ color: `rgb(${color[0]},${color[1]},${color[2]})` });
  }
}
self.addEventListener("unload", () => {
  nativePort.disconnect();
});
console.log("[mprisence] Background script started");
//# sourceMappingURL=background.js.map
