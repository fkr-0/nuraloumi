#!/bin/sh
# Build the private NuraLoumi WPE engine inside the pinned ARMHF CI image.
set -eu
sha=$CI_SOURCE_SHA
case "$sha" in
  *[!0123456789abcdef]*|'') echo "invalid source commit" >&2; exit 2 ;;
esac
[ "$(printf '%s' "$sha" | wc -c)" -eq 40 ] || { echo "source commit must be 40 hex digits" >&2; exit 2; }
[ -f /src/meson.build ] && [ -d /out ] || {
  echo "expected read-only engine source /src and output /out" >&2
  exit 2
}
[ ! -e /out/build ] && [ ! -e /out/release ] || {
  echo "output already contains a build or release" >&2; exit 2
}
# ARMHF cross-link and instruction audit; never an on-device runtime claim.
sh /src/scripts/build-armhf.sh / /out/build
DESTDIR=/out/stage meson install -C /out/build
installed=/out/stage/usr/libexec/nuraloumi/browser/nurabrowser-core
[ -x "$installed" ] || { echo "missing private browser engine" >&2; exit 1; }
arm-linux-gnueabihf-readelf -d "$installed" > /out/ELF-NEEDED.txt
grep -F 'libWPEWebKit-2.0.so.1' /out/ELF-NEEDED.txt >/dev/null
arm-linux-gnueabihf-readelf -A "$installed" > /out/ELF-ATTRIBUTES.txt
arm-linux-gnueabihf-readelf -h "$installed" > /out/ELF-HEADER.txt
mkdir -p /out/release/libexec/nuraloumi/browser
install -m 0755 "$installed" /out/release/libexec/nuraloumi/browser/nurabrowser-core
cat > /out/release/CI-PROVENANCE.txt <<EOF
schema=nuraloumi-browser-engine-ci-v1
source_sha=$sha
target=armv7-linux-gnueabihf-vfpv3-d16-no-neon
web_process_sandbox=unqualified
dependency_instruction_closure=unqualified
device_deployment=forbidden
entry=libexec/nuraloumi/browser/nurabrowser-core
EOF
(cd /out/release && sha256sum libexec/nuraloumi/browser/nurabrowser-core > ENGINE-SHA256SUMS.txt)
(cd /out && tar --sort=name --mtime='@0' --owner=0 --group=0 --numeric-owner -cf nurabrowser-engine-armhf.tar release)
(cd /out && sha256sum nurabrowser-engine-armhf.tar > SHA256SUMS.txt)
dpkg-query -W -f='${Package}\t${Version}\t${Architecture}\n' libwpewebkit-2.0-1:armhf libwpewebkit-2.0-dev:armhf > /out/WPE-PACKAGES.tsv
printf 'NURABROWSER_ENGINE_CI_BUILD=PASS sha=%s runtime=UNQUALIFIED\n' "$sha"
