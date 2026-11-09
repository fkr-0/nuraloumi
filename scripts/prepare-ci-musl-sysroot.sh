#!/bin/sh
# Prepare a signed, upstream-Alpine ARMv7 musl build sysroot for CI only.
# This never contacts an SL101 or touches the tablet-derived local sysroot.
set -eu

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
CROSS_ROOT=${NURALOUMI_SL101_CROSS_ROOT:?set NURALOUMI_SL101_CROSS_ROOT to a dedicated CI path}
IMAGE=${NURALOUMI_CI_ALPINE_IMAGE:-alpine:3.22}

case "$CROSS_ROOT" in
    /*) ;;
    *) echo "SL101_CI_SYSROOT=FAIL reason=cross-root-must-be-absolute" >&2; exit 2 ;;
esac
case "$CROSS_ROOT" in
    "$ROOT/target/sl101-cross")
        echo "SL101_CI_SYSROOT=FAIL reason=refuse-overwrite-tablet-sysroot" >&2
        exit 2
        ;;
esac
if [ -e "$CROSS_ROOT/sysroot" ] || [ -L "$CROSS_ROOT/sysroot" ]; then
    echo "SL101_CI_SYSROOT=FAIL reason=existing-sysroot path=$CROSS_ROOT/sysroot" >&2
    exit 2
fi

mkdir -p "$CROSS_ROOT"
# The native arm/v7 container verifies APKINDEX and package signatures with
# Alpine's own trust keys. No --allow-untrusted, cross-arch index overrides,
# or third-party download feeds are used. Only development/runtime libraries
# are staged; ARM binaries are never executed on the build host.
docker run --rm --platform linux/arm/v7 \
    --volume "$CROSS_ROOT:/out" \
    "$IMAGE" sh -ec '
        test "$(uname -m)" = armv7l
        apk --root /out/sysroot --initdb --no-scripts \
            --keys-dir /etc/apk/keys \
            --repositories-file /etc/apk/repositories \
            add --no-cache \
            musl musl-dev gcc libgcc \
            cairo-dev wayland-dev libxkbcommon-dev \
            freetype-dev harfbuzz-dev fribidi-dev
    '

SYSROOT="$CROSS_ROOT/sysroot"
PCROOT="$CROSS_ROOT/pkgconfig"
mkdir -p "$PCROOT"
# The existing Tegra20 linker wrapper expects a flat pkg-config directory.
# Alpine dev packages provide their own metadata and dependency graph.
for source in "$SYSROOT"/usr/lib/pkgconfig/*.pc "$SYSROOT"/usr/share/pkgconfig/*.pc; do
    [ -f "$source" ] || continue
    cp "$source" "$PCROOT/"
done

for required in \
    "$SYSROOT/lib/ld-musl-armhf.so.1" \
    "$SYSROOT/usr/lib/Scrt1.o" \
    "$SYSROOT/usr/lib/crti.o" \
    "$SYSROOT/usr/lib/crtn.o" \
    "$SYSROOT/usr/lib/libgcc_s.so" \
    "$SYSROOT/usr/lib/libcairo.so" \
    "$SYSROOT/usr/lib/libxkbcommon.so" \
    "$SYSROOT/usr/lib/libfreetype.so" \
    "$SYSROOT/usr/lib/libharfbuzz.so" \
    "$SYSROOT/usr/lib/libfribidi.so" \
    "$PCROOT/cairo.pc" \
    "$PCROOT/freetype2.pc" \
    "$PCROOT/harfbuzz.pc" \
    "$PCROOT/fribidi.pc"; do
    if [ ! -e "$required" ]; then
        echo "SL101_CI_SYSROOT=FAIL reason=missing-required path=$required" >&2
        exit 1
    fi
done

gcc_count=0
for dir in "$SYSROOT"/usr/lib/gcc/armv7-alpine-linux-musleabihf/*; do
    [ -d "$dir" ] || continue
    for file in crtbeginS.o crtendS.o libgcc.a; do
        test -f "$dir/$file" || {
            echo "SL101_CI_SYSROOT=FAIL reason=missing-gcc-support path=$dir/$file" >&2
            exit 1
        }
    done
    gcc_count=$((gcc_count + 1))
done
test "$gcc_count" -eq 1 || {
    echo "SL101_CI_SYSROOT=FAIL reason=gcc-directory-count count=$gcc_count" >&2
    exit 1
}

# pkg-config must resolve target architecture metadata, not native host libs.
PKG_CONFIG_ALLOW_CROSS=1 \
PKG_CONFIG_SYSROOT_DIR="$SYSROOT" \
PKG_CONFIG_LIBDIR="$PCROOT" \
PKG_CONFIG_PATH="$PCROOT" \
pkg-config --exists cairo freetype2 harfbuzz fribidi wayland-client xkbcommon

printf 'source=official-Alpine-signed-apk\nimage=%s\narch=armv7\n' "$IMAGE" > "$CROSS_ROOT/source.txt"
echo "SL101_CI_SYSROOT=PASS source=$IMAGE target=armv7 root=$SYSROOT"
