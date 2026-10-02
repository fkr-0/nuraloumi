#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TARGET=armv7-unknown-linux-musleabihf
SYSROOT=${NURALOUMI_SL101_SYSROOT:-"$ROOT/target/sl101-cross/sysroot"}
CLANG=${NURALOUMI_ARMV7_CLANG:-/usr/bin/clang}
RUSTC=${RUSTC:-rustc}

if [ ! -x "$CLANG" ]; then
    echo "armv7-sl101-linker: clang is unavailable at $CLANG" >&2
    exit 127
fi
if [ ! -f "$SYSROOT/lib/ld-musl-armhf.so.1" ]; then
    echo "armv7-sl101-linker: missing SL101 sysroot at $SYSROOT" >&2
    echo "run scripts/prepare-sl101-sysroot.sh first" >&2
    exit 2
fi

RUST_SYSROOT=$("$RUSTC" --print sysroot)
SELF_CONTAINED="$RUST_SYSROOT/lib/rustlib/$TARGET/lib/self-contained"
if [ ! -f "$SELF_CONTAINED/crt1.o" ]; then
    echo "armv7-sl101-linker: Rust target $TARGET is not installed" >&2
    exit 2
fi

exec "$CLANG" \
    --target="$TARGET" \
    -march=armv7-a \
    -mfpu=vfpv3-d16 \
    -mfloat-abi=hard \
    -fuse-ld=lld \
    --sysroot="$SYSROOT" \
    -B"$SELF_CONTAINED" \
    -L"$SELF_CONTAINED" \
    -L"$SYSROOT/lib" \
    -L"$SYSROOT/usr/lib" \
    -Wl,-rpath-link,"$SYSROOT/lib" \
    -Wl,-rpath-link,"$SYSROOT/usr/lib" \
    -Wl,--dynamic-linker=/lib/ld-musl-armhf.so.1 \
    "$@"
