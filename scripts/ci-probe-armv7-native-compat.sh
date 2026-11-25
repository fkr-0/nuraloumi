#!/usr/bin/env bash
# Prove (or falsify) *native* AArch32 execution on a GitHub-hosted arm64 runner.
# No QEMU setup is performed. A host /proc/PID/exe ELF32 ARM witness is mandatory.
# Usage: bash scripts/ci-probe-armv7-native-compat.sh OUTPUT_DIRECTORY
set -euo pipefail

out=${1:?output directory required}
mkdir -p "$out"
printf 'qualification=UNQUALIFIED\nnative_armv7=UNKNOWN\n' > "$out/RESULT.txt"

test "$(uname -m)" = aarch64 || {
  echo 'This capability probe requires a native AArch64 GitHub runner' >&2
  exit 2
}
command -v docker >/dev/null
command -v readelf >/dev/null

# An ARMv7 container is not enough: QEMU can masquerade as an ARMv7 guest.
# Inspect the *host kernel's executable* for the live container init process.
docker pull --platform linux/arm/v7 alpine:3.22
cidfile=$(mktemp)
cleanup() {
  if test -s "$cidfile"; then
    docker rm --force "$(cat "$cidfile")" >/dev/null 2>&1 || true
  fi
  rm -f "$cidfile"
}
trap cleanup EXIT
cid=$(docker run --rm -d --cidfile "$cidfile" --platform linux/arm/v7 \
  alpine:3.22 /bin/sh -c 'sleep 600')
pid=$(docker inspect --format '{{.State.Pid}}' "$cid")
[[ "$pid" =~ ^[1-9][0-9]*$ ]]
sudo readelf -h "/proc/$pid/exe" | tee "$out/HOST-PROCESS-ELF.txt" >/dev/null
grep -Eq 'Class:[[:space:]]+ELF32' "$out/HOST-PROCESS-ELF.txt" || {
  echo 'Container is emulated or not an ELF32 native process' >&2
  exit 2
}
grep -Eq 'Machine:[[:space:]]+ARM([[:space:]]|$)' "$out/HOST-PROCESS-ELF.txt" || {
  echo 'Container init is not native AArch32' >&2
  exit 2
}

# Compile and execute a target ELF using *the actual Alpine armv7 compiler*
# on this runner, then independently inspect its ABI and no-NEON attributes.
docker exec "$cid" /bin/sh -ec '
  test "$(apk --print-arch)" = armv7
  apk add --no-cache build-base binutils
  printf "int main(void) { return 0; }\n" > /tmp/armv7-native-probe.c
  cc -O2 -mcpu=cortex-a9 -mfpu=vfpv3-d16 -mfloat-abi=hard \
    -mno-unaligned-access /tmp/armv7-native-probe.c -o /tmp/armv7-native-probe
  /tmp/armv7-native-probe
  readelf -A /tmp/armv7-native-probe
' | tee "$out/TARGET-COMPILER.txt"
docker cp "$cid:/tmp/armv7-native-probe" "$out/armv7-native-probe"
readelf -h "$out/armv7-native-probe" > "$out/TARGET-ELF.txt"
readelf -A "$out/armv7-native-probe" > "$out/TARGET-ATTRIBUTES.txt"
grep -Eq 'Class:[[:space:]]+ELF32' "$out/TARGET-ELF.txt"
grep -Eq 'Machine:[[:space:]]+ARM([[:space:]]|$)' "$out/TARGET-ELF.txt"
grep -Fq 'hard-float ABI' "$out/TARGET-ELF.txt"
grep -Fq 'Tag_ABI_VFP_args: VFP registers' "$out/TARGET-ATTRIBUTES.txt"
grep -Fq 'Tag_FP_arch: VFPv3-D16' "$out/TARGET-ATTRIBUTES.txt"
if grep -Eiq 'Advanced_SIMD|NEON' "$out/TARGET-ATTRIBUTES.txt"; then
  echo 'ARMv7 native probe emitted NEON attributes' >&2
  exit 2
fi
printf 'qualification=UNQUALIFIED\nnative_armv7=PASS\nrunner_arch=aarch64\ncontainer_arch=armv7\n' > "$out/RESULT.txt"
echo 'HOSTED_ARM64_NATIVE_ARMV7=PASS (still not a WPE or device qualification)'
