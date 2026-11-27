#!/bin/sh
# Rebuild a private, source-pinned ARMv7/musl crypto overlay inside Alpine 3.22.
# Run in an isolated linux/arm/v7 container; never install onto the build host
# or SL101. All inputs are read-only and the output directory must be fresh.
set -eu

if [ "$#" -ne 2 ]; then
  echo "usage: $0 INPUT_ARCHIVES_DIR FRESH_OUTPUT_DIR" >&2
  exit 2
fi
inputs=$1
out=$2
build_jobs=${NURA_BUILD_JOBS:-1}
case "$build_jobs" in
  ''|*[!0-9]*) echo 'invalid NURA_BUILD_JOBS: expected integer 1..20' >&2; exit 2 ;;
esac
[ "$build_jobs" -ge 1 ] && [ "$build_jobs" -le 20 ] || {
  echo 'invalid NURA_BUILD_JOBS: expected integer 1..20' >&2
  exit 2
}
test -d "$inputs"
test -d "$out"
test ! -e "$out/work"
case "$(cc -dumpmachine)" in
  armv7*musleabihf*) ;;
  *) echo 'not native ARMv7 musl: aborting' >&2; exit 2 ;;
esac
for tool in tar sha512sum patch make perl readelf; do
  command -v "$tool" >/dev/null
done
test -r /usr/include/gmp.h
test -r /usr/include/gpg-error.h

# Exact Alpine 3.22 stable package source pins. OpenSSL includes the 3.5.9-r2
# security patchset; do not fall back to the older r0/r1 libraries for release.
check_source() {
  printf '%s  %s\n' "$2" "$inputs/$1" | sha512sum -c -
}
check_source openssl-3.5.9.tar.gz c4c8136fd2c98d13bb85a57bfb0def25c906f47f1fe97675cd114594944eab957b5b81a3e0f48826e1a0f093afff5e83766b539d49768f6db68310fd678d7de6
check_source nettle-3.10.2.tar.gz bf37ddd7dca8e78488da2a5286dcf16761d527d620572b42f2ad27bb8ee8c12999d92b0272e06f53766e7155a3f4a1ab7ad9c4b1c3caec47c031878b6b1772fb
check_source libgcrypt-1.10.3.tar.bz2 8a8d4c61a6622d8481ceb9edc88ec43f58da32e316f79f8d4775325a48f8936aaa9eb355923b39e2c267b784e9c390600daeb62e0c94f00e30bbadb0d8c0865d
check_source gcc14-armv7.patch edc0679c0b324f54fadcb7e6e4f902eb2a846aeb886423c586dbcaa90d6bc9164d85d8508df8b888de7893ae5bec0401c4c67d63b345c7370e1c2aec8ddf4a9f
check_source auxv.patch b2541075148fd5af4552d34158deb1a325f5adced90626dc03fd126f47323cde949b15b2657523c34b26530b322312a43bd42ada7295b4f2dd1d3b5d11892c62
check_source CVE-2026-88822.patch d4d7b8e740384cc4b129bbc53eb92c61abebb009b83a7c434ec5d0820021b95d402245a4d3d577ea28e0935d8de96d339eec6a6411ef00f3b9d982ccf0718721
check_source CVE-2026-88823.patch ca38eeb1aa261d1051100b0e37a9699ccad73de551825bf49ade50af9396b1e749983f802fa3967f67ef778590adb3aba39b1fd8290a17aa753fa351953487a9
check_source CVE-2026-95691.patch 2c9c869d0427b08c1480307583c1803971fb6223e5876dd20f61fd64102f095373fb32f5cc7bbbd2bf62aca82c229d740a95c39d5556ec72fd41f371e4a311b6
check_source CVE-2026-95692.patch f23fa46c18fca5e475286f4a5fe57834ccca8603a34f54ab0032af16d296f82eee6a9614f70aec4632fc9024f6af9ded9f2698693f02b3ade889c9d58d2cbe59
check_source CVE-2026-107496.patch 0bbd1f64e7bc62c8bbba9c85131a51cbfb6f7715156457d575df1e557d649340b8074195860f83be18fe3ed6852111f10053702d72658012f9c3a4c3fd504cea
check_source CVE-2026-107493.patch 5d1e0eda23be8661b3e75771f835ea7359dbbcb6ca2341c863577a256668758d90f03b11c50f50c8d3c9c4f282c12f825474d2b4d8c0444f47fcc5f630d1a3e2
check_source CVE-2026-124709.patch cbbf250b02a6799d47a9ffff81b2e3ed8524f298748bf8619893b3f963aa42ceb7a39e787574d0d39dc974037caf7bf2d1c50930251bcb324ec7094fc07e67ee
test -f "$inputs/SOURCE-MANIFEST.json"
grep -F '"openssl_alpine_release": "3.5.9-r2"' "$inputs/SOURCE-MANIFEST.json" >/dev/null
grep -F '"alpine_aports_revision": "c1a97dc220098b43d75d03ed5776a948908abfaf"' \
  "$inputs/SOURCE-MANIFEST.json" >/dev/null

mkdir -p "$out/work" "$out/logs" "$out/replacements"
cp "$inputs/SOURCE-MANIFEST.json" "$out/SOURCE-MANIFEST.json"
(
  printf 'target=%s\n' "$(cc -dumpmachine)"
  printf 'alpine_release=%s\n' "$(cat /etc/alpine-release)"
  printf 'build_jobs=%s\n' "$build_jobs"
  cc --version | head -n 1
  apk info -v
) > "$out/BUILD-TOOLCHAIN.txt"
tar -xzf "$inputs/nettle-3.10.2.tar.gz" -C "$out/work"
tar -xjf "$inputs/libgcrypt-1.10.3.tar.bz2" -C "$out/work"
tar -xzf "$inputs/openssl-3.5.9.tar.gz" -C "$out/work"

export CFLAGS='-O2 -mcpu=cortex-a9 -mfpu=vfpv3-d16 -mfloat-abi=hard -mno-unaligned-access'
export CXXFLAGS="$CFLAGS"
export CPPFLAGS=''
export LDFLAGS='-Wl,-z,relro,-z,now'
# libgcrypt's old config.guess sees gnueabihf under QEMU even on Alpine musl.
# Fix its Autoconf triplets to the compiler target, without cross mode.
toolchain_triplet=$(cc -dumpmachine)

# Nettle's Alpine APKBUILD ordinarily enables the fat/NEON runtime dispatch.
# Turn off both the dispatch and assembler, preserving public SONAMEs.
(
  cd "$out/work/nettle-3.10.2"
  ./configure --prefix=/usr --libdir=/usr/lib --enable-shared \
    --disable-fat --disable-assembler --disable-arm-neon \
    --disable-openssl > "$out/logs/nettle-configure.log" 2>&1
  make -j"$build_jobs" > "$out/logs/nettle-build.log" 2>&1
  make DESTDIR="$out/stage-nettle" install > "$out/logs/nettle-install.log" 2>&1
)

# Disable ARMv8/NEON assembler paths, including hardware-dispatch code.
(
  cd "$out/work/libgcrypt-1.10.3"
  patch -p1 -i "$inputs/gcc14-armv7.patch" > "$out/logs/gcrypt-patch.log"
  ./configure --build="$toolchain_triplet" --host="$toolchain_triplet" \
    --prefix=/usr --libdir=/usr/lib --disable-asm \
    --disable-neon-support --disable-arm-crypto-support \
    > "$out/logs/gcrypt-configure.log" 2>&1
  make -j"$build_jobs" > "$out/logs/gcrypt-build.log" 2>&1
  make DESTDIR="$out/stage-gcrypt" install > "$out/logs/gcrypt-install.log" 2>&1
)

# Match Alpine's ARM OpenSSL profile plus source-applied security patches.
(
  cd "$out/work/openssl-3.5.9"
  for name in auxv.patch CVE-2026-88822.patch CVE-2026-88823.patch \
      CVE-2026-95691.patch CVE-2026-95692.patch CVE-2026-107496.patch \
      CVE-2026-107493.patch CVE-2026-124709.patch; do
    patch -p1 -i "$inputs/$name" >> "$out/logs/openssl-patches.log"
  done
  perl ./Configure linux-armv4 --prefix=/usr --libdir=lib \
    --openssldir=/etc/ssl enable-ktls shared no-asm no-zlib no-async \
    no-comp no-idea no-mdc2 no-rc5 no-ec2m no-ssl3 no-seed \
    no-weak-ssl-ciphers $CFLAGS > "$out/logs/openssl-configure.log" 2>&1
  make -j"$build_jobs" > "$out/logs/openssl-build.log" 2>&1
  make DESTDIR="$out/stage-openssl" install_sw \
    > "$out/logs/openssl-install.log" 2>&1
)

copy_soname() {
  test -f "$out/$1/usr/lib/$2"
  cp -L "$out/$1/usr/lib/$2" "$out/replacements/$2"
}
copy_soname stage-nettle libnettle.so.8
copy_soname stage-nettle libhogweed.so.6
copy_soname stage-gcrypt libgcrypt.so.20
copy_soname stage-openssl libcrypto.so.3
copy_soname stage-openssl libssl.so.3
if [ -d "$out/stage-openssl/usr/lib/ossl-modules" ]; then
  mkdir -p "$out/replacements/ossl-modules"
  cp -L "$out/stage-openssl/usr/lib/ossl-modules/"*.so \
    "$out/replacements/ossl-modules/"
fi
test -f "$out/replacements/ossl-modules/legacy.so"
for object in "$out/replacements/"*.so.* "$out/replacements/ossl-modules/"*.so; do
  test -f "$object"
  attributes=$(readelf -A "$object")
  if ! printf '%s\n' "$attributes" | grep -F 'Tag_FP_arch: VFPv3-D16' >/dev/null; then
    echo "missing VFPv3-D16 ARM attribute: $object" >&2
    exit 1
  fi
  if ! printf '%s\n' "$attributes" | grep -F 'Tag_ABI_VFP_args: VFP registers' >/dev/null; then
    echo "missing ARM hardfloat ABI attribute: $object" >&2
    exit 1
  fi
  if printf '%s\n' "$attributes" | grep -Eq 'NEON|Advanced_SIMD'; then
    echo "incompatible ARM ISA: $object" >&2
    exit 1
  fi
done
(cd "$out/replacements" && find . -type f -exec sha256sum '{}' + | sort) \
  > "$out/REPLACEMENTS-SHA256SUMS.txt"
manifest_sha=$(sha256sum "$out/SOURCE-MANIFEST.json" | cut -d' ' -f1)
printf 'qualification=UNQUALIFIED\nnormal_deployment=FORBIDDEN\nopenssl_alpine_release=3.5.9-r2\nsource_manifest_sha256=%s\n' \
  "$manifest_sha" > "$out/CANDIDATE-ONLY.txt"
echo 'D16_CRYPTO_REPLACEMENTS=BUILT release_qualification=UNQUALIFIED'
