#!/bin/sh
# Fetch the pinned upstream WPE 2.54.2 source; never run unverified downloads.
set -eu

readonly_version=2.54.2
readonly_sha=f3284e19e47fcb5ac8c63c2e1c113dbd65b2423eb8a545f5b345707829004f56
out=${1:?usage: ci-fetch-nurabrowser-wpe-source.sh OUTPUT_DIRECTORY}
mkdir -p "$out"
archive="$out/wpewebkit-$readonly_version.tar.xz"
curl --fail --location --retry 3 --connect-timeout 20 --max-time 300 \
  --output "$archive" "https://wpewebkit.org/releases/wpewebkit-$readonly_version.tar.xz"
printf '%s  %s\n' "$readonly_sha" "$archive" | sha256sum -c -
tar -tJf "$archive" >/dev/null
printf 'WPE_SOURCE=PASS version=%s sha256=%s\n' "$readonly_version" "$readonly_sha"
