#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"

nested_only=0
if [ "${1:-}" = "--nested-only" ]; then
    nested_only=1
    shift
fi
if [ "$#" -ne 0 ]; then
    echo "usage: $0 [--nested-only]" >&2
    exit 2
fi

if [ "$nested_only" -eq 0 ]; then
    echo "==> independent fixture/menu host probes"
    cargo run -q -p nuraloumi-xtask -- qualify-host --no-nested
fi

if ! command -v weston >/dev/null 2>&1; then
    echo "NESTED_PIXMAN=SKIP reason=weston-not-installed"
    exit 0
fi

TMP=$(mktemp -d)
WESTON_PID=""
cleanup() {
    if [ -n "$WESTON_PID" ] && kill -0 "$WESTON_PID" 2>/dev/null; then
        kill "$WESTON_PID" 2>/dev/null || true
        wait "$WESTON_PID" 2>/dev/null || true
    fi
    rm -rf "$TMP"
}
trap cleanup EXIT INT TERM

mkdir -p "$TMP/runtime"
chmod 700 "$TMP/runtime"
SOCKET="nuraloumi-smoke-$$"
LOG="$TMP/weston.log"

echo "==> isolated nested compositor probe"
XDG_RUNTIME_DIR="$TMP/runtime" \
WAYLAND_DISPLAY="$SOCKET" \
weston \
    --backend=headless-backend.so \
    --renderer=pixman \
    --socket="$SOCKET" \
    --idle-time=0 \
    --log="$LOG" \
    >/dev/null 2>&1 &
WESTON_PID=$!

i=0
while [ "$i" -lt 50 ]; do
    if [ -S "$TMP/runtime/$SOCKET" ]; then
        break
    fi
    if ! kill -0 "$WESTON_PID" 2>/dev/null; then
        echo "NESTED_PIXMAN=FAIL reason=weston-exited-before-socket"
        sed -n '1,120p' "$LOG" 2>/dev/null || true
        exit 1
    fi
    i=$((i + 1))
    sleep 0.1
done

if [ ! -S "$TMP/runtime/$SOCKET" ]; then
    echo "NESTED_PIXMAN=FAIL reason=socket-timeout"
    sed -n '1,120p' "$LOG" 2>/dev/null || true
    exit 1
fi

if command -v wayland-info >/dev/null 2>&1 && command -v timeout >/dev/null 2>&1; then
    XDG_RUNTIME_DIR="$TMP/runtime" WAYLAND_DISPLAY="$SOCKET" \
        timeout 5 wayland-info >/dev/null
fi

echo "NESTED_PIXMAN=PASS socket=$SOCKET isolated_runtime=yes live_session_untouched=yes"
