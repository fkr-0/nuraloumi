# Repair the WPE media WebProcess GPU-EGL bootstrap on SL101

project: nuraloumi
lane: EGL-WPE
parallel: 1
relength: 0
generation: G0
priority: 99

## Goal
Fix the **actual** WPE WebKit 2.54.2 media-enabled WebProcess GPU/EGL startup on the Tegra20 SL101. Preserve an enabled and operational GStreamer-GL path. A CPU-only build or merely suppressing the crash is **not acceptance**.

## Reproduced starting evidence
- Native ARMv7 media WPE CI producer 46029696057 (commit 76aa5046ce1d1da1de691e126b2491cbc4ea9654), media bundle run 46091415540 (commit a273856); 30 required + 6 optional GStreamer factories passed.
- In isolated SL101 bwrap test the media-enabled libWPEWebKit-2.0.so.1.11.5 crashes before page load: "Could not create surfaceless EGL display: EGL_NOT_INITIALIZED. Aborting..." followed by WebProcess crash. Repro evidence under SL101 private sandbox qualification runs native-moNgGf / native-BMJKbJ / native-BgLemM / native-AalGCJ / native-MgifjI.
- A/B: replacing **only** that WPE library in a private test clone with prior video-disabled WPE gives QUAL_READY and QUAL_HOLD_COMPLETED=20, unchanged production/Sway/WayVNC. This proves a media-WPE-specific failure surface, not a successful video test.
- Source: pinned tar target/wpe-source/wpewebkit-2.54.2.tar.xz, SHA256 f3284e19e47fcb5ac8c63c2e1c113dbd65b2423eb8a545f5b345707829004f56. Relevant WebProcessGLib.cpp initializePlatformDisplayIfNeeded and PlatformDisplaySurfaceless.cpp create; CMake GStreamerDefinitions.cmake defaults USE_GSTREAMER_GL=ON with video.
- The CPU-only diagnostic branch 90aa31b explicitly sets USE_GSTREAMER_GL=OFF and **must not** become the GPU solution.

## Ownership and execution
- Read AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md and active claims. Main is dirty; work in a unique isolated governed worktree and claim only your own paths; do not overwrite any active worker modifications.
- Own the **WPE-side EGL bootstrap/PlatformDisplay** repair and small source patch application in scripts/ci-build-nurabrowser-wpe-musl.sh or a dedicated guarded patch script. Coordinate file overlap with lane GST-PIPELINE; avoid modifying GStreamer sink code or the device's Mesa library collection.
- Trace actual eglQueryString/eglGetPlatformDisplay/eglInitialize and renderer transport paths against the 2.54.2 source and SL101 evidence. Distinguish false EGL platform selection, missing native display, failing DRI driver, wrong explicit-vs-implicit GL context, and a genuine absence of supported GPU EGL. Implement the smallest source-backed positive fix.
- Run a falsifying pre-fix test and a meaningful after-fix test; add a regression that fails with wrong EGL initialization. Preserve USE_GSTREAMER_GL=ON, ENABLE_VIDEO=ON, USE_GSTREAMER=ON. Build using authorized native ARMv7 CI when feasible; do not burn local disk on unbounded WPE rebuild.
- Continue independent fixes after a failed probe; only verified mandatory permission/safety/hardware stops can terminate execution. Do not write a status-only NO-GO.
- Never change production binaries, Sway, WayVNC, boot, or power state. No 30-minute soak; lane DEVICE-RUNTIME coordinates the single real test. No old prompt retries, no children and no speculative 40GiB gate. relength_remaining=0.

## Acceptance and handoff
1. Durable patch/commit on branch "nurabrowser-gstgl-egl-wpe-g0-20261128", exact source SHA and test commands.
2. Demonstrate EGL display/context bootstrap with GStreamer-GL still enabled and an explicit wrong-platform negative; report actual ARMv7 compile evidence separately from physical-device proof.
3. Include an integration recipe consumable by DEVICE-RUNTIME, crash logs before/after, known residual issue if any.
4. Write exact canonical OCP agent_checkpoint with artifacts and tests. No prose-only completion, no automatic child dispatch.
