import type { BrowserKind } from "../types";

/**
 * Detect which browser is running this extension.
 * Uses user agent + global scope checks.
 */
export function detectBrowser(): BrowserKind {
  const ua = navigator.userAgent.toLowerCase();

  // Check order matters — Edge contains "chrome", Firefox doesn't
  if (ua.includes("firefox")) return "firefox";
  if (ua.includes("edg")) return "edge";
  if (ua.includes("vivaldi")) return "vivaldi";
  if (ua.includes("brave")) return "brave";
  if (ua.includes("chrome")) return "chromium";

  // Fallback — should not happen on known browsers
  console.warn("[mprisence] Unknown browser, assuming chromium");
  return "chromium";
}

/** Create a stable source ID for a tab+frame */
export function makeSourceId(
  browser: BrowserKind,
  tabId: number | undefined,
  frameId: number | undefined
): string {
  return `${browser}:tab:${tabId ?? 0}:${frameId ?? 0}`;
}
