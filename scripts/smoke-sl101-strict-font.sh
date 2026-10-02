#!/bin/sh
set -eu

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
TARGET=armv7-unknown-linux-musleabihf
CROSS_ROOT=${NURALOUMI_SL101_CROSS_ROOT:-"$ROOT/target/sl101-cross"}
SYSROOT=${NURALOUMI_SL101_SYSROOT:-"$CROSS_ROOT/sysroot"}
PKGCONFIG=${NURALOUMI_SL101_PKGCONFIG:-"$CROSS_ROOT/pkgconfig"}
TARGET_DIR=${CARGO_TARGET_DIR:-"$ROOT/target"}
EVIDENCE_DIR=${NURALOUMI_SL101_EVIDENCE_DIR:-"$ROOT/target/sl101-evidence"}
SSH_TARGET=${2:-${SL101_SSH_TARGET:-root@192.168.23.106}}
SSH=${SSH:-ssh}
SCP=${SCP:-scp}

FONT=${1:-}
if [ -z "$FONT" ] || [ "$FONT" = "-h" ] || [ "$FONT" = "--help" ]; then
    cat <<EOF
Usage: scripts/smoke-sl101-strict-font.sh FONT_FILE [user@host]

Cross-build the strict packaged-font evidence renderer, copy it and FONT_FILE
temporarily to the SL101, render mixed LTR/RTL text, and pull the resulting PNG
to target/sl101-evidence/strict-font.png. FONT_FILE is never installed.
EOF
    if [ -z "$FONT" ]; then
        exit 2
    fi
    exit 0
fi

if [ ! -f "$FONT" ]; then
    echo "SL101_STRICT_FONT=FAIL reason=font-missing path=$FONT" >&2
    exit 1
fi

for required in \
    "$SYSROOT/lib/ld-musl-armhf.so.1" \
    "$SYSROOT/usr/lib/Scrt1.o" \
    "$SYSROOT/usr/lib/libfreetype.so" \
    "$SYSROOT/usr/lib/libharfbuzz.so" \
    "$SYSROOT/usr/lib/libfribidi.so" \
    "$SYSROOT/usr/lib/libcairo.so" \
    "$PKGCONFIG/freetype2.pc" \
    "$PKGCONFIG/harfbuzz.pc" \
    "$PKGCONFIG/fribidi.pc" \
    "$PKGCONFIG/cairo.pc"; do
    if [ ! -e "$required" ]; then
        echo "SL101_STRICT_FONT=PENDING reason=sysroot-incomplete missing=$required" >&2
        echo "run scripts/prepare-sl101-sysroot.sh $SSH_TARGET" >&2
        exit 2
    fi
done

ssh_run() {
    "$SSH" -o BatchMode=yes -o ConnectTimeout=8 "$SSH_TARGET" "$@"
}

ARCH=$(ssh_run "uname -m")
case "$ARCH" in
    armv7*) ;;
    *)
        echo "SL101_STRICT_FONT=FAIL reason=unexpected-arch arch=$ARCH" >&2
        exit 1
        ;;
esac

export NURALOUMI_SL101_SYSROOT="$SYSROOT"
export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_LINKER="$ROOT/scripts/armv7-sl101-linker.sh"
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_SYSROOT_DIR="$SYSROOT"
export PKG_CONFIG_LIBDIR="$PKGCONFIG"
export PKG_CONFIG_PATH="$PKGCONFIG"
# Cargo config provides checkout-relative target defaults for raw cargo checks.
# Explicit target-scoped environment variables win over those defaults so this
# script remains relocatable when a prepared sysroot lives outside the checkout.
export PKG_CONFIG_ALLOW_CROSS_armv7_unknown_linux_musleabihf=1
export PKG_CONFIG_SYSROOT_DIR_armv7_unknown_linux_musleabihf="$SYSROOT"
export PKG_CONFIG_LIBDIR_armv7_unknown_linux_musleabihf="$PKGCONFIG"
export PKG_CONFIG_PATH_armv7_unknown_linux_musleabihf="$PKGCONFIG"
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C target-feature=-neon,-d32,-hwdiv,-hwdiv-arm,-crt-static"
export CARGO_PROFILE_RELEASE_STRIP=debuginfo

cargo build --locked --release --target "$TARGET" \
    -p nuraloumi-render-cairo \
    --example render_strict_font \
    --features packaged-font

BIN="$TARGET_DIR/$TARGET/release/examples/render_strict_font"
sh "$ROOT/scripts/inspect-armv7-elf.sh" "$BIN"

mkdir -p "$EVIDENCE_DIR"
BIN_SHA=$(sha256sum "$BIN" | cut -d' ' -f1)
FONT_SHA=$(sha256sum "$FONT" | cut -d' ' -f1)
SHORT_SHA=$(printf '%s' "$BIN_SHA" | cut -c1-16)
REMOTE_BIN="/tmp/nuraloumi-strict-font-$SHORT_SHA"
REMOTE_FONT="/tmp/nuraloumi-strict-font-$SHORT_SHA.ttf"
REMOTE_PNG="/tmp/nuraloumi-strict-font-$SHORT_SHA.png"
LOCAL_PNG="$EVIDENCE_DIR/strict-font.png"
DEVICE_LOG="$EVIDENCE_DIR/strict-font-device.log"

cleanup_remote() {
    ssh_run "rm -f '$REMOTE_BIN' '$REMOTE_FONT' '$REMOTE_PNG'" >/dev/null 2>&1 || true
}
trap cleanup_remote EXIT HUP INT TERM

"$SCP" -q -o BatchMode=yes "$BIN" "$SSH_TARGET:$REMOTE_BIN"
"$SCP" -q -o BatchMode=yes "$FONT" "$SSH_TARGET:$REMOTE_FONT"

REMOTE_BIN_SHA=$(ssh_run "sha256sum '$REMOTE_BIN'" | cut -d' ' -f1)
REMOTE_FONT_SHA=$(ssh_run "sha256sum '$REMOTE_FONT'" | cut -d' ' -f1)
if [ "$BIN_SHA" != "$REMOTE_BIN_SHA" ]; then
    echo "SL101_STRICT_FONT=FAIL reason=binary-hash-mismatch" >&2
    exit 1
fi
if [ "$FONT_SHA" != "$REMOTE_FONT_SHA" ]; then
    echo "SL101_STRICT_FONT=FAIL reason=font-hash-mismatch" >&2
    exit 1
fi

ssh_run "set -eu
    chmod 755 '$REMOTE_BIN'
    '$REMOTE_BIN' '$REMOTE_FONT' '$REMOTE_PNG'
    sha256sum '$REMOTE_BIN' '$REMOTE_FONT' '$REMOTE_PNG'" |
    tee "$DEVICE_LOG"

"$SCP" -q -o BatchMode=yes "$SSH_TARGET:$REMOTE_PNG" "$LOCAL_PNG"
REMOTE_PNG_SHA=$(ssh_run "sha256sum '$REMOTE_PNG'" | cut -d' ' -f1)
LOCAL_PNG_SHA=$(sha256sum "$LOCAL_PNG" | cut -d' ' -f1)
if [ "$REMOTE_PNG_SHA" != "$LOCAL_PNG_SHA" ]; then
    echo "SL101_STRICT_FONT=FAIL reason=png-hash-mismatch" >&2
    exit 1
fi

printf 'SL101_STRICT_FONT=PASS target=%s arch=%s binary_sha256=%s font_sha256=%s png_sha256=%s evidence=%s\n' \
    "$SSH_TARGET" "$ARCH" "$BIN_SHA" "$FONT_SHA" "$LOCAL_PNG_SHA" "$LOCAL_PNG"

cleanup_remote
trap - EXIT HUP INT TERM
