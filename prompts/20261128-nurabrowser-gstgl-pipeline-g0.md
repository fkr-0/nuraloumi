# Repair the WPE GStreamer-GL HLS video presentation path on SL101

project: nuraloumi
lane: GST-PIPELINE
parallel: 1
relength: 0
generation: G0
priority: 99

## Goal
Implement and verify GPU-backed GStreamer-GL video frame presentation for a real Tagesschau HLS stream in the native ARMv7-musl NuraBrowser, preserving EGL/GL and the Grate/VDE hardware acceleration path. A muted autoplay page that never decodes frames, an unrelated gst-play example, or CPU-only GStreamer is not success.

## Exact baseline
- WPE 2.54.2 native producer run 46029696057 and armv7 media bundle run 46091415540; 30 required factory PASS and 6 optional PASS (optional webvttparse absent).
- SL101 WebProcess currently dies during surfaceless EGL initialization; EGL-WPE lane owns the initial display fix. Work in parallel on GL integration and a deterministic video-frame acceptance harness without waiting for that lane.
- Existing NuraBrowser fixture scripts/fixtures/nurabrowser_tagesschau_hls_smoke.html uses actual ARD 360p HLS URL and reports requestVideoFrameCallback, getVideoPlaybackQuality, video dimensions, readyState, currentTime and errors to localhost heartbeat. Qualifier scripts/qualify_nurabrowser_nura_native.sh supports a separate private candidate root.
- WPE source pinned SHA256 f3284e19e47fcb5ac8c63c2e1c113dbd65b2423eb8a545f5b345707829004f56. Inspect Source/WebCore/platform/graphics/gstreamer/PlatformDisplayGStreamer.cpp, GLVideoSinkGStreamer.cpp, GStreamerVideoFrameConverter.cpp, VideoDecoderGStreamer.cpp, GStreamerVideoSinkCommon.cpp and media pipelines.
- Old historical SL101 VDE/GstGL/Grate experiments reported actual decoded frames: verify their exact current provenance, do not assume they apply unchanged to WPE.

## Scope and method
- Read AGENTS.md and mandatory repository docs; inspect active path claims; implement in an isolated claimed worktree/branch "nurabrowser-gstgl-pipeline-g0-20261128". Preserve the dirty main and other workers.
- Own GStreamer GL context propagation, GL video sink caps, memory types, frame conversion/presentation, WebKit media feature/build wiring and test fixtures. EGL-WPE lane owns generic PlatformDisplay bootstrap; DEVICE-RUNTIME owns staging/hardware library fixes.
- Source-debug GstGLDisplay/EGL context ownership and WPE media/video sink lifecycle, HLS/MPEG-TS/H.264/AAC decode, DRM/DMABuf/GLMemory interop and teardown. Decide whether an upstream-compatible patch, runtime plugin closure, or a narrowly scoped WebKit fix is needed; implement the executable fix and test it.
- Require USE_GSTREAMER_GL=ON; preserve ENABLE_VIDEO=ON, USE_GSTREAMER=ON, ENABLE_WEB_AUDIO=ON. Avoid software-only sink fallback masquerading as GPU success. Report actual GL caps, buffer type and frames, and discriminate GPU rendering from CPU decode/present.
- Add deterministic tests for GL-path selection, valid/invalid caps, context-loss cleanup, no-frame timeout and actual frame advancement. Source/path/log evidence and negative control mandatory for a PASS, but do not turn missing extra ceremony into a blocker for independent implementation.
- No production deploy/reboot/suspend, no interference with Sway or WayVNC, no broad cleanup, no obsolete 58 retired prompts, no recursive dispatch. relength_remaining=0.

## Acceptance and handoff
1. Real source changes + reproducible tests, commit SHA, ELF/ARMv7 constraints where build occurs. Build in native ARMv7 CI if permitted/needed, avoid unbounded local Docker cache.
2. Provide an executable GPU-frame verification recipe for DEVICE-RUNTIME: actual decoded frames and compositor presentation, not page-loaded or audio-only.
3. Coordinate integration file collisions with EGL-WPE via ws-bridge notes/claims; preserve independent branch until reviewed.
4. Exact canonical OCP agent_checkpoint with paths, tests, failed counterexamples and integration steps; never finish with a status-only matrix. No child task.
