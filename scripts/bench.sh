#!/usr/bin/env bash
# Benchmark harness for mprisence event-driven vs polling A/B comparison.
#
# Usage:
#     scripts/bench.sh <LABEL> <MPRISENCE_BIN> [DURATION_SEC]
#
# - Runs the binary for DURATION_SEC (default 300).
# - Samples /proc/$PID/{stat,status,task,fd} every 1s (no pidstat dependency).
# - Captures dbus-monitor output filtered to the org.mpris.MediaPlayer2 namespace.
# - Drives a reproducible Elisa workload via playerctl (pause/play/seek/next).
# - Writes results into bench/<LABEL>-<EPOCH>/.
#
# The binary is expected to honour ~/.config/mprisence/config.toml; toggle
# event_driven there between runs.

set -uo pipefail

LABEL="${1:?usage: bench.sh <LABEL> <BIN> [DURATION_SEC]}"
BIN="${2:?usage: bench.sh <LABEL> <BIN> [DURATION_SEC]}"
DURATION="${3:-300}"
PLAYER="${PLAYER:-elisa}"

if ! command -v playerctl >/dev/null; then
    echo "playerctl missing — install playerctl" >&2; exit 1
fi
if ! command -v dbus-monitor >/dev/null; then
    echo "dbus-monitor missing — install dbus" >&2; exit 1
fi
if [[ ! -x "$BIN" ]]; then
    echo "binary not executable: $BIN" >&2; exit 1
fi

if ! playerctl --player="$PLAYER" status >/dev/null 2>&1; then
    echo "player '$PLAYER' not running — start Elisa and play a track" >&2; exit 1
fi

OUT="bench/${LABEL}-$(date +%s)"
mkdir -p "$OUT"

echo "==> bench label=$LABEL bin=$BIN duration=${DURATION}s out=$OUT"

# 1. Start mprisence (info-level log is enough to time Discord pushes).
RUST_LOG="${RUST_LOG:-mprisence=info}" "$BIN" >"$OUT/mprisence.log" 2>&1 &
MPRI_PID=$!
echo "$MPRI_PID" >"$OUT/mprisence.pid"
trap 'kill $MPRI_PID 2>/dev/null; kill $DBUS_PID 2>/dev/null; kill $SAMPLER_PID 2>/dev/null' EXIT
sleep 1
if ! kill -0 "$MPRI_PID" 2>/dev/null; then
    echo "mprisence exited immediately — see $OUT/mprisence.log" >&2; exit 1
fi
echo "mprisence pid=$MPRI_PID"

# 2. dbus-monitor — captures every signal/method on the MPRIS interface.
dbus-monitor --session \
    "interface='org.mpris.MediaPlayer2.Player'" \
    "interface='org.freedesktop.DBus.Properties',path='/org/mpris/MediaPlayer2'" \
    >"$OUT/dbus.log" 2>&1 &
DBUS_PID=$!

# 3. Per-second sampler for cpu/rss/threads/fds, no external deps.
(
    CLK=$(getconf CLK_TCK)
    prev_total=0; prev_proc=0; first=1
    echo "ts,cpu_pct,rss_kb,vsz_kb,threads,fds,utime,stime,starttime" >"$OUT/proc.csv"
    while kill -0 "$MPRI_PID" 2>/dev/null; do
        ts=$(date +%s)
        if [[ ! -r "/proc/$MPRI_PID/stat" ]]; then break; fi
        # /proc/<pid>/stat — utime,stime,starttime are fields 14,15,22
        read -r -a S <"/proc/$MPRI_PID/stat"
        utime="${S[13]}"; stime="${S[14]}"; starttime="${S[21]}"
        proc_total=$((utime + stime))
        cpu_total=$(awk '/^cpu / {s=$2+$3+$4+$5+$6+$7+$8; print s; exit}' /proc/stat)
        if (( first )); then
            cpu_pct=0; first=0
        else
            d_proc=$((proc_total - prev_proc))
            d_total=$((cpu_total - prev_total))
            if (( d_total > 0 )); then
                cpu_pct=$(awk -v p=$d_proc -v t=$d_total 'BEGIN{printf "%.2f", 100.0*p/t}')
            else
                cpu_pct=0
            fi
        fi
        prev_total=$cpu_total; prev_proc=$proc_total
        rss_kb=$(awk '/^VmRSS:/{print $2}' "/proc/$MPRI_PID/status" 2>/dev/null || echo 0)
        vsz_kb=$(awk '/^VmSize:/{print $2}' "/proc/$MPRI_PID/status" 2>/dev/null || echo 0)
        threads=$(ls "/proc/$MPRI_PID/task" 2>/dev/null | wc -l)
        fds=$(ls "/proc/$MPRI_PID/fd" 2>/dev/null | wc -l)
        echo "$ts,$cpu_pct,$rss_kb,$vsz_kb,$threads,$fds,$utime,$stime,$starttime" >>"$OUT/proc.csv"
        sleep 1
    done
) &
SAMPLER_PID=$!

# 4. Reproducible workload via playerctl (relative to start_ts).
START_TS=$(date +%s)
echo "$START_TS" >"$OUT/start_ts"

trigger() {
    local epoch="$1"; shift
    echo "$epoch event $*" >>"$OUT/triggers.log"
    "$@" >>"$OUT/triggers.log" 2>&1
}

# Schedule (sleep until offset, then act). All offsets relative to START_TS.
# Events fired at 1/3 mark + 10s gaps, leaving idle bookends on either side.
sleep_until() {
    local target=$(( START_TS + $1 ))
    local now=$(date +%s)
    local delta=$(( target - now ))
    (( delta > 0 )) && sleep "$delta"
}

T_PAUSE=$(( DURATION / 3 ))
T_PLAY=$(( T_PAUSE + 10 ))
T_SEEK=$(( T_PAUSE + 20 ))
T_NEXT=$(( T_PAUSE + 30 ))

sleep_until "$T_PAUSE"; trigger "$(date +%s)" playerctl --player="$PLAYER" pause
sleep_until "$T_PLAY";  trigger "$(date +%s)" playerctl --player="$PLAYER" play
sleep_until "$T_SEEK";  trigger "$(date +%s)" playerctl --player="$PLAYER" position 30+
sleep_until "$T_NEXT";  trigger "$(date +%s)" playerctl --player="$PLAYER" next

# 5. Wait out the rest of the run.
sleep_until "$DURATION"

# 6. Tear down cleanly.
kill "$MPRI_PID" 2>/dev/null
kill "$DBUS_PID" 2>/dev/null
sleep 1
kill -9 "$MPRI_PID" 2>/dev/null
kill -9 "$DBUS_PID" 2>/dev/null
wait "$SAMPLER_PID" 2>/dev/null
trap - EXIT

# 7. Quick summary so the operator sees something useful immediately.
{
    echo "label=$LABEL"
    echo "bin=$BIN"
    echo "duration_sec=$DURATION"
    echo "player=$PLAYER"
    echo "samples=$(($(wc -l <"$OUT/proc.csv") - 1))"
    if [[ -s "$OUT/proc.csv" ]]; then
        awk -F, 'NR>1 {
            if ($2+0 > peak_cpu) peak_cpu=$2+0;
            cpu_sum+=$2+0; cpu_n++;
            if ($3+0 > peak_rss) peak_rss=$3+0;
            rss_sum+=$3+0; rss_n++;
            if ($5+0 > peak_th) peak_th=$5+0;
            if ($6+0 > peak_fd) peak_fd=$6+0;
        } END {
            printf "cpu_avg=%.2f%% cpu_peak=%.2f%%\n", (cpu_n?cpu_sum/cpu_n:0), peak_cpu;
            printf "rss_avg=%.1fMB rss_peak=%.1fMB\n", (rss_n?rss_sum/rss_n/1024:0), peak_rss/1024;
            printf "threads_peak=%d fds_peak=%d\n", peak_th, peak_fd;
        }' "$OUT/proc.csv"
    fi
    echo "dbus_total_lines=$(wc -l <"$OUT/dbus.log")"
    echo "dbus_signal_lines=$(grep -c '^signal' "$OUT/dbus.log" 2>/dev/null || echo 0)"
    echo "dbus_method_call_lines=$(grep -c '^method call' "$OUT/dbus.log" 2>/dev/null || echo 0)"
    echo "discord_updates=$(grep -c 'Updated Discord activity' "$OUT/mprisence.log" 2>/dev/null || echo 0)"
    echo "discord_clears=$(grep -c 'Clearing Discord activity' "$OUT/mprisence.log" 2>/dev/null || echo 0)"
} | tee "$OUT/summary.txt"

echo "==> done. data in $OUT"
