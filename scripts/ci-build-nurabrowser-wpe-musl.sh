#!/bin/sh
# Alpine 3.22 ARMv7/QEMU *candidate* builder. Not a qualified runtime producer.
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

# Build on the target libc/ISA rather than mixing glibc and musl development
# sysroots. This may take several hours under QEMU. Dependencies are explicit.
apk add --no-cache \
  build-base binutils cmake ninja pkgconf python3 perl ruby bison flex gperf \
  git curl tar xz meson linux-headers \
  glib-dev harfbuzz-dev icu-dev libjpeg-turbo-dev libepoxy-dev \
  libgcrypt-dev libsoup3-dev libtasn1-dev libxkbcommon-dev libxml2-dev libxslt-dev \
  libpng-dev sqlite-dev libwebp-dev zlib-dev freetype-dev \
  json-glib-dev wayland-dev wayland-protocols mesa-dev libdrm-dev \
  bubblewrap xdg-dbus-proxy

mkdir -p /out/src /out/build /out/stage
printf '%s  %s\n' \
  f3284e19e47fcb5ac8c63c2e1c113dbd65b2423eb8a545f5b345707829004f56 \
  /work/target/wpe-source/wpewebkit-2.54.2.tar.xz | sha256sum -c -
tar -xJf /work/target/wpe-source/wpewebkit-2.54.2.tar.xz -C /out/src

# Tegra2: ARMv7 hardfloat, VFPv3-D16, no NEON or hardware division.
cpu_flags='-O2 -mcpu=cortex-a9 -mfpu=vfpv3-d16 -mfloat-abi=hard -mno-unaligned-access'
export CFLAGS="$cpu_flags"
export CXXFLAGS="$cpu_flags"

# Production WebKit embeds an absolute process path. It MUST match the
# regular APK install layout; WEBKIT_EXEC_PATH is developer-build-only.
cmake -S /out/src/wpewebkit-2.54.2 -B /out/build/webkit -G Ninja \
  -DPORT=WPE -DCMAKE_BUILD_TYPE=Release -DCMAKE_INSTALL_PREFIX=/usr \
  -DCMAKE_INSTALL_LIBDIR=lib \
  -DLIBEXEC_INSTALL_DIR=/usr/libexec/nuraloumi/browser/libexec \
  -DENABLE_WPE_PLATFORM=ON -DENABLE_WPE_PLATFORM_WAYLAND=ON \
  -DENABLE_WPE_PLATFORM_HEADLESS=ON -DENABLE_WPE_PLATFORM_DRM=OFF \
  -DENABLE_WPE_LEGACY_API=OFF -DENABLE_BUBBLEWRAP_SANDBOX=ON \
  -DENABLE_GPU_PROCESS=OFF -DUSE_GBM=OFF -DUSE_LIBDRM=OFF \
  -DENABLE_VIDEO=OFF -DENABLE_WEB_AUDIO=OFF -DENABLE_WEBGL=OFF \
  -DENABLE_MEDIA_STREAM=OFF -DENABLE_MEDIA_RECORDER=OFF \
  -DENABLE_INTROSPECTION=OFF -DENABLE_DOCUMENTATION=OFF \
  -DENABLE_SPELLCHECK=OFF -DENABLE_SPEECH_SYNTHESIS=OFF \
  -DENABLE_JOURNALD_LOG=OFF -DENABLE_JIT=OFF \
  -DUSE_ATK=OFF -DUSE_FLITE=OFF -DUSE_LIBHYPHEN=OFF -DUSE_LIBBACKTRACE=OFF -DUSE_JPEGXL=OFF \
  -DUSE_AVIF=OFF -DUSE_LCMS=OFF -DUSE_WOFF2=OFF \
  -DUSE_SYSTEM_UNIFDEF=OFF
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
