#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
SYSROOT=${NURALOUMI_SL101_SYSROOT:-"$ROOT/target/sl101-cross/sysroot"}
CLANG=${NURALOUMI_ARMV7_CLANG:-/usr/bin/clang}

if [ ! -x "$CLANG" ]; then
    echo "armv7-sl101-linker: clang is unavailable at $CLANG" >&2
    exit 127
fi
if [ ! -f "$SYSROOT/lib/ld-musl-armhf.so.1" ]; then
    echo "armv7-sl101-linker: missing SL101 sysroot at $SYSROOT" >&2
    echo "run scripts/prepare-sl101-sysroot.sh first" >&2
    exit 2
fi

GCC_BASE="$SYSROOT/usr/lib/gcc/armv7-alpine-linux-musleabihf"
GCCDIR=
for candidate in "$GCC_BASE"/*; do
    [ -d "$candidate" ] || continue
    if [ -n "$GCCDIR" ]; then
        echo "armv7-sl101-linker: expected exactly one GCC support directory in the SL101 sysroot" >&2
        exit 2
    fi
    GCCDIR=$candidate
done
if [ -z "$GCCDIR" ]; then
    echo "armv7-sl101-linker: no GCC support directory found in the SL101 sysroot" >&2
    exit 2
fi

for required in     "$SYSROOT/usr/lib/Scrt1.o"     "$SYSROOT/usr/lib/crti.o"     "$SYSROOT/usr/lib/crtn.o"     "$GCCDIR/crtbeginS.o"     "$GCCDIR/crtendS.o"     "$GCCDIR/libgcc.a"; do
    if [ ! -e "$required" ]; then
        echo "armv7-sl101-linker: incomplete SL101 CRT/sysroot, missing $required" >&2
        exit 2
    fi
done

exec "$CLANG"     --target=armv7-alpine-linux-musleabihf     -march=armv7-a     -mfpu=vfpv3-d16     -mfloat-abi=hard     -fuse-ld=lld     --sysroot="$SYSROOT"     -B"$GCCDIR"     -B"$SYSROOT/usr/lib"     -L"$GCCDIR"     -L"$SYSROOT/usr/lib"     -L"$SYSROOT/lib"     -Wl,-rpath-link,"$SYSROOT/usr/lib"     -Wl,-rpath-link,"$SYSROOT/lib"     -Wl,--dynamic-linker=/lib/ld-musl-armhf.so.1     "$@"
