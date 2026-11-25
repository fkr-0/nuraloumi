# SL101 ARMv7 build on GitHub-hosted Arm runners (2026-11-25)

## Architecture fact

GitHub offers the standard `ubuntu-24.04-arm` hosted runner, but its
documented architecture is **Arm64 (AArch64)**, not 32-bit ARMv7. For a
public repository GitHub documents 4 vCPUs, 16 GiB RAM and 14 GiB storage.
It is therefore **not** automatically a native ARMv7 compiler runner.
Source: https://docs.github.com/en/actions/reference/runners/github-hosted-runners

The SL101 requires armv7-unknown-linux-musleabihf, Cortex-A9,
VFPv3-D16, hard float and **no NEON**. Merely observing `apk --print-arch`
inside a container does not prove native execution; QEMU can emulate the
entire ARMv7 userspace.

## Bounded capability experiment

The existing WPE candidate workflow has a manual boolean input
`probe_only`. When true, it **skips the expensive WPE job** and runs a
15-minute maximum on `ubuntu-24.04-arm`, without configuring QEMU:

1. Pull an official Alpine 3.22 ARMv7 container.
2. Start a real ARMv7 userspace process, capture its Docker init PID and
   inspect **the host kernel's** /proc/PID/exe ELF header with readelf.
   Require `ELF32` and `Machine: ARM`, not an AArch64 QEMU binary.
3. If and only if that works, install Alpine's native `build-base` and
   compile/execute an ARMv7 hello-world ELF with Tegra2 ISA flags.
4. Inspect produced ELF headers and attributes, rejecting NEON and
   requiring VFPv3-D16.
5. Upload bounded evidence under an explicitly `UNQUALIFIED` label.

This is capability discovery, not WebKit, sandbox, media, package, runtime
or SL101 qualification. It does not touch the physical device or publish
browser binaries.

## Decision gate

| Observation | Next build approach |
| --- | --- |
| Host /proc/PID/exe is ELF32 ARM, compiler and sample execute | Consider a short WPE CMake/build sample on this Arm64 runner; only then move the main WPE job off QEMU |
| Docker fails with exec format error | Arm64 kernel lacks required AArch32 userspace compatibility; use x86-64 or Arm64 **cross-compilation** |
| Host /proc/PID/exe is ELF64 AArch64 | QEMU or another interpreter is executing the guest; do **not** claim a native ARMv7 build |
| Package or VFPv3-D16 gate fails | Preserve evidence, fix target toolchain; do not run a long WPE build |

Native AArch32 on this hosted runner is **UNKNOWN until the short GitHub
run completes**. Even a passing compatibility result is not a throughput
benchmark. The existing QEMU checkpoint scheme remains a fallback, not the
desired release build architecture.

## Running and interpreting

After the source revision is intentionally published, dispatch the
`NuraBrowser WPE ARMv7 musl candidate` workflow with `probe_only=true`.
Do **not** supply resume inputs. Inspect the
`hosted-arm64-native-armv7-probe-...` artifact, especially
`RESULT.txt`, `HOST-PROCESS-ELF.txt`, `TARGET-ELF.txt` and
`TARGET-ATTRIBUTES.txt`. A passing script reports
`HOSTED_ARM64_NATIVE_ARMV7=PASS`; otherwise treat the claim as falsified.

Do not switch the full producer to `ubuntu-24.04-arm` simply because
GitHub lists Arm runners. The actual host ELF witness determines whether
this is faster native compilation or just QEMU again.
