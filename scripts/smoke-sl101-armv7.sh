#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TARGET=armv7-unknown-linux-musleabihf
TARGET_DIR=${CARGO_TARGET_DIR:-"$ROOT/target"}
RELEASE="$TARGET_DIR/$TARGET/release"
SSH_TARGET=${1:-${SL101_SSH_TARGET:-root@192.168.23.106}}
SSH=${SSH:-ssh}
SCP=${SCP:-scp}

ssh_run() {
    "$SSH" -o BatchMode=yes -o ConnectTimeout=8 "$SSH_TARGET" "$@"
}

ARCH=$(ssh_run "uname -m")
case "$ARCH" in
    armv7*) ;;
    *)
        echo "SL101_DEVICE_SMOKE=FAIL reason=unexpected-arch arch=$ARCH" >&2
        exit 1
        ;;
esac

for binary in nuraloumi-panel nuraloumi-menu nuraloumi-probe; do
    local_path="$RELEASE/$binary"
    if [ ! -x "$local_path" ]; then
        echo "SL101_DEVICE_SMOKE=PENDING reason=missing-binary path=$local_path" >&2
        exit 2
    fi
    host_sha=$(sha256sum "$local_path" | awk '{print $1}')
    short_sha=$(printf '%s' "$host_sha" | cut -c1-16)
    remote_path="/tmp/$binary-$short_sha"
    "$SCP" -q -o BatchMode=yes "$local_path" "$SSH_TARGET:$remote_path"
    remote_sha=$(ssh_run "sha256sum '$remote_path' | awk '{print \$1}'")
    if [ "$host_sha" != "$remote_sha" ]; then
        ssh_run "rm -f '$remote_path'" >/dev/null 2>&1 || true
        echo "SL101_DEVICE_SMOKE=FAIL reason=hash-mismatch binary=$binary" >&2
        exit 1
    fi
    if ! ssh_run "chmod 755 '$remote_path'; '$remote_path' --help >/dev/null; rc=\$?; rm -f '$remote_path'; exit \$rc"; then
        ssh_run "rm -f '$remote_path'" >/dev/null 2>&1 || true
        echo "SL101_DEVICE_SMOKE=FAIL reason=exec-failed binary=$binary" >&2
        exit 1
    fi
    echo "SL101_DEVICE_BINARY=PASS name=$binary sha256=$host_sha no-sigill-help=yes"
done

echo "SL101_DEVICE_SMOKE=PASS target=$SSH_TARGET arch=$ARCH mode=copy-only-help"
