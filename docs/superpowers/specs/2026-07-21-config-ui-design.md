# Design: `mprisence config ui` — embedded local web config UI

Date: 2026-07-21
Status: approved (approach + scope), pending spec review

## Goal

A GUI for editing mprisence configuration without bloating the core binary or
adding a separate packaged application. Interactive: live template preview,
live variable palette, live validation.

## Approach (decided)

Embedded local web UI, always compiled (no cargo feature gate).

- New CLI surface: `mprisence config` becomes a subcommand group.
  - `mprisence config` / `mprisence config show` — existing behavior (compat).
  - `mprisence config ui` — start the local UI server.
- Server: `tiny_http` (small, synchronous, thread-per-connection). Binds
  `127.0.0.1:0` (random free port), prints the URL, opens it with `xdg-open`.
  Localhost only; no auth.
- UI: one embedded HTML file (`include_str!`), vanilla JS, no build step, no
  framework. Form fields for common settings + raw TOML pane as the single
  source of truth. Form edits patch the TOML text client-side; the server only
  validates and saves whole TOML documents. Unknown/uncommon keys are never
  touched (schema drift-proof).
- The running daemon needs no changes and no IPC: the existing `notify` config
  watcher hot-reloads `config.toml` when the UI saves it. The UI server also
  works standalone (daemon not running) — it queries MPRIS directly.

## Endpoints

| Method | Path | Purpose |
|---|---|---|
| GET | `/` | Embedded HTML page |
| GET | `/api/config` | Raw TOML text of the user's `config.toml` |
| PUT | `/api/config` | Body = full TOML text. Validate by parsing through the existing figment/schema path; 400 + error message on failure, write file on success |
| GET | `/api/players` | Current MPRIS players + playback status + render context (see interactivity) |
| POST | `/api/preview` | Body = `{details, state, large_text, small_text, player_bus_name?}` template strings. Compile + render against the chosen (default: active) player's live metadata. Returns rendered texts or compile/render error |

## Interactivity (researched capabilities)

All reusable from existing internals, no daemon IPC:

1. **Live template preview** — `TemplateManager::new_raw(details, state,
   large_text, small_text)` already exists (`src/template.rs:115`), currently
   `#[cfg(test)]`; un-gate it. Preview endpoint: `PlayerFinder` →
   `MetadataSource::from_mpris_with_override` → `to_media_metadata()` →
   `render_activity_texts`. Handlebars compile errors and render errors are
   returned as text and shown inline. UI debounces as-you-type (~300 ms).
2. **Variable palette** — `RenderContext` derives `Serialize`
   (`src/template.rs:17`, metadata flattened). `/api/players` returns it as
   JSON, so the UI lists every available `{{variable}}` with its live value
   from the currently playing track; click to insert into a template.
3. **Live player list** — same data, polled every 2 s; shows which player the
   preview renders against.
4. **Live validation** — TOML parsed through the existing config loading path
   on every save attempt; errors surfaced with figment's message. Client-side:
   Discord's 128-char limits on details/state shown as live char counters.
5. **Cover art** — preview shows the raw MPRIS `mpris:artUrl` only. The
   `CoverManager` pipeline (caching, providers, imgbb upload) is deliberately
   bypassed: it has network side effects (uploads) and generation-tracking
   complexity unsuitable for a read-only preview.

**Transport: plain HTTP polling.** 2 s poll for players, debounced POST for
preview. No SSE, no websockets — tiny_http's synchronous model makes polling
the simplest correct option and the data rate is trivial.

## Components

- `src/cli.rs` — `ConfigCommand { Show, Ui }` subcommand enum.
- `src/config_ui.rs` (new, single file) — server loop, 5 routes, JSON via
  existing `serde_json`. ~200 lines.
- `src/config_ui.html` (new, embedded via `include_str!`) — form + TOML
  pane + preview panel + variable palette. Vanilla JS. Lives under `src/`
  because `assets/` is excluded from the published crate (`Cargo.toml`
  `exclude`), which would break `cargo publish`.
- `src/template.rs` — remove `#[cfg(test)]` from `new_raw`.
- `Cargo.toml` — add `tiny_http`.

## Error handling

- Config parse failure on save: 400 with figment error text; file not written.
- Template compile/render failure in preview: 200 with `{error: "..."}` per
  field; UI shows inline, never blocks typing.
- No players running: preview renders against a static sample context
  (hardcoded sample track) so template editing still works.
- Port bind failure / xdg-open missing: print URL to stderr and keep serving.

## Testing

- Unit: preview rendering with sample context (reuses existing template test
  patterns); TOML validation round-trip (valid saves, invalid 400s).
- One integration check: start server on random port, GET `/`, PUT invalid
  TOML expecting 400, PUT valid TOML expecting file written.

## Explicitly out of scope (add when asked)

- Full schema-driven editor for every field (raw TOML pane covers the rest).
- Live Discord presence mirror / status panel of the running daemon.
- SSE/websocket push, JS framework, asset build pipeline.
- Cover art provider preview (upload side effects).
- Cargo feature gate (revisit if distro packagers ask).
