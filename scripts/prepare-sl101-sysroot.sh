#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
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
REMOTE_LOADER=/lib/ld-musl-armhf.so.1

for remote in "$REMOTE_LOADER" "$REMOTE_CAIRO" "$REMOTE_WAYLAND" "$REMOTE_GCC"; do
    ssh_run "test -f '$remote'"
done

STAGE="$SYSROOT.new.$$"
trap 'rm -rf "$STAGE"' EXIT HUP INT TERM
rm -rf "$STAGE"
mkdir -p "$STAGE/lib" "$STAGE/usr/lib" "$PKGCONFIG"

"$SCP" -q -o BatchMode=yes "$SSH_TARGET:$REMOTE_LOADER" "$STAGE/lib/ld-musl-armhf.so.1"
CAIRO_BASE=${REMOTE_CAIRO##*/}
WAYLAND_BASE=${REMOTE_WAYLAND##*/}
GCC_BASE=${REMOTE_GCC##*/}
"$SCP" -q -o BatchMode=yes "$SSH_TARGET:$REMOTE_CAIRO" "$STAGE/usr/lib/$CAIRO_BASE"
"$SCP" -q -o BatchMode=yes "$SSH_TARGET:$REMOTE_WAYLAND" "$STAGE/usr/lib/$WAYLAND_BASE"
"$SCP" -q -o BatchMode=yes "$SSH_TARGET:$REMOTE_GCC" "$STAGE/usr/lib/$GCC_BASE"

ln -s ld-musl-armhf.so.1 "$STAGE/lib/libc.so"
ln -s ld-musl-armhf.so.1 "$STAGE/lib/libc.musl-armv7.so.1"

link_alias() {
    target=$1
    alias=$2
    if [ "$target" != "$alias" ]; then
        ln -s "$target" "$STAGE/usr/lib/$alias"
    fi
}

link_alias "$CAIRO_BASE" libcairo.so.2
link_alias libcairo.so.2 libcairo.so
link_alias "$WAYLAND_BASE" libwayland-client.so.0
link_alias libwayland-client.so.0 libwayland-client.so
link_alias "$GCC_BASE" libgcc_s.so.1
link_alias libgcc_s.so.1 libgcc_s.so

cat >"$PKGCONFIG/cairo.pc" <<'EOF'
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

cat >"$PKGCONFIG/wayland-client.pc" <<'EOF'
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

rm -rf "$SYSROOT"
mv "$STAGE" "$SYSROOT"
trap - EXIT HUP INT TERM

{
    printf 'source=%s\n' "$SSH_TARGET"
    printf 'arch=%s\n' "$ARCH"
    ssh_run "uname -a" | sed 's/^/uname=/'
} >"$CROSS_ROOT/source.txt"

(
    cd "$SYSROOT"
    sha256sum \
        lib/ld-musl-armhf.so.1 \
        "usr/lib/$CAIRO_BASE" \
        "usr/lib/$WAYLAND_BASE" \
        "usr/lib/$GCC_BASE"
) >"$CROSS_ROOT/sysroot.sha256"

echo "SL101_SYSROOT=PASS path=$SYSROOT source=$SSH_TARGET arch=$ARCH"
echo "SL101_PKGCONFIG=PASS path=$PKGCONFIG"
