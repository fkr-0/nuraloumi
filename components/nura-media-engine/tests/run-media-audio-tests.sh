#!/bin/sh
# Host-only regression: discover MP4 without/with AAC, and fail closed for a
# nonexistent file. No decoder, DRM, Wayland or live device session.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
scratch=$(mktemp -d)
trap 'rm -r "$scratch"' EXIT INT TERM
ffmpeg -hide_banner -nostdin -loglevel error -y \
  -f lavfi -i color=c=black:s=64x64:r=24 -t 1 \
  -c:v libx264 -pix_fmt yuv420p -an "$scratch/noaudio.mp4"
ffmpeg -hide_banner -nostdin -loglevel error -y \
  -f lavfi -i color=c=black:s=64x64:r=24 \
  -f lavfi -i sine=frequency=440:sample_rate=44100 \
  -t 1 -c:v libx264 -pix_fmt yuv420p -c:a aac \
  "$scratch/aac.mp4"
ffmpeg -hide_banner -nostdin -loglevel error -y \
  -f lavfi -i color=c=black:s=80x48:r=24 \
  -f lavfi -i sine=frequency=440:sample_rate=44100 \
  -t 1 -c:v libx264 -pix_fmt yuv420p -c:a aac \
  "$scratch/other-size.mp4"
cc -std=gnu11 -Wall -Wextra -Werror \
  $(pkg-config --cflags gstreamer-pbutils-1.0) \
  "$root/src/media-audio.c" "$root/tests/test-media-audio.c" \
  $(pkg-config --libs gstreamer-pbutils-1.0) \
  -o "$scratch/test-media-audio"
"$scratch/test-media-audio" "$scratch/noaudio.mp4" "$scratch/aac.mp4" \
  "$scratch/other-size.mp4"
# Both are in the renderer source, not in the test harness: the exact
# decoder name must remain stable across tracks in one long-lived playerd.
grep -Fq 'v4l2slh264dec name=video_decoder capture-stride-align=64' \
  "$root/src/vde-moving-probe.c"
grep -Fq 'gst_bin_get_by_name(GST_BIN(pipeline),"video_decoder")' \
  "$root/src/vde-moving-probe.c"
if grep -Fq '"v4l2slh264dec0"' "$root/src/vde-moving-probe.c"; then
  echo 'FAIL: auto-generated decoder name restored' >&2
  exit 1
fi
printf 'MEDIA_AUDIO_REGRESSION_PASS noaudio=0 aac=1 geometry=distinct decoder_name=stable\n'
