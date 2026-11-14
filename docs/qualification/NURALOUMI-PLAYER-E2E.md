# NuraLoumi native media player: qualification (2026-11-12 through 2026-11-14)

## 2026-11-14 — qualified 0.0.7-dev.1 three-pass integration

The existing NuraLoumi player and playerd use the private SHA-pinned
Grate 0092 Y+U/V/RGB three-internal-draw variant. The two opt-ins
GRATE_I420_YU_PAIR and GRATE_I420_YU_FP_SCHEDULE are passed only to the
playerd child process; there is no second player or system Mesa upgrade.

Physical ARMv7 private-stage E2E from the actual NuraLoumi player:
414 frames, 414 compositor callbacks, 414 releases, 23.590 presented
fps, video/audio/pipeline EOS, zero PTS gaps and no error. Integrated
controls PASS: paused position delta 0 ms, paused seek 5000 ms,
resume at 5166 ms, volume/mute and Stop 245.6 ms; all 147 buffer
leases returned. The full suite cross build has 27/27 ELF audit PASS
(VFPv3-D16, no NEON or ARM hardware division).

A separate Grate 0093 trace probe verifies three real draws/frame,
and qualifies the PCM observation sink pause fix. That observer fix
does not modify the shipping NuraLoumi playerd. Browser isolation and
a separate WPE renderer boundary are documented in
NURALOUMI-VIDEO-BROWSER-ISOLATION-20261114.md.

These are private, no-install tests. Production deployment, full
nonuniform RGB oracles, four-buffer pressure, live WPE page rendering
and physical acoustic/optical sync are not yet qualified.

## 2026-11-14 — single-player VDE/Grate four-pass integration

The original source-only renderer and its 414-frame hardware
qualification are now integrated **inside the existing
nuraloumi-playerd** as one dynamically loaded native shared library.
The single nuraloumi-player Cairo menu and existing JSON Unix socket
remain the only UI and control API. There are no additional media
player executables, fallback selectors or new sockets.

The physically executed no-install Alpine ARMv7 integration test
used the actual nuraloumi-player --open CLI to play the known
854×480 H.264/BT.709 video with AAC audio. All 414 frames were
GPU-rendered, committed to Sway, received compositor frame callbacks
and released their GBM BOs; both video and shared-clock audio pipeline
reached EOS. Observed rate 23.591 fps, PTS gaps/errors zero, no
CPU YUV color conversion. The audio branch was drained on EOS;
physical acoustic/PCM lip sync was not measured.

The separate owner-socket control test on the real SL101 measured
pause stability 0ms, paused seek to 5000ms, volume set to 0.35,
mute/unmute, resume at 5208ms, and Stop reply at 225ms. All 147
Wayland buffers involved in that run were released.

Reproducer (staging only, never installs components or replaces Sway):

    sh scripts/build-nuraloumi-player-video.sh
    sh scripts/qualify-nuraloumi-player-video.sh eos
    sh scripts/qualify-nuraloumi-player-video.sh controls

The above scripts require the sibling sl101_linux private Grate 0088
and source-patched GStreamer SHA-pinned build artifacts, as well as
the exact-device root-held flock, card identity, AC and battery gates.
Only the original media player daemon starts and stops; the installed
desktop/player remain untouched.

Package staging supports an **explicit** native ARM/musl bundle
parameter NURALOUMI_VIDEO_BUNDLE_DIR. The build-only CI path without
that environment variable continues to produce a software-only
package with no GPU-video runtime promise. Runtime video remains
strictly constrained to the tested 854×480 24fps I420 shape;
different codecs/profiles/resolutions fail closed, not via CPU video.

The following historical prerelease section reflects earlier
audio-only status and is retained for release provenance.

---

## Provenance and scope

Development prerelease `0.0.4-dev.3`. The new `nuraloumi-player`
Cairo/wl_shm client and `nuraloumi-playerd` native GStreamer service are
separate from the pre-existing `nuraloumi-music` MPRIS *remote* controller.

The player service is implemented in
`crates/nuraloumi-shell/src/player_engine.rs`; UI and file browser in
`crates/nuraloumi-shell/src/bin/player.rs`; socket service in `playerd.rs`.
They are packaged with the XDG MIME handler
`packaging/applications/nuraloumi-player.desktop`. The file manager and
XDG Open With chooser can discover this handler once the CI package has been
installed. Existing panel integration and an external MPRIS *export* service
are **not yet implemented** and should not be claimed.

The engine dynamically loads GStreamer 1.x and invokes the playbin C API
directly. This is a stateful native playback implementation, **not**
`gst-launch` or another player's MPRIS interface. Normal playbin audio
output uses its negotiated audio sink; the actual sink/device must be
observed on the physical target before claiming audio E2E. There is no
explicit sink-selection UI or physical audio-output proof yet.

The explicit playbin flag is AUDIO=2 (video=1 is never enabled).
`gpu_status` reads **UNSUPPORTED_GPU**. It cannot paint video frames,
claim GPU play, or fall back to software video. A supported video file may
have its audio stream played, but there is **no qualified video surface**.
The shell's controls remain drawable without a valid EGL display.

## Bounded control plane

- Owner-only Unix domain socket at `$XDG_RUNTIME_DIR/nuraloumi-player.sock`;
  mode 0600, single newline-delimited JSON command per connection,
  at most 4096 request bytes, 300 ms I/O deadlines, non-blocking accept
  and bounded GStreamer event bus draining.
- Commands: `status`, `add`, `remove`, `move`, `select`,
  `play`, `pause`, `toggle`, `stop`, `next`, `previous`,
  `seek`, `volume`, `mute` with typed parameters.
- Playlist: at most 256 tracks, absolute canonical existing UTF-8 files
  when adding, no shell interpolation, file URI percent encoding, atomic
  per-user JSON state under XDG state home (0600).
- UI keyboard: arrows/Enter, Space play/pause, O open, +/- volume,
  Q/Escape close or return. Pointer/touch and playlist paging supported.
- No process, DRM ownership, KMS session, kernel module, Mesa or compositor
  modification is performed by the player.

### Host-only reproducible checks

```sh
cargo fmt --all -- --check
cargo test --workspace --offline
cargo clippy --workspace --all-targets --offline -- -D warnings
python3 -m unittest scripts/test_package_integrity.py scripts/test_native_packages_ci.py scripts/test_musl_ci.py scripts/test_debian_armhf_ci.py -v

# Debug-only synthetic GStreamer EOS test: NOT sound-output evidence.
mkdir -p target/player-smoke-runtime target/player-smoke-state
ffmpeg -hide_banner -loglevel error -f lavfi -i sine=frequency=440:duration=2 -c:a pcm_s16le -y target/player-smoke.wav
NURALOUMI_PLAYER_TEST_FAKESINK=1 XDG_RUNTIME_DIR="$PWD/target/player-smoke-runtime" XDG_STATE_HOME="$PWD/target/player-smoke-state" target/debug/nuraloumi-player --open "$PWD/target/player-smoke.wav" --status
# Status should transition Playing -> Stopped after EOS (with no audio output).
```

**Host observation:** the generated 2-second WAV was accepted by real
GStreamer playbin with debug fakesink, initially `Playing`, and after 3 s
`Stopped`, with no bus error. This proves no physical audio, hardware GPU
frames, ARMv7 compatibility, or Sway touch success.

### GPU fault and acceptance blocker

Upstream `sl101_linux/docs/sl101-gpu-video-hardware-20261112.md`
reports from physical Alpine .106 (kernel
`7.0.1-postmarketos-grate-dmavde1`): GStreamer VDE H.264 -> DMA-BUF
-> GLMemory -> glimagesink **SIGSEGV 0x4000** at
`dri2_setup_screen`; 64x64 synthetic GL fixture reproduced failure even
without VDE. `eglinfo` and mpv GLES2 contexts did work, but mpv render
failed at `gl_FragCoord` and `TEX TEMP[2]` shader limitations; VLC
could not create a Wayland EGL vout parent. None produced accepted video pixels.
This code does not repair the EGL/VDE path, so GPU video remains blocked.

Only a separate verified VDE -> GLES2 renderer with DMA-BUF import or
explicitly labeled CPU-upload/GPU-colorspace-conversion, supported Grate
shader compilation, hardware maps, pixel oracle and WayVNC capture,
measured FPS and A/V sync can clear `UNSUPPORTED_GPU`.
CPU colorspace conversion or wl_shm video is not an acceptable substitute.

## CI / deployment rules

Both `.github/workflows/sl101-musl-armv7.yml` and
`.github/workflows/sl101-debian-armhf.yml` discover **all** Cargo binary
targets; the new player binaries and desktop file must be present in
the exact SHA-pinned CI staging manifest, native packages and ELF audit.
The Alpine ARMv7 musl dynamic GStreamer libraries/plugins are runtime
dependencies **not** automatically included by static Cargo ELF link audit.
Validate the real runtime against the device's exact musl GStreamer plugins.
An ephemeral-CI APK signing key is not device trust enrollment.

No deployment is authorized from host-built binaries. Before staging an
exact-commit CI package on .106: check GitHub Actions, provenance hashes,
artifact ABI, hostname, kernel/DTB, SD/eMMC CID, AC/battery, active Sway and
rollback backup; do not alter boot, kernel, Mesa, browser or desktop.
Keep .100 untouched while power and GPU prerequisites remain unmet.
Real audio acceptance requires MP3/AAC/OGG/Opus where supported, sink name
and device inspection, pause/seek/volume/EOS/long-liveness plus human ear
confirmation if needed. Real GPU video acceptance is independent.
No production activation on any failed gate.

## Rollback

Do not replace the installed shell. For a staged inactive package, remove
only the exact staging directory after recording the checksums; leave the
current NuraLoumi binaries and Sway intact. For running development
services, close the menu and stop only the user-owned
`nuraloumi-playerd` after saving its playlist; never kill the compositor.
