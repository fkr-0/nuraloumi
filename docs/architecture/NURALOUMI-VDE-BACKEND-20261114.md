# NuraLoumi single-player VDE/GPU integration — 2026-11-14

## Current device qualification

There is **one user-facing application** (nuraloumi-player) and
**one playback service** (nuraloumi-playerd). A single private
libnuraloumi-video.so is loaded into that service as a thread-owned
native library, not a separate player or IPC endpoint.

The private ARMv7/musl build was physically tested on SL101 .106:
the actual nuraloumi-player --open command reached 414/414 video
frames, 414/414 Wayland releases, zero PTS gaps and full AAC/Video
EOS at 23.591 fps. The normal playerd owner-only socket then passed
pause, stable position, seek-while-paused to 5000ms, volume/mute,
resume and Stop in 225ms; all 147 test buffers were released.

All on-device tests ran against privately staged exact-sha binaries;
**no installed NuraLoumi, Mesa, GStreamer, Sway or kernel change**.
The source renderer uses the qualified Grate 0088 four-internal-draw
I420 path and the source-patched GStreamer 1.28.7
capture-stride-align=64 property. Three-draw candidate 0089 is
not qualified or enabled.

The release packaging includes the internal video library only when
the exact target ARMv7/musl bundle is supplied explicitly via
NURALOUMI_VIDEO_BUNDLE_DIR. Without it, the standard software-only
CI package has no claim to GPU video. Debian/glibc needs separate
runtime qualification.

The commands and bounded device gates are recorded in
components/nura-media-engine/README.md and
docs/qualification/NURALOUMI-PLAYER-E2E.md.

---

## Archived context from the initial single-frame handoff

## Source ownership and current status

- `sl101_linux` owns Tegra VDE, GStreamer capture negotiation, the
  896/448 private stride adapter, Grate R8 sampler patch, GLES2
  multi-pass YUV→BT.709 shader and Wayland DMA-BUF client.
- `nuraloumi` owns the user-facing player, playlist, media service,
  process and surface lifetimes and the audio-only GStreamer playbin.
- This section predates the completed 414-frame integrated video test
  above and is retained as a historical architectural baseline.

## Contract now reflected in NuraLoumi

`player_engine::VdeGpuFrameContract` encodes the **experimental**
codec→renderer boundary for the real qualified 854×480 test:

    visible: 854×480
    coded capture: width 896; height 480
    DRM fourcc: YU12, LINEAR
    memory: 3 independent DMA-BUF FDs with GstVideoMeta
    Y stride: 896; U/V strides: 448
    output: XR24+LINEAR DMA-BUF, released only after wl_buffer.release

NuraLoumi's menu reads `Snapshot.gpu_status` from its player daemon,
which now accurately reports **single-frame PASS, streaming
UNSUPPORTED_GPU** instead of the outdated statement that no Grate
hardware pixel oracle exists. Audio-only playbin remains intact: the
video flag (bit 1) is not enabled and no unqualified GPU provider is
silently instantiated.

## Required implementation before enabling video controls

1. Make the private SL101 one-frame renderer a **persistent**
   decoder→GPU→Wayland service with a bounded in-flight buffer pool,
   per-frame timestamp/seek/stop lifecycle and release-driven reuse.
   Do not launch the one-frame diagnostic for each player frame.
2. Expose an owner-only IPC or stable native ABI for NuraLoumi
   playerd; implement Pause/Play/Seek/Stop/Volume and error/EOS mapping.
3. Keep Cairo/wl_shm menus operational if EGL or VDE is unavailable;
   audio-only mode must remain a real fallback.
4. Validate color and geometry across changing real frames, then run
   unconstrained GPU+Sway performance tests alongside audio A/V sync.
5. The matching private glibc Grate library now builds successfully
   as ELF ARMHF with the same R8 patch. **Actual Debian device pixel
   and Wayland qualification is still outstanding**. Only enable video
   after this and the exact signed/pinned backend artifact pass.

## Reference run data

The current no-clock VDE→appsink benchmark reaches roughly 303–309fps
on 854×480 H.264 Baseline, but the unpaced 6-pass GPU-only loop
measures ~14fps at 854×480. Neither figure is a continuous video FPS
measurement. A 1280×720 H.264 High Profile original fails hardware
decode after its first frame. See the dedicated SL101 benchmark report
and previous GPU pixel qualification in the sibling repository.
