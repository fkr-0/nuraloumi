#!/bin/sh
# Run inside a clean Alpine arm/v7 container with abuild installed.
# Never copies a signing private key into output and never installs on SL101.
set -eu
stage=${1:?usage: build-ci-alpine-apk.sh STAGE OUTPUT_DIR EXPECTED_SHA}
out=${2:?missing output directory}
expected_sha=${3:?missing expected Git SHA}
manifest="$stage/MANIFEST.txt"
test -f "$manifest" && test -d "$stage/bin"
grep -Fx 'schema=nuraloumi-package-manifest-v1' "$manifest" >/dev/null
grep -Fx 'target=armv7-unknown-linux-musleabihf' "$manifest" >/dev/null
grep -Fx 'git_dirty=false' "$manifest" >/dev/null
grep -Fx "git_commit=$expected_sha" "$manifest" >/dev/null
test "$(abuild -A)" = armv7
version=$(sed -n 's/^version=//p' "$manifest")
case "$version" in
  *[!0-9a-zA-Z.+-]*|'') echo 'invalid version' >&2; exit 1;;
esac
# APK versions allow _preN, but not the arbitrary _devN SemVer suffix.
# 0.0.4-dev.2 thus becomes 0.0.4_pre2, below the stable 0.0.4.
apkver=$(printf '%s' "$version" | sed 's/-dev\./_pre/')
mkdir -p "$out"
out=$(cd "$out" && pwd)
tmp=$(mktemp -d /tmp/nuraloumi-abuild.XXXXXXXX)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
cd "$tmp"
sed "s/@PKGVER@/$apkver/g" /work/packaging/alpine/APKBUILD.in > APKBUILD
# abuild expects local source files and verifies their SHA512 checksums.
mkdir bundle
cp -R "$stage"/. bundle/
tar -czf nuraloumi-stage.tar.gz bundle
rm -rf bundle
# Explicitly use a persistent release key only when the CI provided an
# externally mounted read-only key directory. Never place its private key
# in the source checkout or under the uploaded output directory.
export PACKAGER='NuraLoumi CI <ci@nuraloumi.invalid>'
mkdir -p /root/.abuild
if [ -n "${NURALOUMI_APK_RELEASE_KEYS_DIR:-}" ]; then
    private="$NURALOUMI_APK_RELEASE_KEYS_DIR/nuraloumi-release.rsa"
    public="$NURALOUMI_APK_RELEASE_KEYS_DIR/nuraloumi-release.rsa.pub"
    test -s "$private" && test -s "$public"
    test -n "${NURALOUMI_APK_RELEASE_PUBLIC_SHA256:-}"
    expected_pub_sha="$NURALOUMI_APK_RELEASE_PUBLIC_SHA256"
    actual_pub_sha=$(sha256sum "$public" | cut -d' ' -f1)
    test "$expected_pub_sha" = "$actual_pub_sha" || {
        echo 'FAIL: signing certificate public SHA256 does not match expected fingerprint' >&2
        exit 1
    }
    # A matching public key alone is insufficient. Independently derive it
    # from the supplied private key and compare its RSA parameters.
    openssl pkey -in "$private" -pubout -out "$tmp/derived-public.pem" 2>/dev/null
    openssl pkey -pubin -in "$public" -pubout -out "$tmp/supplied-public.pem" 2>/dev/null
    cmp "$tmp/derived-public.pem" "$tmp/supplied-public.pem"
    install -m0600 "$private" /root/.abuild/nuraloumi-release.rsa
    install -m0644 "$public" /root/.abuild/nuraloumi-release.rsa.pub
    export PACKAGER_PRIVKEY=/root/.abuild/nuraloumi-release.rsa
    signing_mode=protected-release
    key=/root/.abuild/nuraloumi-release.rsa.pub
else
    # CI-only development key: not trusted on devices. No production release
    # event may use this fallback (the workflow fails closed for release tags).
    abuild-keygen -a -n -b 2048 >/dev/null
    signing_mode=ephemeral-ci-development-only
    key=$(find /root/.abuild -name '*.rsa.pub' -type f -print -quit)
fi
# Only the disposable build container trusts its temporary signing key so
# abuild can verify the package before generating the local repository index.
# The physical devices never receive this key automatically.
test -n "$key"
install -m0644 "$key" "/etc/apk/keys/${key##*/}"
# Force-root is permitted only because this is an isolated, ephemeral CI
# container; -d skips unused build dependencies in this copy-only recipe.
abuild -F checksum
abuild -F -d -P "$out"
pkg=$(find "$out" -type f -name "nuraloumi-$apkver-r0.apk" -print -quit)
test -n "$pkg" && test -s "$pkg"
cp "$key" "$out/CI-EPHEMERAL-KEY.rsa.pub"
apk --keys-dir /root/.abuild verify "$pkg"
relative_pkg=${pkg#"$out"/}
(cd "$out" && sha256sum "$relative_pkg") > "$out/NURALOUMI-APK-SHA256SUMS.txt"
cat > "$out/CI-APK-SIGNING-POLICY.txt" <<EOF
SIGNING_MODE=$signing_mode
GIT_SHA=$expected_sha
PUBLIC_KEY_SHA256=$(sha256sum "$key" | cut -d' ' -f1)
The private signing key is never included in the published artifact.
If signing_mode=ephemeral-ci-development-only the private key is destroyed
with this container and is NOT a trusted production identity.
If signing_mode=protected-release this package was signed using a pinned
GitHub Actions release secret and the public-key fingerprint above.
Never automatically enroll new keys in the device's /etc/apk/keys or
use --allow-untrusted in a deployment script.
EOF
echo "APK_PACKAGE=PASS path=$pkg sha=$expected_sha signing=$signing_mode"
