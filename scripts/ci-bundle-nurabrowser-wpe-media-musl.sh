#!/bin/sh
# Native ARMv7 Alpine 3.22 bundle of an exact, independently pinned WPE CI result.
# /work is read-only, /input is the producer artifact, /out is private output.
# This produces an UNQUALIFIED candidate, never a release or device deployment.
set -eu
case "${CI_BUNDLE_SOURCE_SHA:-}" in
    ????????????????????????????????????????) ;;
    *) echo "missing full source SHA" >&2; exit 2 ;;
esac
printf '%s' "$CI_BUNDLE_SOURCE_SHA" | grep -Eq '^[0-9a-f]{40}$' ||
    { echo "invalid source SHA" >&2; exit 2; }
printf '%s' "${CI_BUNDLE_TAR_SHA256:-}" | grep -Eq '^[0-9a-f]{64}$' ||
    { echo "invalid tar SHA256" >&2; exit 2; }
case "${CI_BUNDLE_RUN_ID:-}" in
    ''|*[!0-9]*) echo "invalid run ID" >&2; exit 2 ;;
esac
test "$(apk --print-arch)" = armv7 || { echo "not ARMv7 userspace" >&2; exit 2; }
test -f /input/nurabrowser-wpe-musl-candidate.tar
test -f /input/CANDIDATE-ONLY.txt
test -d /out
test ! -e /out/runtime-root
printf '%s  %s\n' "$CI_BUNDLE_TAR_SHA256" \
    /input/nurabrowser-wpe-musl-candidate.tar | sha256sum -c -
for expected in "source_sha=$CI_BUNDLE_SOURCE_SHA" \
    'upstream_wpewebkit=2.54.2' 'media_build_requested=1' \
    'runtime_qualification=UNQUALIFIED' 'device_deployment=FORBIDDEN'; do
    grep -Fx "$expected" /input/CANDIDATE-ONLY.txt >/dev/null || exit 2
done

# The runtime SONAME roots are from the proved older Alpine ARMv7 image.
# Only the WPE engine binary/library is replaced by the new CI media producer.
apk add --no-cache \
    alpine-baselayout alpine-keys alpine-release apk-tools busybox \
    bubblewrap ca-certificates font-dejavu icu-data-full libc-utils \
    mesa-dri-gallium xdg-dbus-proxy binutils \
    'so:libEGL.so.1' 'so:libepoxy.so.0' 'so:libexpat.so.1' \
    'so:libfontconfig.so.1' 'so:libfreetype.so.6' 'so:libgbm.so.1' \
    'so:libgcc_s.so.1' 'so:libgcrypt.so.20' 'so:libgio-2.0.so.0' \
    'so:libGLESv2.so.2' 'so:libglib-2.0.so.0' 'so:libgmodule-2.0.so.0' \
    'so:libgobject-2.0.so.0' 'so:libharfbuzz-icu.so.0' 'so:libharfbuzz.so.0' \
    'so:libhyphen.so.0' 'so:libicui18n.so.76' 'so:libicuuc.so.76' \
    'so:libintl.so.8' 'so:libjpeg.so.8' 'so:libjson-glib-1.0.so.0' \
    'so:libpng16.so.16' 'so:libseccomp.so.2' 'so:libsoup-3.0.so.0' \
    'so:libsqlite3.so.0' 'so:libstdc++.so.6' 'so:libtasn1.so.6' \
    'so:libwayland-client.so.0' 'so:libwayland-server.so.0' \
    'so:libwebp.so.7' 'so:libwebpdemux.so.2' 'so:libwebpmux.so.3' \
    'so:libxkbcommon.so.0' 'so:libxml2.so.2' 'so:libxslt.so.1' \
    'so:libz.so.1' \
    gstreamer gstreamer-tools gst-plugins-base gst-plugins-good \
    gst-plugins-bad gst-libav

mkdir -p /out/producer-extract /out/probe
tar -xf /input/nurabrowser-wpe-musl-candidate.tar -C /out/producer-extract
test ! -e /out/producer-extract/stage/usr/libexec/nuraloumi/browser/runtime.json
test -x /out/producer-extract/stage/usr/libexec/nuraloumi/browser/nurabrowser-core
root=/out/runtime-root
mkdir -p "$root"
cp -a /bin /etc /lib /sbin /usr /var "$root/"
mkdir -p "$root/candidate" "$root/proc" "$root/sys" \
    "$root/dev" "$root/run" "$root/tmp" "$root/home" "$root/opt"
cp -a /out/producer-extract/stage/usr/. "$root/usr/"
cp /input/CANDIDATE-ONLY.txt "$root/candidate/CANDIDATE-ONLY.txt"

# Alpine's build/container account files and privileged D-Bus helper do not
# belong in a portable, unprivileged browser runtime.  Remove them ONLY from
# the newly generated candidate root, before hashing or host-side packaging.
# This also prevents root-only files from silently breaking the runner's tar.
rm -f "$root/etc/shadow" "$root/etc/shadow-" \
    "$root/etc/crontabs/root" \
    "$root/usr/libexec/dbus-daemon-launch-helper" \
    "$root/lib/apk/db/lock"
unshippable=$(find "$root" -type f \( ! -perm /004 -o -perm /6000 \) -print -quit)
test -z "$unshippable" || {
    echo "unreadable or privileged runtime file: $unshippable" >&2
    exit 2
}
untraversable=$(find "$root" -type d ! -perm /005 -print -quit)
test -z "$untraversable" || {
    echo "untraversable runtime directory: $untraversable" >&2
    exit 2
}
test ! -e "$root/usr/libexec/nuraloumi/browser/runtime.json"
test -x "$root/usr/libexec/nuraloumi/browser/libexec/WPEWebProcess"
test -x "$root/usr/libexec/nuraloumi/browser/libexec/WPENetworkProcess"

lib="$root/usr/lib/libWPEWebKit-2.0.so.1.11.5"
readelf -h "$lib" | grep -F 'Machine:                           ARM'
readelf -A "$lib" | grep -F 'Tag_FP_arch: VFPv3-D16'
readelf -A "$lib" | grep -F 'Tag_ABI_VFP_args: VFP registers'
if readelf -A "$lib" | grep -E 'Tag_Advanced_SIMD_arch|Tag_DIV_use'; then
    echo "unsupported Tegra20 ISA attribute" >&2; exit 2
fi

# First nine are already proven against the live source on an ARMv7 container.
# Add the TS/HLS/MSE decode chain, audio conversion, GL/Wayland sink negotiation.
required_factories='h264parse aacparse avdec_h264 avdec_aac qtdemux hlsdemux souphttpsrc uridecodebin videoconvert tsdemux tsparse typefind playbin playbin3 decodebin parsebin appsrc appsink hlsdemux2 audioconvert audioresample videoscale queue2 multiqueue glupload glcolorconvert glimagesink waylandsink alsasink pulsesink'
optional_factories='dashdemux decodebin3 urisourcebin subparse webvttparse deinterlace videorate'
for factory in $required_factories; do
    gst-inspect-1.0 "$factory" > "/out/probe/factory-$factory.txt" 2>&1 ||
        { echo "missing required factory: $factory" >&2; exit 2; }
    printf '%s\tPASS\n' "$factory" >> /out/FACTORIES.tsv
done
for factory in $optional_factories; do
    if gst-inspect-1.0 "$factory" > "/out/probe/factory-$factory.txt" 2>&1; then
        printf '%s\tPASS-OPTIONAL\n' "$factory" >> /out/FACTORIES.tsv
    else
        printf '%s\tMISSING-OPTIONAL\n' "$factory" >> /out/FACTORIES.tsv
    fi
done

export LD_LIBRARY_PATH="$root/usr/lib:$root/lib"
for elf in "$lib" \
    "$root/usr/libexec/nuraloumi/browser/nurabrowser-core" \
    "$root/usr/libexec/nuraloumi/browser/libexec/WPEWebProcess" \
    "$root/usr/libexec/nuraloumi/browser/libexec/WPENetworkProcess" \
    "$root/usr/lib/gstreamer-1.0/libgstlibav.so" \
    "$root/usr/lib/gstreamer-1.0/libgstisomp4.so" \
    "$root/usr/lib/gstreamer-1.0/libgstadaptivedemux2.so"; do
    test -f "$elf"
    ldd "$elf" >> /out/probe/ELF-NEEDED.txt 2>&1
done
if grep -E 'Error loading shared library|Error relocating' /out/probe/ELF-NEEDED.txt; then
    echo "unresolved mandatory ELF dependency" >&2; exit 2
fi
apk info -vv | LC_ALL=C sort > /out/ALPINE-RUNTIME-PACKAGES.txt
( cd "$root" && find . -type f -exec sha256sum {} + | LC_ALL=C sort ) \
    > /out/RUNTIME-SHA256SUMS.txt
{
    echo 'schema=nurabrowser-wpe-media-runtime-candidate-v1'
    echo "producer_source_sha=$CI_BUNDLE_SOURCE_SHA"
    echo "producer_run_id=$CI_BUNDLE_RUN_ID"
    echo "producer_tar_sha256=$CI_BUNDLE_TAR_SHA256"
    echo 'target=armv7-unknown-linux-musleabihf'
    echo 'media_factories=VERIFIED_ON_NATIVE_ARMV7'
    echo 'runtime_qualification=UNQUALIFIED'
    echo 'device_deployment=FORBIDDEN'
    echo 'missing=independent_isa_dlopen_device_play_compositor_pixel_safety'
} > /out/BUNDLE-PROVENANCE.txt
echo MEDIA_RUNTIME_BUNDLE=HOST_PREFLIGHT_PASS_DEVICE_NOT_RUN
