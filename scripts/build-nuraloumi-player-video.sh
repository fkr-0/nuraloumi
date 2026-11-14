#!/bin/sh
# Private reproducible ARMv7/musl native backend for ONE NuraLoumi playerd.
# Cross-builds the in-process .so and stages pinned dependencies; no device
# access or system installation. This is NOT a production deployment gate.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
sl101="$root/../sl101_linux"
src="$root/components/nura-media-engine/src"
out="$root/target/nuraloumi-video-armv7"
cc="$sl101/work/sl101-gstgl-gles-20261113/gst-build/cc"
mesa="$sl101/work/sl101-vde-dmabuf-20261114/mesa-i420-r8/stage/opt/grate-mesa25-musl-docker-20261113/lib"
gst="$sl101/work/sl101-vde-dmabuf-20261114/capture-clean/libgstv4l2codecs.so"
mesa_sha=3745960c85ab21ecc33c40496995a01f806d27293c59cce711b3e38c6eef5977
gst_sha=79ee247faf6397829d908136ad16e04dc936437a1c71b6d49120e470ec452608
[ -x "$cc" ] || { echo 'missing pinned ARMv7 native GStreamer cross compiler' >&2; exit 82; }
[ "$(sha256sum "$mesa/libgallium-25.0.7.so" | cut -d' ' -f1)" = "$mesa_sha" ] || exit 82
[ "$(sha256sum "$gst" | cut -d' ' -f1)" = "$gst_sha" ] || exit 82
mkdir -p "$out/lib" "$out/gst"
docker run --rm --platform linux/amd64 --network none \
  --mount "type=bind,src=$src,dst=/source,readonly" \
  --mount "type=bind,src=$cc,dst=/cc,readonly" \
  --mount "type=bind,src=$out,dst=/out" \
  sl101-gstgl-gles-cross:20261113 sh -eu -c '
    sys=/opt/gstsys
    export PKG_CONFIG_LIBDIR="$sys/opt/grate/lib/pkgconfig:$sys/usr/lib/pkgconfig:$sys/usr/share/pkgconfig"
    export PKG_CONFIG_SYSROOT_DIR="$sys"
    wayland-scanner client-header "$sys/usr/share/wayland-protocols/stable/linux-dmabuf/linux-dmabuf-v1.xml" /out/linux-dmabuf.h
    wayland-scanner private-code "$sys/usr/share/wayland-protocols/stable/linux-dmabuf/linux-dmabuf-v1.xml" /out/linux-dmabuf.c
    wayland-scanner client-header "$sys/usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml" /out/xdg-shell-client-protocol.h
    wayland-scanner private-code "$sys/usr/share/wayland-protocols/stable/xdg-shell/xdg-shell.xml" /out/xdg-shell-protocol.c
    /cc -std=gnu11 -O2 -fPIC -shared -pthread -Wall -Wextra -Werror \
      -I/out -I/source \
      $(pkg-config --cflags gstreamer-app-1.0 gstreamer-video-1.0 gstreamer-allocators-1.0 egl glesv2 gbm wayland-client) \
      /source/vde-moving-probe.c /source/video-bridge.c \
      /source/wayland-present.c /source/wayland-stream.c \
      /out/linux-dmabuf.c /out/xdg-shell-protocol.c \
      -L"$sys/opt/grate/lib" -Wl,-rpath-link,"$sys/opt/grate/lib" \
      -Wl,-soname,libnuraloumi-video.so \
      $(pkg-config --libs gstreamer-app-1.0 gstreamer-video-1.0 gstreamer-allocators-1.0 egl glesv2 gbm wayland-client) \
      -o /out/lib/libnuraloumi-video.so
  '
cp -a "$mesa"/. "$out/lib/"
cp "$gst" "$out/gst/libgstv4l2codecs.so"
readelf -h "$out/lib/libnuraloumi-video.so" | grep -E 'Machine:|Class:'
readelf -d "$out/lib/libnuraloumi-video.so" | grep NEEDED
sha256sum "$out/lib/libnuraloumi-video.so" "$out/lib/libgallium-25.0.7.so" "$out/gst/libgstv4l2codecs.so"
echo "NURAL_VIDEO_CROSS_BUILD=PASS target=armv7-musl output=$out"
