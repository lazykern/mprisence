# Optimization & Refactoring Opportunities — mprisence 1.7.0-beta.1

**Generated**: 2026-05-21 (source-verified after review of OPTIMIZATION_FINDINGS.md era claims)  
**Codebase**: 12.5K LOC, 25 `.rs` files

---

## ⚠️ Verification Note

Several claims from the earlier `OPTIMIZATION_FINDINGS.md` and the initial `OPTIMIZE_REFS.md` were **wrong or overstated** after source-level verification. This file is the corrected version.

### Claims Retracted

| Claim | Why Wrong |
|-------|-----------|
| "5× `get_playback_status()` calls per cycle" | `RenderContext::new()` and `PlaybackState::from_with_status()` receive status as parameter — zero D-Bus calls. Actual: 2 calls in hot path, 1 redundant. |
| "`save_index()` rewrites entire 256+ entry index on every insertion" | Cache uses **file-per-entry** (`cache_dir/<blake3_hash>`). No monolithic index. Reads/writes are O(1) per entry. |
| "`RenderContext::new()` calls D-Bus for status_icon" | Receives `PlaybackStatus` as parameter; `status`/`status_icon` are `format!()` calls, not D-Bus. |
| "`to_media_metadata()` is a big win to skip" | Cost is ~5-10µs (HashMap lookups + String allocs). Dominated by D-Bus metadata fetch (~100-500µs). Template render already skipped via snapshot check. |
| "Background task clones 3 large objects" | `metadata_source` is MOVED, not cloned. `activity_texts` is 4 small Strings. `player_config` clone is ~2KB. Total cost < 1µs per track change. |
| "30+ fields in Presence struct" | 28 fields, each with clear purpose, well-grouped by comment convention. |
| "Health transitions log 300+ char info! lines" | Only `info!` on Clear transitions with state change (player disappeared). Routine tracking is `debug!/trace!`. |

---

## 🔴 Genuinely Worth Addressing

### #1 Redundant `get_playback_status()` in `update_activity()` 

**Call sites in `update()` polling path**:
- `update():369` — fetches status, passes to `PlaybackState::from_with_status()` and `HealthCheckInput` ✓
- `update_activity():692` — fetches status AGAIN even though caller at line 369 already has it ✗

**Call sites in `handle_event()` event path**:
- `handle_event():~1415` — fetches status for health transition (track change only)
- `update_activity():692` — fetches status AGAIN ✗

**Fix**: Add `playback_status: PlaybackStatus` parameter to `update_activity()`. Both callers already have it.

**Impact**: Eliminates 1 of 2 D-Bus round-trips on the hot path (~50% savings). `update_activity()` is called per track change + per significant position change in polling mode.

**Effort**: Trivial. Add one parameter, remove one `get_playback_status()` call.

### #2 Cover Provider Boilerplate

`catbox.rs` (283), `imgbb.rs` (131), `musicbrainz.rs` (628) — **1114 total lines across 4 files**.

Each implements independently:
- HTTP request construction (headers, body, multipart)
- Response parsing (JSON deserialization of upload URLs)
- Error mapping (network errors → `CoverArtError`)
- Authentication (catbox litter, imgbb API key, MusicBrainz API)

**Fix**: Extract shared `CoverUploadProvider` trait:
```rust
#[async_trait]
trait CoverUploadProvider {
    fn name(&self) -> &'static str;
    async fn upload(&self, data: &[u8], mime: &str) -> Result<String, CoverArtError>;
}
```

Then each provider implements only the upload logic; shared HTTP client + retry logic lives in the trait's default methods.

**Impact**: Reduces ~300 lines of duplicated code. Simplifies adding new providers.

**Effort**: Medium. Requires careful API design to handle divergent auth patterns.

---

## 🟡 Minor / Cosmetic

### #3 Error Hierarchy: 3 Levels with Stacked `#[from]`

`Error` → `MprisenceError` → `DiscordError`/`TemplateError`. Nested `#[from]` makes error source tracing opaque — compiler errors point to intermediate enum instead of root cause.

**Fix**: Flatten `MprisenceError` to include all variants directly, or use `color-eyre` for better error reporting.

### #4 `parking_lot::Mutex` — 4 Usages

`health`, `last_pushed_track_*` (3 fields), `last_reconnect_attempt`, `cover_cancel_token`, `last_effective_app_id`, `last_resolved_cover_art`.

For single-task daemon with no contention, `std::sync::Mutex` is ~9% faster (tokio#6317). Low priority but worth knowing for new code.

### #5 `#[allow(dead_code)]` on Test Constructors

`config/mod.rs:64,68,75` — 3 `#[allow(dead_code)]` for integration-test-only constructors. Use `#[cfg(test)]` module or `cfg_attr(test, allow(dead_code))` instead. Trivial cleanup.

---

## ✅ Already Well-Optimized (Verification)

| Area | Verification |
|------|-------------|
| `PlaybackState::from_with_status()` | Already receives status as parameter. No redundant D-Bus. |
| `RenderContext::new()` | `playback_status` and `status_icon` are local `format!()` calls. |
| `MetadataSource::cache_key` with `OnceLock` | Memoized, computed once. |
| `CompiledPattern` / `precompile_patterns()` | Already implemented in `schema.rs:377-449`. |
| Template snapshot caching | `last_rendered_snapshot` + `last_activity_texts` gates template renders. |
| `drain_latest_track_change()` | Coalesces rapid skips in event-driven mode. |
| Cache file-per-entry with BLAKE3 keys | O(1) reads/writes. No amortization problems. |
| `ActivityFraming` borrow pattern | Prevents clones on the hot `build_and_push_activity` path. |
| `metadata_source` moved into background task | Not cloned. Correct ownership transfer. |
| `parking_lot::RwLock` on config | Reads don't block other reads. Correct choice for config lookups. |
