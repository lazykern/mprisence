# Research Progress

## Status: Complete

All 8 claims validated against external sources. Findings written to `findings-research.md`.

## Key Results

1. **Handlebars ~10-50µs**: ✅ Plausible for small templates (2.3µs measured). ❌ Misleading — large templates hit 3.4ms.
2. **BLAKE3 ~3 cycles/byte**: ✅ Accurate on AVX-512 hardware. ~4-6 without.
3. **D-Bus ~0.1-1ms**: ✅ Conservative. Real numbers ~77-125µs for simple calls.
4. **parking_lot vs std Mutex**: ❌ Claim of "faster" is wrong. std is faster in low-contention (the mprisence case). parking_lot wins on fairness under contention.
5. **tokio full vs minimal**: ✅ Correct. ~40KB + ~2s for small programs. Bigger savings possible.
6. **AtomicU64 vs Mutex\<u64\>**: ✅ Correct and safe. Key subtlety: use appropriate Ordering.
7. **spawn vs spawn_blocking**: ✅ Correct. spawn is 34x faster for lightweight tasks. spawn_blocking for blocking I/O.
8. **MPRIS anti-patterns**: 🚨 **Critical finding missed**: Polling is fundamentally wrong. Use PropertiesChanged signals.

## Sources Consulted
- 13 kept primary sources (repos, benchmarks, papers, official docs)
- 6 dropped (outdated or irrelevant)
- 4 search passes across ~45 queries total


---

## Optimization #6 Verification

**Status**: Complete. Written to `findings-6.md`.

### Summary
- `to_media_metadata()` populates 51 struct fields via 41 in-memory lookup calls
- **Critical correction**: All lookups are HashMap accesses on already-fetched MPRIS metadata — NOT D-Bus calls as the finding claimed
- The D-Bus call happens once per cycle (`player.get_metadata()` at presence.rs:693)
- Real CPU cost of `to_media_metadata()` is ~3µs, not the ~0.1-1ms implied
- **Bug found**: Default template references `album_name` but field is `album` — large_text always falls through to title
- Approach A (lazy): NOT FEASIBLE (Handlebars eager serialization)
- Approach B (early-diff): FEASIBLE but = Optimization #1
- Approach C (slim construct): Technically possible but sub-microsecond savings not worth complexity
- **Recommendation**: Downgrade impact from Medium to Low; fix album_name bug
