// ─── Protocol types (matches bridge/src/protocol.rs) ─────────────

export const PROTOCOL_VERSION = 1;

export type BrowserKind = "firefox" | "chromium" | "brave" | "vivaldi" | "edge";

export type Status = "playing" | "paused" | "stopped";
export type ConfidenceLevel = "provider" | "dom" | "fallback";

export interface PlaybackState {
  status: Status;
  position_ms: number;
  duration_ms: number;
  rate: number;
}

export interface MediaMetadata {
  title?: string;
  artist: string[];
  album?: string;
  album_artist: string[];
  art_url?: string;
  track_id?: string;
}

export interface Capabilities {
  play_pause: boolean;
  next: boolean;
  previous: boolean;
  seek: boolean;
  set_position: boolean;
  raise: boolean;
}

// Extension → Bridge
export type ExtMessage =
  | { type: "hello"; browser: BrowserKind; extension_version: string; protocol: number; git_sha?: string }
  | {
      type: "update";
      source_id: string;
      url: string;
      origin: string;
      site: string;
      playback: PlaybackState;
      metadata: MediaMetadata;
      capabilities: Capabilities;
      confidence: ConfidenceLevel;
    }
  | { type: "remove"; source_id: string };

// Bridge → Extension
export type BridgeMessage =
  | { type: "hello"; bridge_version: string; protocol: number; git_sha?: string }
  | {
      type: "command";
      source_id: string;
      command: CommandKind;
      position_ms?: number;
    }
  | { type: "heartbeat" };

export type CommandKind =
  | "play_pause"
  | "next"
  | "previous"
  | "seek"
  | "set_position"
  | "raise";
