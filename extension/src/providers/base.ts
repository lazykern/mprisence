import type {
  Capabilities,
  MediaMetadata,
  PlaybackState,
} from "../types";

export interface ProviderResult {
  metadata: MediaMetadata;
  playback: PlaybackState;
  capabilities: Capabilities;
  /** Override the page URL sent in ExtMessage (e.g. mini player watch URL) */
  pageUrl?: string;
  /** Canonical track/page URL when distinct from the visible page URL. */
  canonicalUrl?: string;
}

/**
 * Base media provider interface.
 * Each provider knows how to extract metadata from a specific site.
 */
export interface Provider {
  /** Check if this provider handles the given URL */
  matches(url: URL): boolean;

  /** Extract metadata and playback state from the page */
  extract(): ProviderResult | null;

  /** Stable site key used for bridge grouping/config, e.g. `youtube_music`. */
  readonly siteKey?: string;

  /** Execute a media control command. `positionMs` is absolute for set_position. */
  command(cmd: string, positionMs?: number): Promise<void>;
}

// Note: the generic fallback for UNSUPPORTED sites does not live here. The
// isolated world can't read the page's `navigator.mediaSession`, so generic
// collection runs in page-world.ts (MAIN world) instead; see startGenericMode
// there. content.ts relays its output through the same bridge path.
