#!/bin/sh
# Build ONE in-process native video library for nuraloumi-playerd.
# Does not create another player executable, install Mesa or touch devices.
set -eu
src=$(CDPATH= cd -- "$(dirname -- "$0")/src" && pwd -P)
out=${1:-${TMPDIR:-/tmp}/nuraloumi-video-build}
mkdir -p "$out"
out=$(CDPATH= cd -- "$out" && pwd -P)
protocols=$(pkg-config --variable=pkgdatadir wayland-protocols)
wayland-scanner client-header "$protocols/stable/linux-dmabuf/linux-dmabuf-v1.xml" "$out/linux-dmabuf.h"
wayland-scanner private-code "$protocols/stable/linux-dmabuf/linux-dmabuf-v1.xml" "$out/linux-dmabuf.c"
wayland-scanner client-header "$protocols/stable/xdg-shell/xdg-shell.xml" "$out/xdg-shell-client-protocol.h"
wayland-scanner private-code "$protocols/stable/xdg-shell/xdg-shell.xml" "$out/xdg-shell-protocol.c"
${CC:-cc} -std=gnu11 -O2 -fPIC -shared -pthread -Wall -Wextra -Werror \
  -I"$out" -I"$src" \
  $(pkg-config --cflags gstreamer-app-1.0 gstreamer-video-1.0 gstreamer-allocators-1.0 egl glesv2 gbm wayland-client) \
  "$src/vde-moving-probe.c" "$src/video-bridge.c" \
  "$src/wayland-present.c" "$src/wayland-stream.c" \
  "$out/linux-dmabuf.c" "$out/xdg-shell-protocol.c" \
  -Wl,-soname,libnuraloumi-video.so \
  $(pkg-config --libs gstreamer-app-1.0 gstreamer-video-1.0 gstreamer-allocators-1.0 egl glesv2 gbm wayland-client) \
  -o "$out/libnuraloumi-video.so"
file "$out/libnuraloumi-video.so"
sha256sum "$out/libnuraloumi-video.so"
