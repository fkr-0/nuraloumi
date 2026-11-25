#!/bin/sh
# Host-only regression: discover MP4 without/with AAC, and fail closed for a
# nonexistent file. No decoder, DRM, Wayland or live device session.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
scratch=$(mktemp -d)
trap 'rm -r "$scratch"' EXIT INT TERM
# Opt-in ASan/UBSan on an ordinary host. The ARMv7 distribution smoke job
# uses the normal C ABI, so it needs no sanitizer runtime on the tablet.
sanitizer_flags=
if [ "${NURA_MEDIA_SANITIZERS:-0}" = 1 ]; then
  sanitizer_flags='-fsanitize=address,undefined -fno-omit-frame-pointer'
fi
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
ffmpeg -hide_banner -nostdin -loglevel error -y \
  -f lavfi -i sine=frequency=440:sample_rate=44100 \
  -t 1 -c:a aac "$scratch/audio-only.m4a"
cc -std=gnu11 -Wall -Wextra -Werror $sanitizer_flags \
  $(pkg-config --cflags gstreamer-pbutils-1.0) \
  "$root/src/media-audio.c" "$root/tests/test-media-audio.c" \
  $(pkg-config --libs gstreamer-pbutils-1.0) \
  -o "$scratch/test-media-audio"
"$scratch/test-media-audio" "$scratch/noaudio.mp4" "$scratch/aac.mp4" \
  "$scratch/other-size.mp4" "$scratch/audio-only.m4a"
cc -std=gnu11 -Wall -Wextra -Werror $sanitizer_flags \
  $(pkg-config --cflags gstreamer-1.0) \
  "$root/src/media-bus.c" "$root/tests/test-media-bus.c" \
  $(pkg-config --libs gstreamer-1.0) \
  -o "$scratch/test-media-bus"
"$scratch/test-media-bus"
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
# Playerd may discard stdout/stderr, so setup failures must survive through
# nura_video_status().error rather than only printf diagnostics.
for stage in \
  'GStreamer pipeline parse failed' \
  'missing GStreamer video appsink' \
  'cannot open /dev/dri/renderD128' \
  'GBM device creation failed' \
  'EGL/GL DMA-BUF extension entry points missing' \
  'EGL GBM display initialization failed' \
  'Grate GLES renderer unavailable' \
  'Wayland video stream surface unavailable'
do
  if ! grep -Fq "$stage" "$root/src/vde-moving-probe.c"; then
    echo "FAIL: missing native video setup diagnostic: $stage" >&2
    exit 1
  fi
done
grep -Fq 'if(!vendor||strcmp((const char *)vendor,"Grate"))' \
  "$root/src/vde-moving-probe.c"
printf 'MEDIA_AUDIO_REGRESSION_PASS noaudio=0 aac=1 geometry=distinct decoder_name=stable video_setup_diagnostics=present\n'
