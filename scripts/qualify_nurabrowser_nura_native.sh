#!/bin/sh
# Owner-authorized, bounded ARMv7 WPE/Wayland qualification on sl101-nura.
# Run on Nura via: sh -s -- URL HOLD_SECONDS [navigation|soak] < this_script
# This DOES NOT install/activate the candidate. A passing window is not a
# release sign-off: UI/Web/Network policies require independent review.
set -eu

url=${1:-https://example.org/}
hold=${2:-90}
mode=${3:-navigation}
case "$mode" in
    navigation|soak) ;;
    *) echo "QUAL_FAIL invalid mode" >&2; exit 64 ;;
esac
case "$url" in
    https://*|http://*) ;;
    *) echo "QUAL_FAIL invalid URL" >&2; exit 64 ;;
esac
case "$hold" in
    ''|*[!0-9]*) echo "QUAL_FAIL invalid hold" >&2; exit 64 ;;
esac
[ "$hold" -ge 20 ] && [ "$hold" -le 1800 ] ||
    { echo "QUAL_FAIL hold out of bounds" >&2; exit 64; }
[ "$(id -u)" -eq 10000 ] && [ "$(uname -m)" = armv7l ] &&
    [ "$(hostname)" = sl101-nura ] ||
    { echo "QUAL_FAIL wrong device/user" >&2; exit 65; }
base=/home/user/.local/state/nura-sandbox-qualification-20261127
root=${NURA_QUAL_ROOT:-$base/runtime-root}
# Private CI media candidates may be exercised in their own immutable test
# roots. Never accept an arbitrary root, symlink pivot or production path.
if [ "$root" != "$base/runtime-root" ]; then
    case "$root" in
        /home/user/.local/state/nura-media-60s-*/runtime-root) ;;
        *) echo "QUAL_FAIL private runtime path not allowed" >&2; exit 65 ;;
    esac
    case "$root" in
        *..*|*//*)
            echo "QUAL_FAIL unsafe runtime path" >&2; exit 65 ;;
    esac
    test -d "$root" && test "$(readlink -f "$root")" = "$root" ||
        { echo "QUAL_FAIL private runtime root unresolved" >&2; exit 65; }
fi
case ${NURA_QUAL_EGL_TRACE:-0} in
    0) egl_log_level=warning; libgl_debug=quiet ;;
    1) egl_log_level=debug; libgl_debug=verbose ;;
    *) echo "QUAL_FAIL invalid EGL trace mode" >&2; exit 64 ;;
esac
core="$root/usr/libexec/nuraloumi/browser/nurabrowser-core"
test -x "$core" &&
test -x "$root/usr/libexec/nuraloumi/browser/libexec/WPEWebProcess" &&
test -x "$root/usr/libexec/nuraloumi/browser/libexec/WPENetworkProcess" &&
test -f "$root/candidate/CANDIDATE-ONLY.txt" &&
test ! -e "$root/usr/libexec/nuraloumi/browser/runtime.json" &&
test -S /run/sl101-desktop-user/wayland-1 ||
    { echo "QUAL_FAIL unverified root or missing display" >&2; exit 65; }
test "$(pgrep -x sl101-sway | wc -l)" -eq 1 &&
test "$(pgrep -x wayvnc | wc -l)" -eq 1 ||
    { echo "QUAL_FAIL desktop baseline" >&2; exit 65; }

# Reuse the historical canary lock when it exists. After a reboot Alpine's
# /run/lock may be empty, so use an owned persistent fallback in that case.
if [ -r /run/lock/sl101-nura-qualification.lock ]; then
    exec 9</run/lock/sl101-nura-qualification.lock
else
    exec 9>"$base/qualification.lock"
fi
flock -n -x 9 || { echo "QUAL_FAIL lock busy" >&2; exit 75; }
if pgrep -x nurabrowser-cor >/dev/null ||
        pgrep -x WPEWebProcess >/dev/null ||
        pgrep -x WPENetworkProce >/dev/null; then
    echo "QUAL_FAIL existing WPE session" >&2
    exit 75
fi
mkdir -p "$base/runs"
out=$(mktemp -d "$base/runs/native-XXXXXX")
mkdir -m 0700 "$out/run" "$out/config" "$out/cache" "$out/data" "$out/state"
marker="nura-d16-native-$$"
production_before=$(readlink -f /opt/nuraloumi/current)
sway_before=$(pgrep -x sl101-sway)
vnc_before=$(pgrep -x wayvnc)
outer_pid=

capture() {
    printf 'CAPTURE_AT=%s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" >> "$out/proc.txt"
    for proc in /proc/[0-9]*; do
        pid=${proc##*/}
        [ -r "$proc/comm" ] || continue
        comm=$(cat "$proc/comm" 2>/dev/null || true)
        case "$comm" in
            nurabrowser-cor*|WPEWebProcess|WPENetworkProc*) ;;
            *) continue ;;
        esac
        [ "$(stat -c %u "$proc" 2>/dev/null || true)" = 10000 ] || continue
        if tr '\000' '\n' < "$proc/environ" 2>/dev/null |
                grep -Fxq "NURA_QUAL_MARK=$marker"; then
            echo "PROCESS pid=$pid comm=$comm" >> "$out/proc.txt"
            grep -E '^(Name|PPid|Uid|NoNewPrivs|Seccomp|Seccomp_filters):' \
                "$proc/status" >> "$out/proc.txt" || true
            for ns in user mnt pid net ipc uts; do
                echo "$ns $(readlink "$proc/ns/$ns" 2>/dev/null || true)" \
                    >> "$out/proc.txt"
            done
            cp "$proc/mountinfo" "$out/mountinfo-$pid.txt" 2>/dev/null || true
            cp "$proc/maps" "$out/maps-$pid.txt" 2>/dev/null || true
        fi
    done
}

sample_soak() {
    total=0
    ui=0
    web=0
    network=0
    sample_lines=
    for proc in /proc/[0-9]*; do
        pid=${proc##*/}
        [ -r "$proc/comm" ] || continue
        comm=$(cat "$proc/comm" 2>/dev/null || true)
        case "$comm" in
            nurabrowser-cor*) role=ui; ui=$((ui + 1)) ;;
            WPEWebProcess) role=web; web=$((web + 1)) ;;
            WPENetworkProc*) role=network; network=$((network + 1)) ;;
            *) continue ;;
        esac
        [ "$(stat -c %u "$proc" 2>/dev/null || true)" = 10000 ] || continue
        if ! tr '\000' '\n' < "$proc/environ" 2>/dev/null |
                grep -Fxq "NURA_QUAL_MARK=$marker"; then
            case "$role" in
                ui) ui=$((ui - 1)) ;;
                web) web=$((web - 1)) ;;
                network) network=$((network - 1)) ;;
            esac
            continue
        fi
        rss=$(awk '/^VmRSS:/ {print $2; exit}' "$proc/status")
        rss=${rss:-0}
        total=$((total + rss))
        sample_lines="$sample_lines $role:$pid:$rss"
    done
    cpu=$(cat /sys/class/thermal/thermal_zone1/temp)
    skin=$(cat /sys/class/thermal/thermal_zone0/temp)
    available=$(awk '/^MemAvailable:/ {print $2; exit}' /proc/meminfo)
    timestamp=$(date -u +%FT%TZ)
    echo "$timestamp ui=$ui web=$web network=$network rss_kib=$total available_kib=$available cpu_mC=$cpu skin_mC=$skin$sample_lines" \
        >> "$out/soak-samples.log"
    if [ "$ui" -ne 1 ] || [ "$web" -lt 1 ] || [ "$network" -lt 1 ] ||
            [ "$total" -ge 716800 ] || [ "$available" -le 100000 ] ||
            [ "$cpu" -ge 70000 ] || [ "$skin" -ge 60000 ]; then
        echo "SOAK_ABORT unsafe process/RSS/memory/thermal threshold at $timestamp" >&2
        return 1
    fi
}

stop_owned_processes() {
    for proc in /proc/[0-9]*; do
        pid=${proc##*/}
        [ -r "$proc/comm" ] || continue
        comm=$(cat "$proc/comm" 2>/dev/null || true)
        case "$comm" in
            nurabrowser-cor*|WPEWebProcess|WPENetworkProc*) ;;
            *) continue ;;
        esac
        [ "$(stat -c %u "$proc" 2>/dev/null || true)" = 10000 ] || continue
        if tr '\000' '\n' < "$proc/environ" 2>/dev/null |
                grep -Fxq "NURA_QUAL_MARK=$marker"; then
            kill -TERM "$pid" 2>/dev/null || true
        fi
    done
}

cleanup() {
    trap - EXIT HUP INT TERM
    capture || true
    stop_owned_processes || true
    if [ -n "$outer_pid" ]; then
        kill -TERM "$outer_pid" 2>/dev/null || true
        wait "$outer_pid" 2>/dev/null || true
    fi
    echo "PRODUCTION_UNCHANGED=$([ "$(readlink -f /opt/nuraloumi/current)" = "$production_before" ] && echo yes || echo no)"
    echo "SWAY_UNCHANGED=$([ "$(pgrep -x sl101-sway)" = "$sway_before" ] && echo yes || echo no)"
    echo "WAYVNC_UNCHANGED=$([ "$(pgrep -x wayvnc)" = "$vnc_before" ] && echo yes || echo no)"
    echo "QUAL_EVIDENCE=$out"
}
trap cleanup EXIT HUP INT TERM
echo "QUAL_START=$out"
echo "QUAL_MARK=$marker"
echo "QUAL_RUNTIME_ROOT=$root"

# The outer role gets a private conventional XDG_RUNTIME_DIR and only the
# named compositor socket. No host /home, /run/sl101-desktop-user or session
# D-Bus socket is present. WebKit's own WebProcess bwrap may nest inside it;
# NetworkProcess isolation MUST be evaluated independently.
# shellcheck disable=SC2016 # The embedded script expands inside the container.
bwrap --die-with-parent --unshare-user --unshare-pid --unshare-uts --unshare-ipc \
    --ro-bind "$root" / --ro-bind /sys /sys \
    --ro-bind /etc/resolv.conf /etc/resolv.conf \
    --proc /proc --dev /dev --tmpfs /tmp --tmpfs /run \
    --dir /run/user --dir /run/user/10000 \
    --bind "$out/run" /run/user/10000 \
    --ro-bind /run/sl101-desktop-user/wayland-1 /run/user/10000/wayland-0 \
    --dir /run/qualification \
    --bind "$out/config" /run/qualification/config \
    --bind "$out/cache" /run/qualification/cache \
    --bind "$out/data" /run/qualification/data \
    --bind "$out/state" /run/qualification/state \
    --clearenv \
    --setenv PATH /usr/bin:/bin:/usr/sbin:/sbin \
    --setenv HOME /run/qualification \
    --setenv XDG_RUNTIME_DIR /run/user/10000 \
    --setenv WAYLAND_DISPLAY wayland-0 \
    --setenv WPE_PLATFORM wayland \
    --setenv WEBKIT_USE_SANDBOX 1 \
    --setenv WEBKIT_EXEC_PATH /usr/libexec/nuraloumi/browser/libexec \
    --setenv XDG_CONFIG_HOME /run/qualification/config \
    --setenv XDG_CACHE_HOME /run/qualification/cache \
    --setenv XDG_DATA_HOME /run/qualification/data \
    --setenv XDG_STATE_HOME /run/qualification/state \
    --setenv LIBGL_ALWAYS_SOFTWARE 1 \
    --setenv EGL_LOG_LEVEL "$egl_log_level" \
    --setenv LIBGL_DEBUG "$libgl_debug" \
    --setenv GALLIUM_DRIVER softpipe \
    --setenv MESA_SHADER_CACHE_DISABLE true \
    --setenv OPENSSL_armcap 0 \
    --setenv NETTLE_FAT_OVERRIDE '' \
    --setenv NURA_QUAL_MARK "$marker" \
    /bin/sh -c '
        core=/usr/libexec/nuraloumi/browser/nurabrowser-core
        "$core" --platform=wayland --url="$1" &
        browser_pid=$!
        if [ "$2" = soak ]; then
            sleep 9
            while kill -0 "$browser_pid" 2>/dev/null; do
                "$core" --remote uri >> /run/qualification/state/browser-state.jsonl ||
                    exit 1
                "$core" --remote tabs >> /run/qualification/state/browser-state.jsonl ||
                    exit 1
                sleep 10
            done &
            monitor_pid=$!
            wait "$browser_pid"
            result=$?
            kill "$monitor_pid" 2>/dev/null || true
            exit "$result"
        fi
        sleep 23
        "$core" --remote uri || echo "QUAL_REMOTE_INITIAL_FAIL"
        "$core" --remote open https://www.tagesschau.de/ ||
            echo "QUAL_REMOTE_OPEN_FAIL"
        sleep 17
        "$core" --remote uri || echo "QUAL_REMOTE_SECOND_FAIL"
        "$core" --remote back || echo "QUAL_REMOTE_BACK_FAIL"
        sleep 5
        "$core" --remote forward || echo "QUAL_REMOTE_FORWARD_FAIL"
        wait "$browser_pid"
    ' nura-native-driver "$url" "$mode" > "$out/engine.log" 2>&1 9<&- &
outer_pid=$!
sleep 15
if ! kill -0 "$outer_pid" 2>/dev/null; then
    echo "QUAL_FAIL early browser exit"
    tail -n 50 "$out/engine.log"
    exit 1
fi
capture
XDG_RUNTIME_DIR=/run/sl101-desktop-user WAYLAND_DISPLAY=wayland-1 \
    grim -o LVDS-1 "$out/wayland-first.png" 2>"$out/grim.log" ||
    echo "QUAL_WARN compositor screenshot failed"
echo "QUAL_READY=$out"
echo "QUAL_WINDOW_SECONDS=$hold"
if [ "$mode" = soak ]; then
    soak_start=$(date +%s)
    samples=0
    while :; do
        sample_soak
        samples=$((samples + 1))
        elapsed=$(($(date +%s) - soak_start))
        if [ $((samples % 6)) -eq 0 ]; then
            echo "SOAK_HEARTBEAT seconds=$elapsed samples=$samples"
        fi
        [ "$elapsed" -ge "$hold" ] && break
        sleep 10
    done
    echo "SOAK_SAMPLES=$samples"
else
    sleep "$hold"
fi
capture
XDG_RUNTIME_DIR=/run/sl101-desktop-user WAYLAND_DISPLAY=wayland-1 \
    grim -o LVDS-1 "$out/wayland-last.png" 2>>"$out/grim.log" || true
echo "QUAL_HOLD_COMPLETED=$hold"
