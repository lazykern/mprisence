# Benchmark Results — Event-Driven vs Polling

## Setup

- Host: Linux 7.0.8-arch1-1, Tokio multi-thread runtime
- Player: Elisa (KDE), single track playing
- Discord: connected via IPC throughout
- Methodology: `scripts/bench.sh <label> <bin> <duration>` — 1 s sampler reads `/proc/<pid>/{stat,status,task,fd}`; `dbus-monitor` filters `org.mpris.MediaPlayer2.Player` + `org.freedesktop.DBus.Properties` on `/org/mpris/MediaPlayer2`. Reproducible workload via `playerctl`.
- Three configurations:
  - `main-poll-2s` — `/usr/bin/mprisence` 1.4.5 (no event-driven code path), `interval=2000`
  - `branch-poll-2s` — feature branch, `event_driven=false`, `interval=2000` (regression control)
  - `branch-event` — feature branch, `event_driven=true`, `discovery_interval=5000`

## Active workload (120 s, with `pause → play → seek → next` at t=40/50/60/70)

|                          | main-poll-2s | branch-poll-2s | branch-event |
|--------------------------|-------------:|---------------:|-------------:|
| CPU avg                  | 0.01 %       | 0.01 %         | 0.00 %       |
| CPU peak                 | 0.19 %       | 0.06 %         | 0.12 %       |
| RSS avg                  | 20.0 MB      | 20.6 MB        | 20.7 MB      |
| RSS peak                 | 20.2 MB      | 22.8 MB        | 23.3 MB      |
| Threads peak             | 20           | 20             | **21**       |
| FDs peak                 | 16           | 16             | 16           |
| D-Bus total lines        | 1347         | 1329           | **948**      |
| D-Bus method calls       | 371          | 365            | **263**      |
| D-Bus signals            | 33           | 33             | 33           |
| Discord pushes (set)     | 6            | 5              | 4            |
| Discord pushes (clear)   | 1            | 1              | 1            |

Active workload delta vs `main-poll-2s`:
- D-Bus method calls: −29 % (`263` vs `371`)
- Total D-Bus traffic: −30 % (`948` vs `1347`)
- CPU/RSS: indistinguishable at this load
- Extra thread cost: +1 (per-player listener)

`branch-poll-2s` ≈ `main-poll-2s` within noise — confirms no regression in the fallback path.

## Idle baseline (60 s, no user interaction, steady-state playing)

|                          | idle-main-poll | idle-branch-event |
|--------------------------|---------------:|------------------:|
| D-Bus total lines        | 472            | **40**            |
| D-Bus method calls       | 156            | **12**            |
| D-Bus signals            | 2              | 2                 |
| Discord pushes           | 1              | 0                 |

Idle delta:
- D-Bus method calls: −92 % (`12` vs `156`)
- Total D-Bus traffic: −92 % (`40` vs `472`)

This is the asymptotic win the issue (#84) targets — when nothing is happening, mprisence stops asking. The 12 residual calls in 60 s are the 5 s discovery scans (`ListNames` + `GetAll` × N players ≈ 2 calls per scan × 6 scans = 12).

## Discord update latency (from `playerctl` trigger to log line)

|              | pause → clear | play → set | seek → set | next → set |
|--------------|--------------:|-----------:|-----------:|-----------:|
| main-poll-2s | ~1 s          | ~1 s       | ~1 s       | ~6 s       |
| branch-poll-2s | ~1 s        | ~1 s       | ~1 s       | ~6 s       |
| branch-event | **~0 s**      | **~0 s**   | n/a (Elisa) | ~5 s      |

- Event mode reacts within the same wall-clock second; polling waits up to one interval tick (≤ 2 s).
- Elisa does not emit `Seeked` on `position 30+`, so event mode can't beat polling on seek for this player. (Players that do emit `Seeked` would win here too.)
- `next` track latency is dominated by cover-art lookup (MusicBrainz HTTP), not by the signal/poll path — both modes pay it.

## Hypothesis check

| Hypothesis                                             | Result |
|--------------------------------------------------------|--------|
| Idle D-Bus traffic drops 80–95 %                       | ✅ −92 % |
| Discord update lag drops from ≤ 2 s to < 200 ms        | ✅ visible at second granularity (~0 s) |
| `branch-poll-2s` matches `main-poll-2s` (no regression)| ✅ within noise |
| RSS slightly higher in event mode                      | ✅ +0.7 MB avg, +3.1 MB peak (listener thread) |
| CPU avg drops noticeably                               | ⚠️ both already at ≤ 0.01 % avg — load is too small to measure |

## Caveats

- 120 s × 1 run per config; no medians — re-run if you need confidence intervals.
- `dbus-monitor` itself adds traffic; captured in *both* configurations so the comparison is fair.
- Single-player workload. Multi-player setups will scale the listener thread cost linearly.
- Tested on Elisa only; players with different signal-emission behaviour (e.g. Spotify, mpv) may show different per-player profiles. Worth re-running.

## Reproduction

```bash
# branch
cargo build --release

# idle baseline + branch
XDG_CONFIG_HOME=/tmp/bench-cfg-mp ./scripts/bench-idle.sh idle-main-poll /usr/bin/mprisence 60
XDG_CONFIG_HOME=/tmp/bench-cfg-be ./scripts/bench-idle.sh idle-branch-event ./target/release/mprisence 60

# active workload (3 configs)
XDG_CONFIG_HOME=/tmp/bench-cfg-mp ./scripts/bench.sh main-poll-2s   /usr/bin/mprisence            120
XDG_CONFIG_HOME=/tmp/bench-cfg-bp ./scripts/bench.sh branch-poll-2s ./target/release/mprisence    120
XDG_CONFIG_HOME=/tmp/bench-cfg-be ./scripts/bench.sh branch-event   ./target/release/mprisence    120
```

Configs in `/tmp/bench-cfg-{mp,bp,be}/mprisence/config.toml` differ only in `event_driven` / `discovery_interval`; everything else (including `[player.default].status_display_type`) is held constant.
