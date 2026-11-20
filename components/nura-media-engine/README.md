# NuraLoumi internal video renderer

The public application remains **exactly one player**: `nuraloumi-player`.
Its existing `nuraloumi-playerd` daemon owns all playback, playlist,
controls, audio, GPU resources, buffer lifetimes, and window destruction.
The private `libnuraloumi-video.so` is an *in-process shared library*
loaded by the same daemon, **not another player process or service**.

## Source and qualification

The native C four-pass renderer originates from the independently tested
SL101 VDE → DMA-BUF → Grate 0088 → Sway source in
`sl101_linux/tools/sl101-grate-rgba-dmabuf-20261113/`.
Its source-only prototype decoded all 414 video frames, presented all
414 Sway buffers, released all 414, reached the shared-clock AAC EOS,
and measured 23.58 fps.

The vendored native library adds an opaque thread-safe C ABI:
`nura_video_open`, `nura_video_command`, `nura_video_status`,
`nura_video_close`. All commands continue to arrive at the existing
owner-only `nuraloumi-playerd` Unix socket. No additional socket,
window controller, CLI, or shell command is created.

When a video track is selected, the single daemon stops its audio-only
playbin, opens the same-file H.264 VDE pipeline, and reuses
its existing Play/Pause/Seek/Volume/Mute/Stop/Next/Previous/EOS model.
It discovers whether the MP4 contains an audio stream: AAC tracks use
the same-pipeline PulseAudio branch, while silent MP4 files do not
create a nonexistent AAC branch. The V4L2 decoder has an explicit
name, because GStreamer increments automatically generated names
between clips in a long-lived daemon.
An unsupported resolution or frame rate is rejected immediately with
the actual detected geometry rather than a generic GPU error.
The bus monitor latches EOS and GStreamer errors across intermediate
polls, so an early-consumed EOS cannot become a false timeout and an
early-consumed error cannot disappear from the final qualification result.
Audio-only tracks remain serviced by the same daemon's GStreamer
audio playbin. There is **no video backend selector**, no CPU YUV
conversion, no `glimagesink`, no generic software-video fallback.

## Device proof

The integrated, physical SL101 test invoked the **normal UI command**:
`nuraloumi-player --open /home/user/tds3e1-main.mp4 --status`
against one privately staged `nuraloumi-playerd`.

- 414/414 presented VDE hardware frames, 414 Sway frame callbacks,
  414 `wl_buffer.release`, zero outstanding GBM BOs.
- AAC/Video pipeline EOS; zero PTS gaps/errors, no mapped CPU YUV
  conversion; observed 23.591 fps and ~28.0ms mean clock lag.
- A separate real-device owner-socket test exercised paused position
  stability (0ms delta), seek while paused to 5000ms, volume 35%,
  mute/unmute, resume at 5208ms and stop under 226ms. All 147 submitted
  Sway buffers were released cleanly.

Both tests were performed with separately copied ARMv7/musl binaries and
private libraries in a unique /tmp directory. **No installed NuraLoumi,
Mesa, kernel, compositor or system libraries were changed.**

## Build and package

Host ABI smoke:

    sh components/nura-media-engine/build.sh target/nuraloumi-video-host

Pinned ARMv7/musl private bundle (requires the sibling `sl101_linux`
qualification artifacts and its offline cross compiler):

    sh scripts/build-nuraloumi-player-video.sh

An explicitly opted-in `cargo xtask package --target
armv7-unknown-linux-musleabihf` can include that bundle by setting
`NURALOUMI_VIDEO_BUNDLE_DIR` to the absolute
`target/nuraloumi-video-armv7` path. The manifest SHA256-covers
the native library and every private runtime dependency, and the Alpine
packager installs it under `/usr/lib/nuraloumi/video`.
Without that explicit bundle, the normal CI artifact remains
software-only; it does **not** claim device video availability.
The Debian/glibc version requires separate native qualification.
The independent `.github/workflows/nura-media-video-armv7.yml`
tests the audio probe and compiles the native library against Alpine
ARMv7/musl and Debian ARMHF/glibc distribution headers. Those
CI-produced libraries are ABI smoke artifacts only, carry
`device_deployment=forbidden`, and lack the pinned hardware producer
stack. They must **not** be installed on the tablet. Only the pinned
producer build plus an on-device end-to-end qualification can promote
a renderer to a device release.

## Remaining limits

Only the exact tested 854×480/24fps BT.709 linear YU12 hardware
capture (coded width 896, pitches 896/448/448) is presently supported.
The existing 640×360 and 720×404 MP4 clips on the tablet cannot
be shown by this path yet; audio presence alone is not sufficient.
The audio output phase is tied to the GStreamer pipeline clock, but
has not had an acoustic/PCM loopback lip-sync measurement.
No system-level player UI, seek-storm, fault-injection or arbitrary
file/codec qualification is claimed. Hardware runtime unavailable
must produce an explicit error, not a second player or CPU fallback.
