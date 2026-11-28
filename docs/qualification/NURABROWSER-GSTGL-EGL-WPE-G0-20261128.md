# WPE 2.54.2 WebProcess EGL bootstrap — G0 source repair

## Immutable baseline and observed failure

- Original archive: `target/wpe-source/wpewebkit-2.54.2.tar.xz`;
  SHA256 `f3284e19e47fcb5ac8c63c2e1c113dbd65b2423eb8a545f5b345707829004f56`.
- Prior isolated SL101 media-enabled WebProcess: `Could not create surfaceless EGL display:
  EGL_NOT_INITIALIZED. Aborting...`, before page load. Previous video-disabled
  library A/B reached `QUAL_HOLD_COMPLETED=20`; **not** a video success.
- Original `WebProcessGLib.cpp::initializePlatformDisplayIfNeeded` chooses GBM
  only under `USE(GBM)` and hardware renderer transport; otherwise tries
  `PlatformDisplaySurfaceless::create`. The latter advertises
  `EGL_MESA_platform_surfaceless` from `eglQueryString(EGL_NO_DISPLAY, ...)`,
  then invokes `eglGetPlatformDisplayEXT`/`eglGetPlatformDisplay` and
  `GLDisplay::create`, whose `eglInitialize` can fail independently of client
  extension advertisement. Upstream source unconditionally `CRASH()`ed there.

## G0 source change and intent

The media producer enables `USE_GBM=ON`, `USE_LIBDRM=ON`,
`USE_GSTREAMER_GL=ON`, `USE_GSTREAMER=ON`, and `ENABLE_VIDEO=ON` explicitly,
and fails if the CMake cache disagrees. WPE already implements the positive
`DRMDeviceManager::mainGBMDevice(Render)` → `PlatformDisplayGBM::create`
path for hardware renderer transport. The guarded source patch allows a
failed surfaceless `eglInitialize` to return `nullptr` instead of aborting
inside the platform candidate. It does **not** silently select software GL.

The overall WebProcess fallback still crashes if no platform is usable;
this is deliberate: a functioning GPU display/context is required for the
media acceptance, and the patch is not evidence that one exists. In
particular, `m_rendererBufferTransportMode` may be empty, the sandbox may
not expose a render node, the Grate Mesa driver may fail initialization,
or EGL context creation may fail later. These alternatives require the
DEVICE-RUNTIME lane's bounded read-only/isolated probes.

## Reproduction and integration

1. Confirm original archive SHA256 above; use `CI_WPE_MEDIA=1` with the
   updated `scripts/ci-build-nurabrowser-wpe-musl.sh` and
   `scripts/patch_wpe_egl_bootstrap.py` in the same `/work/scripts` mount.
   Build in fresh staging; do **not** resume Ninja across changed builder SHA.
2. Verify `CMakeCache.txt` has `USE_GBM`, `USE_LIBDRM`, `USE_GSTREAMER_GL`,
   `USE_GSTREAMER`, `ENABLE_VIDEO` all `BOOL=ON`; audit native ARMv7 ELF and
   dependency closure separately. Do not install or replace live device code.
3. DEVICE-RUNTIME: use one private bwrap test with unchanged production
   Sway/WayVNC and staged library closure. Capture client
   `eglQueryString(EGL_NO_DISPLAY, EGL_EXTENSIONS)`,
   `eglGetPlatformDisplay[EXT]`, `eglInitialize` result/error for both GBM
   and surfaceless; record DRM render-node accessibility, GBM device choice,
   `m_rendererBufferTransportMode`, `EGL_VENDOR`, renderer string and context
   creation, followed by actual HTMLVideoElement frame progress.
4. Negative control: force an invalid EGL platform in **private** test
   process and assert the selected display initialization fails with an
   explicit error; it must not count as GPU PASS or a page/video result.
   Preserve original crash log and collect the exact post-patch log.

## Tests and outstanding evidence

- `sh -n scripts/ci-build-nurabrowser-wpe-musl.sh` — PASS.
- `python3 -m unittest discover -s scripts -p 'test_patch_wpe_egl_bootstrap.py' -v`
  — 3 tests PASS; includes wrong-platform source negative and idempotence.
- `python3 -m unittest discover -s scripts -p 'test_wpe_musl_producer_ci.py' -v`
  — 11 existing tests PASS.
- No native ARMv7 compile or post-patch SL101 EGL/context/video evidence yet;
  only source and host regression are verified. No device state mutated.
