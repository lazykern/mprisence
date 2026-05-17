#!/usr/bin/env bash
# Pure-idle variant of bench.sh — no playerctl actions. Measures the asymptotic
# D-Bus traffic floor when the player is just playing steadily with no user
# interaction. This is where event-driven mode should dominate polling.
#
# Usage: scripts/bench-idle.sh <LABEL> <MPRISENCE_BIN> [DURATION_SEC]

set -uo pipefail

LABEL="${1:?usage: bench-idle.sh <LABEL> <BIN> [DURATION_SEC]}"
BIN="${2:?usage: bench-idle.sh <LABEL> <BIN> [DURATION_SEC]}"
DURATION="${3:-60}"
PLAYER="${PLAYER:-elisa}"

if ! playerctl --player="$PLAYER" status >/dev/null 2>&1; then
    echo "player '$PLAYER' not running" >&2; exit 1
fi
[[ -x "$BIN" ]] || { echo "binary not executable: $BIN" >&2; exit 1; }

OUT="bench/${LABEL}-$(date +%s)"
mkdir -p "$OUT"
echo "==> idle bench label=$LABEL bin=$BIN duration=${DURATION}s out=$OUT"

RUST_LOG="${RUST_LOG:-mprisence=info}" "$BIN" >"$OUT/mprisence.log" 2>&1 &
MPRI_PID=$!
echo "$MPRI_PID" >"$OUT/mprisence.pid"
trap 'kill $MPRI_PID 2>/dev/null; kill $DBUS_PID 2>/dev/null' EXIT
sleep 1
kill -0 "$MPRI_PID" 2>/dev/null || { echo "mprisence died — see $OUT/mprisence.log" >&2; exit 1; }

dbus-monitor --session \
    "interface='org.mpris.MediaPlayer2.Player'" \
    "interface='org.freedesktop.DBus.Properties',path='/org/mpris/MediaPlayer2'" \
    >"$OUT/dbus.log" 2>&1 &
DBUS_PID=$!

sleep "$DURATION"

kill "$MPRI_PID" 2>/dev/null
kill "$DBUS_PID" 2>/dev/null
sleep 1
trap - EXIT

{
    echo "label=$LABEL"
    echo "bin=$BIN"
    echo "duration_sec=$DURATION"
    echo "dbus_total_lines=$(wc -l <"$OUT/dbus.log")"
    echo "dbus_signal_lines=$(grep -c '^signal' "$OUT/dbus.log" 2>/dev/null || echo 0)"
    echo "dbus_method_call_lines=$(grep -c '^method call' "$OUT/dbus.log" 2>/dev/null || echo 0)"
    echo "discord_updates=$(grep -c 'Updated Discord activity' "$OUT/mprisence.log" 2>/dev/null || echo 0)"
} | tee "$OUT/summary.txt"
