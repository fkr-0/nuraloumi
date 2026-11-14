#!/bin/sh
# No-install SL101 hardware qualification for the ONE integrated NuraLoumi
# playerd. Uses unique /tmp, private registry/state and unchanged compositor.
set -eu
mode=${1:-eos}
case "$mode" in eos|controls) : ;; *) echo 'usage: qualify-nuraloumi-player-video.sh [eos|controls]' >&2; exit 2 ;; esac
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
bin="$root/target/armv7-unknown-linux-musleabihf/release"
bundle="$root/target/nuraloumi-video-armv7"
identity="$root/../sl101_linux/work/sl101-gstgl-gles-20261113/device-identity.env"
[ -r "$identity" ] || exit 82
# shellcheck disable=SC1090
. "$identity"
[ -n "${SL101_EXPECT_SD_CID:-}" ] &&
 [ -n "${SL101_EXPECT_EMMC_CID:-}" ] || exit 82
for file in "$bin/nuraloumi-player" "$bin/nuraloumi-playerd" \
            "$bundle/lib/libnuraloumi-video.so" \
            "$bundle/lib/libgallium-25.0.7.so" \
            "$bundle/gst/libgstv4l2codecs.so"; do
  [ -r "$file" ] || exit 82
done
target=sl101-relay
dir=$(ssh -o BatchMode=yes "$target" 'mktemp -d /tmp/nuraloumi-video-qual.XXXXXX')
case "$dir" in /tmp/nuraloumi-video-qual.*) : ;; *) exit 82 ;; esac
cleanup(){
  ssh -o BatchMode=yes "$target" "cat '$dir/playerd.log' 2>/dev/null || :" \
    >"$root/target/nuraloumi-player-e2e-device.log" 2>/dev/null || :
  ssh -o BatchMode=yes "$target" "cat '$dir/current.json' 2>/dev/null || :" \
    >"$root/target/nuraloumi-player-e2e-status.json" 2>/dev/null || :
  ssh -o BatchMode=yes "$target" "rm -rf '$dir'" || :
}
trap cleanup EXIT HUP INT TERM
ssh -o BatchMode=yes "$target" "mkdir -p '$dir/lib' '$dir/gst'"
scp -q "$bin/nuraloumi-player" "$bin/nuraloumi-playerd" \
    "$root/scripts/qualify-nuraloumi-player-video-on-device.sh" \
    "$root/scripts/qualify-nuraloumi-player-controls-on-device.py" "$target:$dir/"
tar -C "$bundle" -cf - lib gst |
  ssh -o BatchMode=yes "$target" "tar -C '$dir' -xf -"
rust_sha=$(sha256sum "$bin/nuraloumi-playerd" | cut -d' ' -f1)
native_sha=$(sha256sum "$bundle/lib/libnuraloumi-video.so" | cut -d' ' -f1)
ssh -o BatchMode=yes "$target" \
  "sudo -n flock -n /run/lock/sl101-nura-qualification.lock \
  env NURAL_VIDEO_LOCK_HELD=1 \
  sh '$dir/qualify-nuraloumi-player-video-on-device.sh' \
  '$dir' '$SL101_EXPECT_SD_CID' '$SL101_EXPECT_EMMC_CID' \
  '$rust_sha' '$native_sha' '$mode'"
