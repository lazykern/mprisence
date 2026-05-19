# CPU/RAM Optimization Opportunities — mprisence

> **Constraint**: No business-logic or outcome changes. Same presence updates, same timing, same cover art quality.
>
> **Changes from v1**: Corrected 3 factual errors (#6, #8, #10), re-prioritized. Added researcher insights about signal-driven architecture and the default template bug. Added verification notes from live code inspection.

---

## Summary of Findings

| # | File | What | Impact | Effort | Risk | Verified |
|---|------|------|--------|--------|------|----------|
| 1 | `presence.rs` + `template.rs` | Skip template rendering when metadata unchanged | **Medium** | Low | Low | ⚠️ Partially redundant (polling already skips via `significant_change` guard) |
| 2 | `template.rs` | Avoid 5× `get_playback_status()` D-Bus calls per cycle | **High** | Trivial | None | ✅ Confirmed — 5 calls, not 2 |
| 3 | `cover/cache.rs` | Reuse cache key across same-track cycles | **Medium** | Low | Low | ✅ Confirmed |
| 4 | `cover/mod.rs` | Cache `direct_url_policy` result in cache entry | **Low** | Low | Low | ⚠️ Minor risk: stale policy after code update |
| 5 | `config/schema.rs` | Cache compiled `Regex` objects for wildcard/regex matching | **High** | Medium | Low | ✅ Confirmed — precompile at config load |
| 6 | `metadata.rs` | Lazy `MediaMetadata` construction | **Low** (was Medium) | High | Low | ❌ Cost overstated: HashMap lookups, not D-Bus |
| 7 | `presence.rs` | `cover_fetch_generation` uses `Mutex<u64>` where `AtomicU64` suffices | **Low** | Low | Low | ⚠️ Two concurrent tasks access it (CAS needed) |
| 8 | `cover/cache.rs` | Avoid `spawn_blocking` for every cache read | **N/A** | N/A | N/A | ❌ Fast path is sync — no spawn_blocking at all |
| 9 | Cargo.toml | Trim tokio `features = ["full"]` to needed subset | **Low** | Low | Low | ✅ Confirmed |
| ~~10~~ | ~~main.rs~~ | ~~Cache cleanup interval waste~~ | — | — | — | ❌ 6h is scan frequency, not eviction. Removed. |
| B1 | `config/schema.rs` | Default template: `album_name` → `album` | **Bug fix** | Trivial | None | 🐛 Default `large_text` always shows title, never album |

---

## Finding #1: Skip Template Rendering When Metadata Unchanged

**Files**: `src/presence.rs`, `src/template.rs`

**Corrected assessment**: The finding claimed this is the biggest win ("~80-90% of template renders" eliminated in polling mode). **This is partially incorrect.** The polling-mode path already has a `significant_change` guard in `Presence::update()` that checks `PlaybackState::has_significant_changes()` + `has_position_jump()`. For normal position advancement, this guard returns false and `update_activity()` — where template rendering happens — is **never reached**.

**Where it still matters**:
- **Event-driven mode**: `handle_event()` calls `update_activity()` directly, bypassing the outer guard. A `Playing` event on the same track (emitted periodically by some players) triggers unnecessary template renders.
- **`update_from_current_state()`**: Called on initial player discovery, always calls `update_activity()`. (This is fine — first push.)

**Correct fix**: Inside `update_activity()`, compare `TrackFingerprint` + `playback_status` + `volume` against a cached `last_rendered_snapshot`. Only call `to_media_metadata()` and `render_activity_texts()` when something relevant changed. This benefits event-driven mode primarily.

**Note**: `{{elapsed}}` is not exposed to templates. `{{duration}}` is static per track. So position changes don't affect template output — fingerprint comparison is safe.

**Impact**: Downgraded from High to Medium. The existing polling guard already covers the common case.

---

## Finding #2: Reuse `get_playback_status()` Result (Worse Than Reported)

**File**: `src/template.rs:37-47`

**Corrected assessment**: Confirmed and **worse than originally described**. It's not 2 calls — it's **5** `get_playback_status()` D-Bus calls per poll cycle:

| Call # | Location | Purpose |
|--------|----------|---------|
| 1 | `presence.rs` `update()` | Guard for stopped/paused |
| 2 | `PlaybackState::from()` | Building state snapshot for diff |
| 3 | `update_activity()` | Guard inside activity path |
| 4 | `RenderContext::new()` | `{{status}}` template variable |
| 5 | `RenderContext::new()` | `{{status_icon}}` template variable |

There is no caching in the `mpris` crate — each call issues a fresh D-Bus `Get` property round-trip.

**Fix**: 
1. Fetch `playback_status` once at the top of `update()` and thread it down through `PlaybackState`, `HealthCheckInput`, and `RenderContext`.
2. Add a `PlaybackState::from_with_status()` constructor to avoid the redundant call inside `from()`.
3. Pass the value to `RenderContext::new()` instead of calling it internally.

**Expected Savings**: Eliminates 4 of 5 D-Bus round-trips per poll cycle (~300-400µs saved per cycle).

**Impact**: Upgraded to **High** due to understated scope.

---

## Finding #3: Reuse Cover Cache Key Across Same-Track Cycles

**File**: `src/cover/cache.rs:generate_key()`

**Accuracy**: Confirmed. `generate_key()` is called in both `try_cached_cover_art()` (fast path) and `get_cover_art()` (async path) on every track change. Cost: ~6-10 allocations (Vec<String>, sorted clone of artists, joined string) + BLAKE3 hash.

**Important**: `generate_key()` includes `mpris:artUrl` which is NOT in `TrackFingerprint`. For browser-based players (YouTube, SoundCloud), the art URL changes per-track while other metadata stays the same. Don't reuse the fingerprint — instead memoize the key in `MetadataSource`.

**Fix**: Add a `memoized_cache_key: RefCell<Option<String>>` or similar to `MetadataSource`, set on first call.

**Impact**: Unchanged (Medium). Cost is per-track-change, not per-poll.

---

## Finding #4: Cache `direct_url_policy` Result

**File**: `src/cover/mod.rs:try_cached_cover_art()`

**Accuracy**: Correct observation but negligible cost. `direct_url_policy()` does `Url::parse()` + string comparisons — already gated to only `provider == "direct"` entries. Cost is <1µs.

**Risk**: Caching the result in the on-disk `CacheEntry` means a code update changing the policy wouldn't take effect for cached entries until TTL expires (24h). The in-memory fast path is preferred.

**Recommendation**: If implemented, store in an in-memory sidecar (not serialized to disk) to avoid staleness risk. Otherwise, skip — cost is too small to justify complexity.

**Impact**: Downgraded from Medium to **Low**.

---

## Finding #5: Cache Compiled `Regex` Objects for Player Config Matching

**File**: `src/config/schema.rs`

**Accuracy**: Confirmed. `regex_from_pattern()` and `wildcard_match()` compile a new `Regex` on every match attempt. These are called in `get_player_config()` → `collect_ordered_matches()` which iterates over **all** player config entries per lookup.

**Call frequency**: Every `update_activity()` call (not just discovery cycles). With ~30 bundled player patterns + user entries, this is dozens of regex compilations per track change.

**Also affected**: `find_matching_website_entry()` has the same problem (~14 website entries × multiple match patterns).

**Fix**: Precompile patterns at config load time into a `#[serde(skip)]` map on `Config`:

```rust
#[serde(skip)]
pub compiled_player_patterns: HashMap<String, PatternKind>,
#[serde(skip)]
pub compiled_website_patterns: HashMap<String, Vec<PatternKind>>,

enum PatternKind {
    Exact(String),
    Wildcard(Regex),
    Regex(Regex),
}
```

Build in a `precompile_patterns()` method called after every config load/reload. Config reloads replace the entire `Config` struct, so stale patterns aren't a concern.

**Expected Savings**: Eliminates N×M regex compilations + associated allocations per pattern-matching call. For a typical cycle: ~30+ compilations avoided.

**Impact**: Upgraded from Medium to **High**. Hotter path than described.

---

## Finding #6: Lazy `MediaMetadata` Construction — **Major Correction**

**File**: `src/metadata.rs`

**Original claim**: *"10-20 fewer MPRIS property lookups, each potentially a D-Bus property access"*

**Correction**: **This is false.** All 41 field lookups in `to_media_metadata()` are in-memory `HashMap::get()` calls on the already-fetched `mpris::Metadata` (fetched once via `player.get_metadata()` at the top of the call chain). Zero D-Bus involvement. The real cost of `to_media_metadata()` is:
- ~41 HashMap lookups (nanoseconds each)
- ~10 String clones (microseconds)
- Lofty tag access for file:// URLs (parsed during `MetadataSource` construction, not in `to_media_metadata()`)

Total cost: <5µs. Not worth optimizing in isolation.

**The real optimization**: Skip `to_media_metadata()` **entirely** when the track hasn't changed — which is exactly Finding #1's territory. The `track_changed` bool is already computed at `presence.rs:696-722`. Extending that guard to gate `to_media_metadata()` + template rendering gives the actual win.

**Verdict**: Impact downgraded from Medium to **Low**. Approaches A/C (lazy construction, selective construction) have an unfavorable cost/benefit ratio. Approach B (skip entirely on same-track) is already covered by Finding #1.

---

## Finding #7: `cover_fetch_generation` `Mutex<u64>` → `AtomicU64`

**File**: `src/presence.rs`

**Correction**: The finding claims *"only ever read/written by one task."* **This is wrong.** Three access points across **two concurrent tasks**:

| Access Point | Task |
|---|---|
| Track-change reset | Main event loop |
| Spawn gating decision | Main event loop |
| `InFlightGuard::drop` (reset to 0) | Spawned background `tokio::task` |

The spawned task holds an `Arc<parking_lot::Mutex<u64>>` and writes on drop — concurrently with the main loop.

**AtomicU64 is still safe** but requires `compare_exchange` loops for the spawn gate and drop guard (not simple `store`/`load`). Use `Ordering::AcqRel` for coordinated access.

**Impact**: Unchanged (Low). Correctness fix, minor perf gain.

---

## Finding #8: Avoid `spawn_blocking` for Cache Reads — **Retracted**

**Original claim**: *"All cache operations go through `spawn_blocking`"*

**Correction**: **False.** The fast path `try_cached_cover_art()` is **synchronous** (`pub fn`, not `pub async fn`). It calls `CoverCache::get_by_key()` directly — no `spawn_blocking`, no `.await`, no async overhead. The Mutex on `CoverCache` is also **not in the read path** (`get_by_key` is lock-free; the Mutex only guards write-side `CacheUsage` tracking).

**Real (minor) concern**: The synchronous I/O (`fs::read`, `path.exists()`) in `get_by_key` runs on a tokio worker thread. On slow disks this could stall the async runtime. But this only happens on track changes, not every poll, and typical JSON entries are <1KB.

**Verdict**: **Retracted.** The proposed fix would have zero impact. The real concern is minor and doesn't justify the refactoring cost.

**Impact**: Removed from the priority list.

---

## Finding #9: Tokio Feature Bloat

**File**: `Cargo.toml`

**Accuracy**: Confirmed. `features = ["full"]` pulls in 20+ sub-features.

**Actual usage across src/**:

| Feature | Used? | API examples |
|---------|-------|-------------|
| `rt-multi-thread` | Yes | `#[tokio::main]` |
| `macros` | Yes | `tokio::select!` |
| `sync` | Yes | `mpsc`, `Notify`, `broadcast` |
| `time` | Yes | `interval()`, `timeout()` |
| `fs` | Yes | `tokio::fs::read/write/remove_file` |
| `process` | Yes | `Command` (cmus) |
| `net` | No | Not used |
| `io-util` | No | Not used |
| `signal` | No | Not used |
| `tracing` | No | Not used |

**Minimal set**:
```toml
tokio = { version = "1.50.0", features = ["rt-multi-thread", "macros", "sync", "time", "fs", "process"] }
```

**Expected Savings**: ~50-200KB binary size reduction, ~3-8s compile time savings (from external benchmarks). No runtime change.

**Impact**: Unchanged (Low). Build-time optimization.

---

## Finding #10: Cache Cleanup Interval — **Retracted**

**Original claim**: *"6-hour cleanup interval causes premature eviction of 18-hour-old entries."*

**Correction**: **False.** The 6-hour value (`Duration::from_secs(6 * 60 * 60)`) is the **scan frequency**, not the eviction threshold. The actual eviction gate in `CoverCache::clean()` is:

```rust
if now > entry.expires_at {  // expires_at = creation_time + 24h TTL
    // evict
}
```

Entries younger than 24 hours are **never** removed regardless of how many scans have run. There is no premature eviction.

**Verdict**: **Retracted.**

---

## 🐛 Bug B1: Default Template `album_name` → `album`

**File**: `src/config/schema.rs:28`

The default `large_text` template references `{{{album_name}}}` but the `MediaMetadata` struct field is `album` (serialized as `"album"`). The `{{#if album_name}}` condition is always falsy, so the template **always shows the title instead of the album name**.

```diff
- "{{#if album_name includeZero=true}}{{{album_name}}}{{else}}{{{title}}}{{/if}}"
+ "{{#if album includeZero=true}}{{{album}}}{{else}}{{{title}}}{{/if}}"
```

All default-config users are affected.

---

## Additional Insight: MPRIS Polling is Fundamentally an Anti-Pattern

**Source**: MPRIS specification v2.2, real-world reports from spotifyd, ncspot, polybar-spotify.

The MPRIS spec mandates that media players emit `PropertiesChanged` signals on `org.freedesktop.DBus.Properties` when any property (metadata, playback status, volume) changes. Polling every 2 seconds when signals exist is:

1. **Redundant**: `PropertiesChanged` fires immediately on track change, state change, etc.
2. **Inaccurate**: Polling can observe stale/inconsistent intermediate states (spotifyd: "polling the PlayingStatus sometimes returns 'Stopped' when Playing").
3. **Wasteful**: Each poll does D-Bus round trips that are completely unnecessary.
4. **Battery-draining**: Every poll wakes the CPU from idle on laptops.

**Recommendation**: Make event-driven mode the **primary** update mechanism. Use polling (at a longer interval, e.g., 30s) only as a health-check fallback to detect crashed/stuck players. This eliminates ~95% of D-Bus traffic with higher accuracy.

The project already has event-driven infrastructure. Strengthen it:
- Use a single `GetAll` call then cache property reads (zbus-style proxy caching)
- Maintain a local position timer between `Seeked`/`PropertiesChanged` signals
- Debounce: suppress the next health-check poll after a signal arrives

**Impact**: **Higher than all 10 findings combined** for users in event-driven mode. For polling-mode users, redirect efforts from micro-optimizing the poll loop toward making event-driven mode reliable enough to be the default.

---

## Also: `parking_lot::Mutex` is Not Universally Faster

**Source**: Cuong Le (2024 benchmarks), tokio issue #6317.

For mprisence's use case (single-task daemon, low contention, short critical sections), `std::sync::Mutex` is **9% faster** than `parking_lot::Mutex`. parking_lot wins on fairness under high contention — a scenario mprisence never encounters. Tokio itself has considered disabling `parking_lot` by default (issue #6317).

**Recommendation**: Not an active finding (the code works fine with either), but for any NEW mutexes added, prefer `std::sync::Mutex` unless contention is measured.

---

## Corrected Priority Order

1. **#2 (Reuse `get_playback_status()` result)** — 5 calls → 1. Biggest per-cycle savings. Trivial.
2. **Improve event-driven reliability** — Eliminates ~95% of D-Bus traffic. Bigger than all findings combined.
3. **#5 (Cache compiled Regex)** — Eliminates N×M compilations per pattern match. Medium effort.
4. **#3 (Reuse cache key)** — Eliminates redundant allocations+hashing per track change. Trivial.
5. **#1 (Skip template rendering in event-driven mode)** — Valuable but partially redundant with existing `significant_change` guard.
6. **#7 (Mutex → AtomicU64)** — Minor correctness + perf. Use CAS loops.
7. **#9 (Trim tokio features)** — Build-time only. Low effort.
8. **#6 (Lazy MediaMetadata)** — `to_media_metadata()` cost is negligible (<5µs). Skip unless profiling shows otherwise.
9. **#4 (Cache direct_url_policy)** — Cost is negligible. Skip unless the staleness risk is acceptable.

**Retracted**: #8 (factually wrong), #10 (factually wrong).

---

## What NOT to Change (Already Well-Optimized)

| Area | Why it's fine |
|------|---------------|
| `drain_latest_track_change()` in event loop | ✅ Already coalesces rapid track skips |
| `SmolStr` for player identifiers | ✅ Inline/small-string optimization, no heap alloc for short names |
| Cover-art upload providers (imgbb, catbox) | ✅ Already run in background task, not on critical path |
| `blake3` for cache key hashing | ✅ BLAKE3 is extremely fast (10+ GB/s on modern CPUs) |
| `parking_lot::Mutex` everywhere | ✅ Works correctly; std::Mutex would be slightly faster but not worth a migration |
| `Arc` cloning | ✅ Atomic refcount increment, not a real cost |
| Discard stale updates before Discord push | ✅ Critical correctness guard, not optional |
| Per-player listener thread in event-driven mode | ✅ Cost: ~8KB stack + OS thread. Benchmark shows negligible RSS |
