import type { BridgeMessage, ExtMessage } from "../types";

const NATIVE_HOST_NAME = "mprisence.web.bridge";

/**
 * Manages a persistent native messaging connection to the bridge.
 * Uses `runtime.connectNative()` for long-lived session.
 */
export class NativeMessagingPort {
  private port: chrome.runtime.Port | null = null;
  private onMessage: ((msg: BridgeMessage) => void) | null = null;
  private onDisconnect: (() => void) | null = null;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;

  /** Connect (or reconnect) to the native host. */
  connect(
    onMessage: (msg: BridgeMessage) => void,
    onDisconnect: () => void
  ): void {
    this.onMessage = onMessage;
    this.onDisconnect = onDisconnect;
    this.doConnect();
  }

  private doConnect(): void {
    if (this.port) {
      try {
        this.port.disconnect();
      } catch {
        // ignore
      }
    }

    try {
      this.port = chrome.runtime.connectNative(NATIVE_HOST_NAME);
    } catch (err) {
      console.error("[mprisence] Failed to connect native host:", err);
      this.scheduleReconnect();
      return;
    }

    this.port.onMessage.addListener((msg: BridgeMessage) => {
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
  send(msg: ExtMessage): void {
    if (!this.port) {
      console.warn("[mprisence] Cannot send — no native host connection");
      return;
    }
    try {
      this.port.postMessage(msg);
    } catch (err) {
      console.error("[mprisence] Failed to send message:", err);
    }
  }

  /** Disconnect and stop reconnection. */
  disconnect(): void {
    if (this.reconnectTimer) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    if (this.port) {
      try {
        this.port.disconnect();
      } catch {
        // ignore
      }
      this.port = null;
    }
    this.onMessage = null;
    this.onDisconnect = null;
  }

  private scheduleReconnect(): void {
    if (this.reconnectTimer) return; // already pending
    this.reconnectTimer = setTimeout(() => {
      this.reconnectTimer = null;
      if (this.onMessage && this.onDisconnect) {
        console.log("[mprisence] Reconnecting to native host...");
        this.doConnect();
      }
    }, 3000);
  }

  get connected(): boolean {
    return this.port !== null;
  }
}
