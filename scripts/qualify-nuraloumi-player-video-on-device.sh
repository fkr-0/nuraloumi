#!/bin/sh
# Coordinator-only, root/flock-gated, no-install NuraLoumi player E2E.
# Args: PRIVATE_DIR expected_SD_CID expected_EMMC_CID expected_RUST_SHA
#       expected_NATIVE_SO_SHA MODE(eos|controls).
set -eu
blocked(){ echo "GATE_BLOCKED nuraloumi-video $*" >&2; exit 82; }
[ "$#" -eq 6 ] || blocked argv
case "$6" in eos|controls) : ;; *) blocked mode ;; esac
dir=$1
case "$dir" in /tmp/nuraloumi-video-qual.*) : ;; *) blocked private-dir ;; esac
[ "$(id -u)" -eq 0 ] && [ "${NURAL_VIDEO_LOCK_HELD:-}" = 1 ] || blocked root-flock
[ "$(hostname)" = sl101-nura ] &&
 [ "$(uname -r)" = 7.0.1-postmarketos-grate-dmavde1 ] || blocked target
[ "$(cat /sys/block/mmcblk1/device/cid)" = "$2" ] || blocked sd-cid
[ "$(cat /sys/block/mmcblk0/device/cid)" = "$3" ] || blocked emmc-cid
[ "$(cat /sys/class/power_supply/ac-adapter-detect/online)" = 1 ] || blocked ac
[ "$(cat /sys/class/power_supply/sbs-5-000b/capacity)" -ge 40 ] || blocked battery
sway=$(pgrep -xo sl101-sway) || blocked no-sway
[ "$(readlink "/proc/$sway/exe")" = /usr/local/bin/sl101-sway ] || blocked sway-bin
[ -z "$(pgrep -x nuraloumi-playerd || :)" ] || blocked active-playerd
[ -z "$(pgrep -x gst-launch-1.0 || :)" ] || blocked competing-decoder
[ -f /home/user/tds3e1-main.mp4 ] || blocked source-video
rust_sha=$(sha256sum "$dir/nuraloumi-playerd" | cut -d' ' -f1)
native_sha=$(sha256sum "$dir/lib/libnuraloumi-video.so" | cut -d' ' -f1)
[ "$rust_sha" = "$4" ] || blocked playerd-sha
[ "$native_sha" = "$5" ] || blocked native-sha
[ "$(sha256sum "$dir/lib/libgallium-25.0.7.so"|cut -d' ' -f1)" = 84af7822128f8a08a03c460164b00cd92b9058f22fdbe094f2811119d1d13cf8 ] || blocked mesa-sha
[ "$(sha256sum "$dir/gst/libgstv4l2codecs.so"|cut -d' ' -f1)" = 79ee247faf6397829d908136ad16e04dc936437a1c71b6d49120e470ec452608 ] || blocked gst-sha
[ -f /usr/local/lib/gr3d_serialize.so ] &&
 [ -d /usr/local/lib/sl101-libdrm-import-lists-5b06215a2876fec6 ] || blocked runtime-prerequisites
[ -S /run/sl101-desktop-user/wayland-1 ] &&
 [ -S /run/sl101-desktop-user/pulse/native ] || blocked desktop-sockets
mkdir -p "$dir/runtime" "$dir/state"
chown -R user:user "$dir"
chmod 700 "$dir" "$dir/runtime" "$dir/state"
printf 'GATE_PASS nuraloumi-one-playerd-VDE-3pass-internal\n'
sudo -n -u user env \
  QUAL_DIR="$dir" QUAL_MODE="$6" XDG_RUNTIME_DIR="$dir/runtime" XDG_STATE_HOME="$dir/state" \
  WAYLAND_DISPLAY=/run/sl101-desktop-user/wayland-1 \
  PULSE_SERVER=unix:/run/sl101-desktop-user/pulse/native \
  LD_LIBRARY_PATH="/usr/local/lib/sl101-libdrm-import-lists-5b06215a2876fec6:$dir/lib" \
  LD_PRELOAD=/usr/local/lib/gr3d_serialize.so \
  GST_PLUGIN_PATH_1_0="$dir/gst" SL101_PRIVATE_GST_DIR="$dir/gst" \
  GST_REGISTRY="$dir/registry.bin" \
  MESA_LOADER_DRIVER_OVERRIDE=grate LIBGL_DRIVERS_PATH="$dir/lib" \
  GRATE_I420_YU_PAIR=1 GRATE_I420_YU_FP_SCHEDULE=1 \
  GBM_BACKENDS_PATH="$dir/lib/gbm" \
  sh -eu -c '
    "$QUAL_DIR/nuraloumi-playerd" >"$QUAL_DIR/playerd.log" 2>&1 &
    pid=$!
    trap "kill $pid 2>/dev/null || :; wait $pid 2>/dev/null || :" EXIT INT TERM
    sleep 1
    "$QUAL_DIR/nuraloumi-player" --open /home/user/tds3e1-main.mp4 --status >"$QUAL_DIR/initial.json"
    if [ "$QUAL_MODE" = controls ]; then
      /usr/sbin/python3 "$QUAL_DIR/qualify-nuraloumi-player-controls-on-device.py"
      grep "WAYLAND_STREAM_RESULT" "$QUAL_DIR/playerd.log" | tail -1 || :
      echo "PLAYERD_INTEGRATED_CONTROL_PASS one_playerd=1"
      exit 0
    fi
    result=1
    for n in $(seq 1 40); do
      sleep 1
      if ! "$QUAL_DIR/nuraloumi-player" --status >"$QUAL_DIR/current.json"; then
        echo "PLAYER_CONTROL_FAIL poll=$n"; break
      fi
      if grep -q "\"status\": \"Error\"" "$QUAL_DIR/current.json"; then
        echo "PLAYER_ERROR poll=$n"; break
      fi
      if grep -q "\"status\": \"Stopped\"" "$QUAL_DIR/current.json"; then
        result=0;echo "PLAYER_EOS at_poll=$n"; break
      fi
    done
    grep "STREAM_RESULT" "$QUAL_DIR/playerd.log" | tail -1 || :
    grep "WAYLAND_STREAM_RESULT" "$QUAL_DIR/playerd.log" | tail -1 || :
    grep "STREAM_GST_ERROR" "$QUAL_DIR/playerd.log" | tail -3 || :
    if [ "$result" -ne 0 ]; then
      cat "$QUAL_DIR/current.json" 2>/dev/null || :
      tail -25 "$QUAL_DIR/playerd.log"
      exit 7
    fi
    grep -q "STREAM_PIPELINE_EOS video_and_audio_drained=1" "$QUAL_DIR/playerd.log"
    grep -q "WAYLAND_STREAM_RESULT submitted=414 callbacks=414 releases=414 outstanding=0 pass=1" "$QUAL_DIR/playerd.log"
    echo "PLAYERD_INTEGRATED_E2E_PASS video_frames=414 ui_ipc=1 audio_eos=1 drm_buffers_released=414"
  '
