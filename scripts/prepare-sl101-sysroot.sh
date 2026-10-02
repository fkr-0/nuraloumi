#!/bin/sh
set -eu

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
CROSS_ROOT=${NURALOUMI_SL101_CROSS_ROOT:-"$ROOT/target/sl101-cross"}
SYSROOT=${NURALOUMI_SL101_SYSROOT:-"$CROSS_ROOT/sysroot"}
PKGCONFIG=${NURALOUMI_SL101_PKGCONFIG:-"$CROSS_ROOT/pkgconfig"}
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
        echo "SL101_SYSROOT=FAIL reason=unexpected-arch arch=$ARCH" >&2
        exit 1
        ;;
esac

REMOTE_CAIRO=$(ssh_run "readlink -f /usr/lib/libcairo.so.2")
REMOTE_WAYLAND=$(ssh_run "readlink -f /usr/lib/libwayland-client.so.0")
REMOTE_GCC=$(ssh_run "readlink -f /lib/libgcc_s.so.1")
REMOTE_FREETYPE=$(ssh_run "readlink -f /usr/lib/libfreetype.so.6")
REMOTE_HARFBUZZ=$(ssh_run "readlink -f /usr/lib/libharfbuzz.so.0")
REMOTE_FRIBIDI=$(ssh_run "readlink -f /usr/lib/libfribidi.so.0")
REMOTE_LOADER=/lib/ld-musl-armhf.so.1

for remote in \
    "$REMOTE_LOADER" \
    "$REMOTE_CAIRO" \
    "$REMOTE_WAYLAND" \
    "$REMOTE_GCC" \
    "$REMOTE_FREETYPE" \
    "$REMOTE_HARFBUZZ" \
    "$REMOTE_FRIBIDI"; do
    ssh_run "test -f '$remote'"
done

runtime_version() {
    package=$1
    remote=$2
    owner=$(ssh_run "apk info --who-owns '$remote'")
    version=$(printf '%s\n' "$owner" |
        sed -n "s/.* owned by ${package}-\(.*\)-r[0-9][0-9]*$/\1/p")
    if [ -z "$version" ]; then
        echo "SL101_SYSROOT=FAIL reason=runtime-version-unresolved package=$package path=$remote owner=$owner" >&2
        exit 1
    fi
    printf '%s\n' "$version"
}

FREETYPE_VERSION=$(runtime_version freetype "$REMOTE_FREETYPE")
HARFBUZZ_VERSION=$(runtime_version harfbuzz "$REMOTE_HARFBUZZ")
FRIBIDI_VERSION=$(runtime_version fribidi "$REMOTE_FRIBIDI")

STAGE="$SYSROOT.new.$$"
PKGSTAGE="$PKGCONFIG.new.$$"
REMOTE_STAGE=$(ssh_run "mktemp -d /tmp/nuraloumi-cross.XXXXXX")
cleanup() {
    rm -rf "$STAGE"
    rm -rf "$PKGSTAGE"
    ssh_run "rm -rf '$REMOTE_STAGE'" >/dev/null 2>&1 || true
}
trap cleanup EXIT HUP INT TERM

rm -rf "$STAGE"
rm -rf "$PKGSTAGE"
mkdir -p "$STAGE/lib" "$STAGE/usr/lib" "$PKGSTAGE"

# Fetch but do not install the tablet's own development/runtime support packages.
# These provide musl's dynamic PIE startup objects plus GCC's matching crtbeginS,
# crtendS and libgcc linker support. Mixing Rust's bundled musl CRT with the
# tablet loader corrupts DSO TLS on this SL101 and is therefore forbidden.
ssh_run "set -eu
    mkdir -p '$REMOTE_STAGE/apks' '$REMOTE_STAGE/root'
    cd '$REMOTE_STAGE/apks'
    apk fetch --no-progress --output . musl-dev gcc libgcc-static >/dev/null
    for pkg in *.apk; do
        tar -xzf \"\$pkg\" -C '$REMOTE_STAGE/root'
    done"

"$SSH" -o BatchMode=yes -o ConnectTimeout=8 "$SSH_TARGET"     "tar -C '$REMOTE_STAGE/root' -cf - usr/lib usr/include" |
    tar -xf - -C "$STAGE"

"$SCP" -q -o BatchMode=yes "$SSH_TARGET:$REMOTE_LOADER" "$STAGE/lib/ld-musl-armhf.so.1"
CAIRO_BASE=${REMOTE_CAIRO##*/}
WAYLAND_BASE=${REMOTE_WAYLAND##*/}
GCC_RUNTIME_BASE=${REMOTE_GCC##*/}
FREETYPE_BASE=${REMOTE_FREETYPE##*/}
HARFBUZZ_BASE=${REMOTE_HARFBUZZ##*/}
FRIBIDI_BASE=${REMOTE_FRIBIDI##*/}
"$SCP" -q -o BatchMode=yes "$SSH_TARGET:$REMOTE_CAIRO" "$STAGE/usr/lib/$CAIRO_BASE"
"$SCP" -q -o BatchMode=yes "$SSH_TARGET:$REMOTE_WAYLAND" "$STAGE/usr/lib/$WAYLAND_BASE"
"$SCP" -q -o BatchMode=yes "$SSH_TARGET:$REMOTE_GCC" "$STAGE/usr/lib/$GCC_RUNTIME_BASE"
"$SCP" -q -o BatchMode=yes "$SSH_TARGET:$REMOTE_FREETYPE" "$STAGE/usr/lib/$FREETYPE_BASE"
"$SCP" -q -o BatchMode=yes "$SSH_TARGET:$REMOTE_HARFBUZZ" "$STAGE/usr/lib/$HARFBUZZ_BASE"
"$SCP" -q -o BatchMode=yes "$SSH_TARGET:$REMOTE_FRIBIDI" "$STAGE/usr/lib/$FRIBIDI_BASE"

link_alias() {
    target=$1
    alias=$2
    if [ "$target" != "$alias" ] && [ ! -e "$STAGE/usr/lib/$alias" ] && [ ! -L "$STAGE/usr/lib/$alias" ]; then
        ln -s "$target" "$STAGE/usr/lib/$alias"
    fi
}

link_alias "$CAIRO_BASE" libcairo.so.2
link_alias libcairo.so.2 libcairo.so
link_alias "$WAYLAND_BASE" libwayland-client.so.0
link_alias libwayland-client.so.0 libwayland-client.so
link_alias "$GCC_RUNTIME_BASE" libgcc_s.so.1
link_alias "$FREETYPE_BASE" libfreetype.so.6
link_alias libfreetype.so.6 libfreetype.so
link_alias "$HARFBUZZ_BASE" libharfbuzz.so.0
link_alias libharfbuzz.so.0 libharfbuzz.so
link_alias "$FRIBIDI_BASE" libfribidi.so.0
link_alias libfribidi.so.0 libfribidi.so

GCC_SUPPORT_BASE="$STAGE/usr/lib/gcc/armv7-alpine-linux-musleabihf"
GCCDIR=
for candidate in "$GCC_SUPPORT_BASE"/*; do
    [ -d "$candidate" ] || continue
    if [ -n "$GCCDIR" ]; then
        echo "SL101_SYSROOT=FAIL reason=gcc-support-directory-not-unique" >&2
        exit 1
    fi
    GCCDIR=$candidate
done
if [ -z "$GCCDIR" ]; then
    echo "SL101_SYSROOT=FAIL reason=gcc-support-directory-missing" >&2
    exit 1
fi
GCC_REL=${GCCDIR#"$STAGE/"}

for required in \
    "$STAGE/usr/lib/Scrt1.o" \
    "$STAGE/usr/lib/crti.o" \
    "$STAGE/usr/lib/crtn.o" \
    "$STAGE/usr/lib/libc.so" \
    "$GCCDIR/crtbeginS.o" \
    "$GCCDIR/crtendS.o" \
    "$GCCDIR/libgcc.a" \
    "$STAGE/usr/lib/libgcc_s.so" \
    "$STAGE/usr/lib/libfreetype.so" \
    "$STAGE/usr/lib/libharfbuzz.so" \
    "$STAGE/usr/lib/libfribidi.so"; do
    if [ ! -e "$required" ]; then
        echo "SL101_SYSROOT=FAIL reason=missing-toolchain-file path=$required" >&2
        exit 1
    fi
done

cat >"$PKGSTAGE/cairo.pc" <<'EOF'
prefix=/usr
exec_prefix=${prefix}
libdir=${prefix}/lib
includedir=${prefix}/include
Name: cairo
Description: SL101 Cairo runtime ABI
Version: 1.18.4
Libs: -L${libdir} -lcairo
Cflags:
EOF

cat >"$PKGSTAGE/wayland-client.pc" <<'EOF'
prefix=/usr
exec_prefix=${prefix}
libdir=${prefix}/lib
includedir=${prefix}/include
Name: wayland-client
Description: SL101 Wayland client runtime ABI
Version: 1.26.0
Libs: -L${libdir} -lwayland-client
Cflags:
EOF

cat >"$PKGSTAGE/freetype2.pc" <<EOF
prefix=/usr
exec_prefix=\${prefix}
libdir=\${prefix}/lib
includedir=\${prefix}/include
Name: FreeType 2
Description: SL101 FreeType runtime ABI
Version: $FREETYPE_VERSION
Libs: -L\${libdir} -lfreetype
Cflags:
EOF

cat >"$PKGSTAGE/harfbuzz.pc" <<EOF
prefix=/usr
exec_prefix=\${prefix}
libdir=\${prefix}/lib
includedir=\${prefix}/include
Name: harfbuzz
Description: SL101 HarfBuzz runtime ABI
Version: $HARFBUZZ_VERSION
Libs: -L\${libdir} -lharfbuzz
Cflags:
EOF

cat >"$PKGSTAGE/fribidi.pc" <<EOF
prefix=/usr
exec_prefix=\${prefix}
libdir=\${prefix}/lib
includedir=\${prefix}/include
Name: fribidi
Description: SL101 FriBidi runtime ABI
Version: $FRIBIDI_VERSION
Libs: -L\${libdir} -lfribidi
Cflags:
EOF

rm -rf "$SYSROOT"
mv "$STAGE" "$SYSROOT"
rm -rf "$PKGCONFIG"
mv "$PKGSTAGE" "$PKGCONFIG"

TOOLCHAIN_APKS=$(ssh_run "cd '$REMOTE_STAGE/apks' && printf '%s ' *.apk")
{
    printf 'source=%s\n' "$SSH_TARGET"
    printf 'arch=%s\n' "$ARCH"
    printf 'toolchain_apks=%s\n' "$TOOLCHAIN_APKS"
    printf 'text_runtime_versions=freetype:%s harfbuzz:%s fribidi:%s\n' \
        "$FREETYPE_VERSION" "$HARFBUZZ_VERSION" "$FRIBIDI_VERSION"
    ssh_run "uname -a" | sed 's/^/uname=/'
} >"$CROSS_ROOT/source.txt"

(
    cd "$SYSROOT"
    sha256sum \
        lib/ld-musl-armhf.so.1 \
        usr/lib/Scrt1.o \
        usr/lib/crti.o \
        usr/lib/crtn.o \
        "$GCC_REL/crtbeginS.o" \
        "$GCC_REL/crtendS.o" \
        "$GCC_REL/libgcc.a" \
        "usr/lib/$CAIRO_BASE" \
        "usr/lib/$WAYLAND_BASE" \
        "usr/lib/$GCC_RUNTIME_BASE" \
        "usr/lib/$FREETYPE_BASE" \
        "usr/lib/$HARFBUZZ_BASE" \
        "usr/lib/$FRIBIDI_BASE"
) >"$CROSS_ROOT/sysroot.sha256"

ssh_run "rm -rf '$REMOTE_STAGE'" >/dev/null 2>&1 || true
trap - EXIT HUP INT TERM

echo "SL101_SYSROOT=PASS path=$SYSROOT source=$SSH_TARGET arch=$ARCH gccdir=$GCC_REL freetype=$FREETYPE_VERSION harfbuzz=$HARFBUZZ_VERSION fribidi=$FRIBIDI_VERSION"
echo "SL101_PKGCONFIG=PASS path=$PKGCONFIG"
