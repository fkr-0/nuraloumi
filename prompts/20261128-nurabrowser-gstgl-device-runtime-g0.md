# Fix SL101 Mesa/EGL runtime closure and qualify hardware GStreamer-GL playback

project: nuraloumi
lane: DEVICE-RUNTIME
parallel: 1
relength: 0
generation: G0
priority: 99

## Goal
Make the **real** SL101 NuraBrowser media-enabled WebProcess initialize a working GPU EGL/DRI display and render actual Tagesschau HLS frames for 60 seconds in a private, reversible test. Diagnose and fix the device Mesa/GStreamer-GL runtime and sandbox permissions, then integrate independently reviewed EGL-WPE and GST-PIPELINE patches when ready. CPU-only video is not an acceptable final result.

## Reproduced physical baseline
- Approved device SSH alias from the project's existing configuration; architecture ARMv7 VFPv3-D16/no NEON. Production release/Sway/WayVNC were unchanged during previous tests.
- A private original CI runtime staging root from bundle run 46091415540 exists on device; discover the exact owner-scoped path from current device records, matching artifact SHA256 293de88a7c77f7865ece02c6617e851bd275de1ab0a65621f5440039a879e2b1.
- A private disposable Mesa A/B clone exists; discover its exact owner-scoped path rather than assuming this prompt is path authority. Prior investigation overlaid *known working D16* libEGL/libGLES/libgbm/libgallium/LLVM and crypto/ffi/xml2, quarantined only Alpine25 dri_gbm.so. Generic library swaps alone did not fix media-WPE WebProcess EGL_NOT_INITIALIZED.
- A/B with old video-disabled WPE library passed 20s startup. The clone was last left with this **old library**: restore new media-enabled library from original private CI root before claiming media success. No fake 60s result.
- Existing root test: scripts/qualify_nurabrowser_nura_native.sh with NURA_QUAL_ROOT pointing to the verified absolute private root and a bounded 60s soak; fixture scripts/fixtures/nurabrowser_tagesschau_hls_smoke.html. Real ARD HLS URL is embedded in fixture; verify actual frames and pixels.
- Concurrent WPE and GStreamer source repair workers have separate ownership. CPU-only diagnostic branch 90aa31b is **not** a GPU fix.

## Execute safely, not as a gate-review
- Read AGENTS.md and mandatory project docs; inspect current device identity, SSH host key, compositor, resource/thermal state, active claims, then use your own claimed isolated worktree and uniquely named **private** device staging; do not edit the shared private roots in place without ownership.
- Own runtime Mesa/DRI/GBM/libEGL and GstGL closure, library provenance, DRM node visibility/bwrap device permissions, loader and actual EGL initialization. Capture bounded diagnostic EGL extension/client platform/provider and eglInitialize results. Confirm whether surfaceless, GBM or Wayland display is valid on Tegra20; do not infer from a library filename. Fix the smallest actual driver/plugin/version/permission mismatch.
- Keep running independent bounded probes even if source workers are still compiling; when usable code arrives, integrate on a separate reviewed branch/private root, no blind overwrites. No speculative mandatory 40GiB, signer/touch custodian or production rollout gate for a private smoke.
- Real video PASS requires consecutive 60 seconds with browser live, decoded frame progression, nonzero video dimensions, advancing currentTime, actual compositor video pixels or captured frame changes, bounded resource/thermal probes and no WebProcess crash. Distinguish GPU rendering from CPU-only fallback using EGL renderer/DRI/GstGL caps evidence. Keep an explicit falsifying no-frame/invalid-EGL negative.
- Hard safety: no production symlink/release/boot/session change, no compositor/WayVNC restart, no suspend/reboot, no kernel/device risk, no broad cleanup. Avoid concurrent physical smoke with other workers; reserve only one supervised test. No old task retries and no follow-up child; relength_remaining=0.
- If not all paths are ready, deliver a concrete reversible runtime fix/diagnostic tool or patch and verified standalone EGL test; never substitute status-only NO-GO.

## Acceptance and handoff
1. New committed device/runtime or test-harness fix on branch "nurabrowser-gstgl-device-runtime-g0-20261128" with exact commands and before/after logs.
2. Physical isolated EGL initialization + GPU-backed media evidence; record 60s PASS only when all actual frame/pixel conditions pass, otherwise exact failing step and next runnable fix already attempted.
3. No modification to the running NuraLoumi release, Sway or WayVNC; report independent baseline comparison.
4. Exact canonical OCP agent_checkpoint with artifacts, measured evidence and integration notes. Do not create children.
