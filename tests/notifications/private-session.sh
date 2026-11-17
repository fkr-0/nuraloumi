#!/bin/sh
# Private-session notification integration smoke. No live bus service takeover.
set -eu
cd "$(dirname "$0")/../.."
if [ "${1:-}" != "--inside" ]; then
    cargo build --offline -p nuraloumi-shell --bin nuraloumi-notificationd
    exec dbus-run-session -- /bin/sh "$0" --inside
fi
bin=./target/debug/nuraloumi-notificationd
log=$(mktemp)
conflict=$(mktemp)
"$bin" >"$log" 2>&1 &
daemon=$!
trap 'kill "$daemon" 2>/dev/null || :; wait "$daemon" 2>/dev/null || :; rm -f "$log" "$conflict"' EXIT HUP INT TERM
n=0
until busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications GetServerInformation >/dev/null 2>&1; do
    n=$((n+1))
    [ "$n" -lt 30 ] || { cat "$log"; exit 1; }
    sleep 0.1
done
notify-send --expire-time=30000 "NuraLoumi E2E" "private bus only"
busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications GetHistory | grep -q 'NuraLoumi E2E'
echo 'PASS private bus Notify/GetHistory'
if "$bin" >"$conflict" 2>&1; then
    echo 'FAIL duplicate daemon acquired name' >&2; exit 1
fi
grep -q 'name already taken' "$conflict"
echo 'PASS duplicate owner fail-closed'
busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications CloseNotification u 1
busctl --user call org.freedesktop.Notifications /org/freedesktop/Notifications org.freedesktop.Notifications GetHistory | grep -q 'false'
echo 'PASS CloseNotification retains inactive history'
echo 'PASS private session notifications smoke'
