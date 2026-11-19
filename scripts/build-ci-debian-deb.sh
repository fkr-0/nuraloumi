#!/bin/sh
# Convert the CI-audited Debian armhf staging bundle into a real .deb.
# This script MUST NOT compile, deploy, run maintainer hooks or touch a tablet.
set -eu
stage=${1:?usage: build-ci-debian-deb.sh STAGE OUTPUT_DIR EXPECTED_SHA}
out=${2:?missing output directory}
expected_sha=${3:?missing expected Git SHA}
manifest="$stage/MANIFEST.txt"
test -f "$manifest" && test -d "$stage/bin"
grep -Fx 'schema=nuraloumi-package-manifest-v1' "$manifest" >/dev/null
grep -Fx 'target=armv7-unknown-linux-gnueabihf' "$manifest" >/dev/null
grep -Fx 'git_dirty=false' "$manifest" >/dev/null
grep -Fx "git_commit=$expected_sha" "$manifest" >/dev/null
version=$(sed -n 's/^version=//p' "$manifest")
case "$version" in
  *[!0-9a-zA-Z.+-]*|'') echo 'invalid version' >&2; exit 1;;
esac
# Debian orders "~dev" and "~rc" before the corresponding stable release.
deb_version=$(printf '%s' "$version" | sed 's/-dev\./~dev./;s/-rc\./~rc./')
mkdir -p "$out"
out=$(cd "$out" && pwd)
tmp=$(mktemp -d "$out/.deb-stage.XXXXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
pkg="$tmp/pkg"
install -d "$pkg/DEBIAN" "$pkg/usr/bin" "$pkg/usr/share/applications" "$pkg/usr/share/dbus-1/services" "$pkg/usr/share/nuraloumi" "$pkg/usr/share/doc/nuraloumi"
count=0
for executable in "$stage"/bin/nuraloumi-*; do
  test -f "$executable" && test -x "$executable" && test ! -L "$executable" || {
    echo "missing or unsafe staged executable: ${executable##*/}" >&2
    exit 1
  }
  install -m 0755 "$executable" "$pkg/usr/bin/${executable##*/}"
  count=$((count+1))
done
test "$count" -ge 4
# Every packaged QoL launcher must already be an audited staging input.
for required in nuraloumi-player.desktop nuraloumi-files.desktop nuraloumi-notifications.desktop; do
  test -f "$stage/share/applications/$required" && test ! -L "$stage/share/applications/$required" || {
    echo "missing or unsafe required desktop entry: $required" >&2
    exit 1
  }
done
desktop_count=0
for desktop in "$stage"/share/applications/*.desktop; do
  test -f "$desktop" && test ! -L "$desktop"
  install -m 0644 "$desktop" "$pkg/usr/share/applications/${desktop##*/}"
  desktop_count=$((desktop_count+1))
done
test "$desktop_count" -ge 3
service="$stage/share/dbus-1/services/org.freedesktop.Notifications.service"
test -f "$service" && test ! -L "$service" || {
  echo "missing or unsafe D-Bus activation descriptor" >&2
  exit 1
}
grep -Fx 'Exec=/usr/bin/nuraloumi-notificationd' "$service" >/dev/null
test -x "$stage/bin/nuraloumi-notificationd" && test ! -L "$stage/bin/nuraloumi-notificationd" || {
  echo "missing or unsafe notificationd executable" >&2
  exit 1
}
install -m 0644 "$service" "$pkg/usr/share/dbus-1/services/org.freedesktop.Notifications.service"
install -m 0644 "$stage/config/nuraloumi.toml.example" "$pkg/usr/share/nuraloumi/nuraloumi.toml.example"
install -d "$pkg/usr/share/nuraloumi/qualification"
install -m 0644 "$stage/share/nuraloumi/qualification/README.md" "$pkg/usr/share/nuraloumi/qualification/README.md"
install -m 0644 "$stage/MANIFEST.txt" "$pkg/usr/share/doc/nuraloumi/CI-MANIFEST.txt"
install -m 0644 "$stage/LICENSE" "$pkg/usr/share/doc/nuraloumi/copyright"
install -m 0644 "$stage/README.md" "$pkg/usr/share/doc/nuraloumi/README"
cat > "$pkg/DEBIAN/control" <<EOF
Package: nuraloumi
Version: $deb_version
Section: utils
Priority: optional
Architecture: armhf
Maintainer: NuraLoumi maintainers <26308101+fkr-0@users.noreply.github.com>
Depends: libc6 (>= 2.41), libcairo2, libgcc-s1, libwayland-client0, libxkbcommon0
Description: Native Wayland touch shell for Tegra20 ARMv7
 NuraLoumi's standalone Cairo/wl_shm surfaces for the SL101.
 This package does not enable or modify system services.
 CI provenance: $expected_sha.
EOF
chmod 0644 "$pkg/DEBIAN/control"
filename="nuraloumi_${deb_version}_armhf.deb"
SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH:-0} dpkg-deb --root-owner-group --build "$pkg" "$out/$filename"
test "$(dpkg-deb --field "$out/$filename" Architecture)" = armhf
test "$(dpkg-deb --field "$out/$filename" Version)" = "$deb_version"
mkdir "$tmp/extracted"
dpkg-deb --extract "$out/$filename" "$tmp/extracted"
for executable in "$stage"/bin/nuraloumi-*; do
  cmp "$executable" "$tmp/extracted/usr/bin/${executable##*/}"
done
for desktop in "$stage"/share/applications/*.desktop; do
  cmp "$desktop" "$tmp/extracted/usr/share/applications/${desktop##*/}"
done
cmp "$service" "$tmp/extracted/usr/share/dbus-1/services/org.freedesktop.Notifications.service"
(cd "$out" && sha256sum "$filename") > "$out/$filename.sha256"
echo "DEB_PACKAGE=PASS path=$out/$filename binaries=$count sha=$expected_sha"
