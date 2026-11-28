#!/bin/sh
# Alpine 3.22 native ARMv7 *candidate* builder. Not a qualified runtime producer.
# Called inside an isolated linux/arm/v7 container with /work read-only, /out writable.
set -eu

case "${CI_SOURCE_SHA:-}" in
  ????????????????????????????????????????)
    printf '%s' "$CI_SOURCE_SHA" | grep -Eq '^[0-9a-f]{40}$' ||
      { echo 'invalid source commit' >&2; exit 2; } ;;
  *) echo 'missing full source commit' >&2; exit 2 ;;
esac
test -f /work/target/wpe-source/wpewebkit-2.54.2.tar.xz
test -d /work/components/nura-browser-engine
test -d /out
test ! -e /out/stage
case "${CI_WPE_RESUME:-0}" in 0|1) ;; *) echo 'invalid resume mode' >&2; exit 2 ;; esac

# Build on the target libc/ISA rather than mixing glibc and musl development
# sysroots. Dependencies are explicit; hosted Arm64 must pass native AArch32 preflight.
apk add --no-cache \
  build-base binutils cmake ninja pkgconf python3 perl ruby bison flex gperf \
  git curl tar xz meson linux-headers \
  glib-dev harfbuzz-dev icu-dev libjpeg-turbo-dev libepoxy-dev \
  libgcrypt-dev libsoup3-dev libtasn1-dev libxkbcommon-dev libxml2-dev libxslt-dev hyphen-dev \
  libpng-dev sqlite-dev libwebp-dev zlib-dev freetype-dev \
  json-glib-dev wayland-dev wayland-protocols mesa-dev libdrm-dev \
  gettext-dev libseccomp-dev bubblewrap xdg-dbus-proxy
if [ "${CI_WPE_MEDIA:-0}" = 1 ]; then
  apk add --no-cache gstreamer-dev gst-plugins-base-dev gst-plugins-bad-dev \
    gst-plugins-good gst-plugins-bad gst-libav gst-plugins-base
  media_video=ON
  media_audio=ON
  media_gstreamer=ON
  media_gbm=ON
else
  media_video=OFF
  media_audio=OFF
  media_gstreamer=OFF
  media_gbm=OFF
fi

mkdir -p /out/src /out/build /out/stage
printf '%s  %s\n' \
  f3284e19e47fcb5ac8c63c2e1c113dbd65b2423eb8a545f5b345707829004f56 \
  /work/target/wpe-source/wpewebkit-2.54.2.tar.xz | sha256sum -c -

builder_sha256=$(sha256sum /work/scripts/ci-build-nurabrowser-wpe-musl.sh | cut -d ' ' -f 1)
packages_sha256=$(apk info -vv | LC_ALL=C sort | sha256sum | cut -d ' ' -f 1)
if [ "${CI_WPE_RESUME:-0}" = 1 ]; then
  # A restored Ninja tree must have precisely the same source, script and
  # Alpine package set. Otherwise rebuilding from scratch is mandatory.
  test -s /out/BUILD-PROVENANCE.txt
  grep -Fx "source_sha=$CI_SOURCE_SHA" /out/BUILD-PROVENANCE.txt
  grep -Fx "builder_sha256=$builder_sha256" /out/BUILD-PROVENANCE.txt
  grep -Fx "packages_sha256=$packages_sha256" /out/BUILD-PROVENANCE.txt
  test -s /out/build/webkit/build.ninja
  test -f /out/src/wpewebkit-2.54.2/CMakeLists.txt
  echo 'WPE_RESTORED_NINJA=VERIFIED'
else
  test ! -e /out/BUILD-PROVENANCE.txt
  test ! -e /out/src/wpewebkit-2.54.2
  tar -xJf /work/target/wpe-source/wpewebkit-2.54.2.tar.xz -C /out/src

# WPE 2.54.2 uses DRM_FORMAT_XRGB8888 in the WPEPlatform Wayland DMA-BUF
# backing-store path even when USE_LIBDRM=OFF, but this translation unit does
# not include the UAPI fourcc definitions in that configuration. Keep libdrm
# disabled for the target runtime and make the compile-time dependency explicit.
python3 - <<'PY'
from pathlib import Path

source = Path("/out/src/wpewebkit-2.54.2/Source/WebKit/UIProcess/wpe/AcceleratedBackingStore.cpp")
text = source.read_text()
needle = '#elif USE(LIBDRM)\n#include <drm_fourcc.h>\n'
replacement = '#else\n#include <libdrm/drm_fourcc.h>\n'
if replacement not in text:
    if needle not in text:
        raise SystemExit(f"unexpected WPE 2.54.2 source layout: {source}")
    source.write_text(text.replace(needle, replacement, 1))

# Link the Alpine gettext and Bubblewrap dependencies on the final shared target.
cmake = source.parents[2] / "CMakeLists.txt"
text = cmake.read_text()
needle = "WEBKIT_FRAMEWORK(WebKit)\n"
link = (
    "target_link_libraries(WebKit PRIVATE intl)\n"
    "if (ENABLE_BUBBLEWRAP_SANDBOX)\n"
    "    target_link_libraries(WebKit PRIVATE Libseccomp::Libseccomp)\n"
    "endif ()\n"
)
if link not in text:
    if text.count(needle) != 1:
        raise SystemExit(f"unexpected WPE 2.54.2 target layout: {cmake}")
    cmake.write_text(text.replace(needle, needle + link, 1))
PY
if [ "${CI_WPE_MEDIA:-0}" = 1 ]; then
  # The WebProcess tries its hardware GBM display before surfaceless EGL.
  # A supported client extension alone cannot guarantee eglInitialize.
  python3 /work/scripts/patch_wpe_egl_bootstrap.py /out/src/wpewebkit-2.54.2
fi
fi

# Tegra2: ARMv7 hardfloat, VFPv3-D16, no NEON or hardware division.
cpu_flags='-O2 -mcpu=cortex-a9 -mfpu=vfpv3-d16 -mfloat-abi=hard -mno-unaligned-access'
export CFLAGS="$cpu_flags"
export CXXFLAGS="$cpu_flags"

# An Arm64 kernel exposes aarch64 via uname even to genuine ARMv7 containers.
# WebKitCommon has a supported FORCE_32BIT path for this case: without it,
# WebKit incorrectly selects ARM64 allocator/JSC features for 32-bit objects.
# Explicitly choose the upstream ARM Thumb-2 mimalloc backend as well.
# Production WebKit embeds an absolute process path. It MUST match the
# regular APK install layout; WEBKIT_EXEC_PATH is developer-build-only.
cmake -S /out/src/wpewebkit-2.54.2 -B /out/build/webkit -G Ninja \
  -DPORT=WPE -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr \
  -DFORCE_32BIT=ON -DUSE_MIMALLOC=ON -DUSE_SYSTEM_MALLOC=OFF \
  -DCMAKE_INSTALL_LIBDIR=lib \
  -DLIBEXEC_INSTALL_DIR=/usr/libexec/nuraloumi/browser/libexec \
  -DENABLE_WPE_PLATFORM=ON -DENABLE_WPE_PLATFORM_WAYLAND=ON \
  -DENABLE_WPE_PLATFORM_HEADLESS=ON -DENABLE_WPE_PLATFORM_DRM=OFF \
  -DENABLE_WPE_LEGACY_API=OFF -DENABLE_BUBBLEWRAP_SANDBOX=ON \
  -DENABLE_GPU_PROCESS=OFF -DUSE_GBM="$media_gbm" -DUSE_LIBDRM="$media_gbm" \
  -DENABLE_VIDEO="$media_video" -DENABLE_WEB_AUDIO="$media_audio" -DENABLE_WEBGL=OFF -DUSE_GSTREAMER="$media_gstreamer" \
  -DUSE_GSTREAMER_GL="$media_gstreamer" \
  -DENABLE_MEDIA_STREAM=OFF -DENABLE_MEDIA_RECORDER=OFF \
  -DENABLE_INTROSPECTION=OFF -DENABLE_DOCUMENTATION=OFF \
  -DENABLE_SPELLCHECK=OFF -DENABLE_SPEECH_SYNTHESIS=OFF \
  -DENABLE_JOURNALD_LOG=OFF -DENABLE_JIT=OFF \
  -DUSE_ATK=OFF -DUSE_FLITE=OFF -DUSE_JPEGXL=OFF \
  -DUSE_AVIF=OFF -DUSE_LCMS=OFF -DUSE_WOFF2=OFF \
  -DUSE_LIBBACKTRACE=OFF -DUSE_SYSTEM_SYSPROF_CAPTURE=OFF -DUSE_SYSTEM_UNIFDEF=OFF
# Fail before the expensive Ninja graph if the kernel's AArch64 identity won
# over our actual ARMv7 userspace ABI, or no bmalloc implementation was chosen.
grep -Fx 'CMAKE_SYSTEM_PROCESSOR:INTERNAL=armv7l' /out/build/webkit/CMakeCache.txt >/dev/null || {
  echo 'WPE CMake did not select the ARMv7 target processor' >&2
  exit 2
}
if [ "${CI_WPE_MEDIA:-0}" = 1 ]; then
  for feature in ENABLE_VIDEO USE_GSTREAMER USE_GSTREAMER_GL USE_GBM USE_LIBDRM; do
    grep -Fx "$feature:BOOL=ON" /out/build/webkit/CMakeCache.txt >/dev/null || {
      echo "GPU media feature not enabled: $feature" >&2
      exit 2
    }
  done
fi
grep -Fx 'USE_MIMALLOC:BOOL=ON' /out/build/webkit/CMakeCache.txt >/dev/null || {
  echo 'WPE CMake did not select the ARMv7 mimalloc backend' >&2
  exit 2
}
grep -Fx 'USE_SYSTEM_MALLOC:BOOL=OFF' /out/build/webkit/CMakeCache.txt >/dev/null || {
  echo 'WPE CMake unexpectedly enabled conflicting system malloc' >&2
  exit 2
}
if [ "${CI_WPE_RESUME:-0}" = 0 ]; then
  printf 'source_sha=%s\nbuilder_sha256=%s\npackages_sha256=%s\nqualification=UNQUALIFIED\n' \
    "$CI_SOURCE_SHA" "$builder_sha256" "$packages_sha256" > /out/BUILD-PROVENANCE.txt
fi
# Compile the formerly failing object-library target first. This checks
# allocator macros on real WPEPlatform sources before the larger link graph.
cmake --build /out/build/webkit --target WPEPlatformHeadless --parallel 2
cmake --build /out/build/webkit --parallel 2
cmake --install /out/build/webkit
DESTDIR=/out/stage cmake --install /out/build/webkit

# Compile the project engine against the newly built WPEPlatform/WebKit.
meson setup /out/build/engine /work/components/nura-browser-engine \
  --prefix=/usr --libexecdir=libexec --buildtype=release
meson compile -C /out/build/engine -j 2
DESTDIR=/out/stage meson install -C /out/build/engine --no-rebuild

test -x /out/stage/usr/libexec/nuraloumi/browser/nurabrowser-core
test -x /out/stage/usr/libexec/nuraloumi/browser/libexec/WPEWebProcess
test -x /out/stage/usr/libexec/nuraloumi/browser/libexec/WPENetworkProcess

# Deliberately omit runtime.json and every PASS qualification: this install
# tree lacks complete shared-library/dlopen closure and device sandbox/pixel
# evidence, so package/xtask must reject it.
{
  echo 'schema=nurabrowser-wpe-musl-candidate-v1'
  echo "source_sha=$CI_SOURCE_SHA"
  echo 'upstream_wpewebkit=2.54.2'
  echo 'target=armv7-unknown-linux-musleabihf'
  echo 'runtime_qualification=UNQUALIFIED'
  echo 'device_deployment=FORBIDDEN'
  echo 'missing=transitive_elf_dlopen_sandbox_real_pixels'
} > /out/CANDIDATE-ONLY.txt
apk info -vv > /out/ALPINE-PACKAGES.txt
(cd /out/stage && find . -type f -exec sha256sum '{}' + | sort > /out/STAGED-SHA256SUMS.txt)
tar -C /out -cf /out/nurabrowser-wpe-musl-candidate.tar stage CANDIDATE-ONLY.txt ALPINE-PACKAGES.txt STAGED-SHA256SUMS.txt
sha256sum /out/nurabrowser-wpe-musl-candidate.tar > /out/SHA256SUMS.txt
echo 'WPE_ARMV7_MUSL_CANDIDATE=BUILT qualification=UNQUALIFIED'
