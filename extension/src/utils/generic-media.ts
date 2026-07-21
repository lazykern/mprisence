/**
 * Pure helpers for the generic (unsupported-site) media provider.
 * No DOM / browser APIs here so they stay unit-testable under `node --test`.
 */

export interface ArtworkEntry {
  src?: string;
  sizes?: string;
  type?: string;
}

/**
 * Pick the largest artwork by pixel area. `sizes` is a space-separated list
 * of `WxH` tokens (MediaImage spec); we take the biggest token in each entry.
 * `"any"` (scalable) wins over everything. Returns the chosen `src`.
 */
export function pickArtwork(artwork: ArtworkEntry[] | undefined): string | undefined {
  if (!artwork || artwork.length === 0) return undefined;

  let best: ArtworkEntry | undefined;
  let bestArea = -1;

  for (const entry of artwork) {
    if (!entry?.src) continue;
    const area = artworkArea(entry.sizes);
    if (area > bestArea) {
      bestArea = area;
      best = entry;
    }
  }
  return best?.src;
}

/** Largest `WxH` token's area, `Infinity` for `"any"`, 0 when unparseable. */
function artworkArea(sizes: string | undefined): number {
  if (!sizes) return 0;
  let max = 0;
  for (const token of sizes.trim().split(/\s+/)) {
    if (token.toLowerCase() === "any") return Number.POSITIVE_INFINITY;
    const m = /^(\d+)x(\d+)$/i.exec(token);
    if (m) {
      const area = parseInt(m[1], 10) * parseInt(m[2], 10);
      if (area > max) max = area;
    }
  }
  return max;
}

/**
 * A finite element shorter than 8s is almost certainly a UI/notification
 * sound (chat blip, hover preview), not real media worth publishing.
 * Live streams report `Infinity`/`NaN` duration and are NOT filtered.
 * Mirrors KDE Plasma Browser Integration's 8-second floor.
 */
export const NOTIFICATION_SOUND_MAX_SECONDS = 8;
export function isNotificationSound(durationSec: number): boolean {
  return (
    Number.isFinite(durationSec) &&
    durationSec > 0 &&
    durationSec < NOTIFICATION_SOUND_MAX_SECONDS
  );
}

/**
 * Whether we have enough to publish a generic source: some title, from any
 * fallback tier. Empty title + empty artist means we'd publish a blank
 * player — skip it.
 */
export function hasPublishableIdentity(title: string | undefined, artist: string[]): boolean {
  return !!(title && title.trim()) || artist.length > 0;
}
