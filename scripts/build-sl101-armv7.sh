#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TARGET=armv7-unknown-linux-musleabihf
CROSS_ROOT=${NURALOUMI_SL101_CROSS_ROOT:-"$ROOT/target/sl101-cross"}
SYSROOT=${NURALOUMI_SL101_SYSROOT:-"$CROSS_ROOT/sysroot"}
PKGCONFIG=${NURALOUMI_SL101_PKGCONFIG:-"$CROSS_ROOT/pkgconfig"}
TARGET_DIR=${CARGO_TARGET_DIR:-"$ROOT/target"}
PREPARE=0
SSH_TARGET=${SL101_SSH_TARGET:-root@192.168.23.106}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --prepare)
            PREPARE=1
            shift
            ;;
        --ssh)
            [ "$#" -ge 2 ] || { echo "--ssh requires TARGET" >&2; exit 2; }
            SSH_TARGET=$2
            shift 2
            ;;
        -h|--help)
            cat <<EOF
Usage: scripts/build-sl101-armv7.sh [--prepare] [--ssh user@host]

Builds nuraloumi-panel, nuraloumi-menu and nuraloumi-probe for the real
SL101 musl ARMv7 ABI. --prepare refreshes the generated runtime sysroot from
the tablet first. Build output remains under target/ and is never committed.
EOF
            exit 0
            ;;
        *)
            echo "unknown argument: $1" >&2
            exit 2
            ;;
    esac
done

if [ "$PREPARE" -eq 1 ]; then
    "$ROOT/scripts/prepare-sl101-sysroot.sh" "$SSH_TARGET"
fi

for required in \
    "$SYSROOT/lib/ld-musl-armhf.so.1" \
    "$SYSROOT/usr/lib/libcairo.so" \
    "$SYSROOT/usr/lib/libgcc_s.so" \
    "$SYSROOT/usr/lib/Scrt1.o" \
    "$SYSROOT/usr/lib/crti.o" \
    "$SYSROOT/usr/lib/crtn.o" \
    "$PKGCONFIG/cairo.pc"; do
    if [ ! -e "$required" ]; then
        echo "SL101_CROSS_BUILD=PENDING reason=sysroot-not-prepared missing=$required" >&2
        echo "run scripts/build-sl101-armv7.sh --prepare" >&2
        exit 2
    fi
done

if ! rustup target list --installed | grep -Fxq "$TARGET"; then
    echo "SL101_CROSS_BUILD=PENDING reason=rust-target-not-installed target=$TARGET" >&2
    exit 2
fi

export NURALOUMI_SL101_SYSROOT="$SYSROOT"
export CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_LINKER="$ROOT/scripts/armv7-sl101-linker.sh"
export PKG_CONFIG_ALLOW_CROSS=1
export PKG_CONFIG_SYSROOT_DIR="$SYSROOT"
export PKG_CONFIG_LIBDIR="$PKGCONFIG"
export PKG_CONFIG_PATH="$PKGCONFIG"
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }-C target-feature=-neon,-d32,-hwdiv,-hwdiv-arm,-crt-static"
# Keep ARM mapping symbols ($a/$t/$d) so llvm-objdump can distinguish ARM,
# Thumb and inline data. Full symbol stripping makes no-NEON disassembly
# heuristics produce false positives; stripping debug info does not affect RSS.
export CARGO_PROFILE_RELEASE_STRIP=debuginfo

cargo build --locked --release --target "$TARGET" \
    -p nuraloumi-shell --bins \
    -p nuraloumi-providers --bin nuraloumi-probe

RELEASE="$TARGET_DIR/$TARGET/release"
sh "$ROOT/scripts/inspect-armv7-elf.sh" \
    "$RELEASE/nuraloumi-panel" \
    "$RELEASE/nuraloumi-menu" \
    "$RELEASE/nuraloumi-probe"

for binary in nuraloumi-panel nuraloumi-menu nuraloumi-probe; do
    path="$RELEASE/$binary"
    printf 'SL101_BINARY=PASS name=%s sha256=%s file=' "$binary" "$(sha256sum "$path" | awk '{print $1}')"
    file -b "$path"
done

echo "SL101_CROSS_BUILD=PASS target=$TARGET release=$RELEASE"
