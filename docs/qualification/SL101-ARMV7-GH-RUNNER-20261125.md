# SL101 ARMv7 build on GitHub-hosted Arm runners (2026-11-25)

## Architecture fact

GitHub offers the standard `ubuntu-24.04-arm` hosted runner, but its
documented architecture is **Arm64 (AArch64)**, not 32-bit ARMv7. For a
public repository GitHub documents 4 vCPUs, 16 GiB RAM and 14 GiB storage.
The Arm64 label alone is not proof of ARMv7 execution. An independent
on-runner test below established that this particular hosted kernel supports
native AArch32 compatibility.
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
| Host /proc/PID/exe is ELF32 ARM, compiler and sample execute | **Observed: PASS**. Move the WPE candidate job to this Arm64 runner with mandatory no-QEMU preflight and monitor its real build result |
| Docker fails with exec format error | Arm64 kernel lacks required AArch32 userspace compatibility; use x86-64 or Arm64 **cross-compilation** |
| Host /proc/PID/exe is ELF64 AArch64 | QEMU or another interpreter is executing the guest; do **not** claim a native ARMv7 build |
| Package or VFPv3-D16 gate fails | Preserve evidence, fix target toolchain; do not run a long WPE build |

## Observed result (2026-11-25)

**PASS**, run https://github.com/fkr-0/nuraloumi/actions/runs/45587548628
at commit `7d74ff31c581d426f320864040f3c8b2da177434`. The actual
artifact is https://github.com/fkr-0/nuraloumi/actions/runs/45587548628/artifacts/15008429094 .
The host kernel's live Alpine container process was `ELF32 ARM`,
Version5 EABI hard-float. Alpine identified `armv7`. A compiled and executed
Cortex-A9 sample had `Tag_FP_arch: VFPv3-D16`,
`Tag_ABI_VFP_args: VFP registers`, and no NEON attribute.
The runner image was `ubuntu-24.04-arm` (version `20261116.188.1`).
The independent downloaded files were `RESULT.txt`,
`HOST-PROCESS-ELF.txt`, `TARGET-COMPILER.txt`, `TARGET-ELF.txt`, and
`TARGET-ATTRIBUTES.txt`. The result states `qualification=UNQUALIFIED`:
it proves **execution capability only**. Runner throughput, disk capacity,
full WebKit linking, dependency closure, sandbox, pixels and SL101 device
qualification still require separate verification.

Consequently, the WPE candidate job now uses `ubuntu-24.04-arm`, does not
install or enable QEMU, and runs the same ARMv7 host-ELF preflight every time.
The 4h30 bounded checkpoint mechanism remains an emergency fallback for
native build failures, not a routine multi-run strategy.

## Native WPE compiler failure and architecture correction

The first full native WPE run
https://github.com/fkr-0/nuraloumi/actions/runs/45588143763
at `98ed4ebd87f829e691be59b1e497422307602ba5` compiled 6,642 of 8,544
Ninja steps before `BPlatform.h:435` rejected the missing allocator
selection. A verified 104 MB compressed checkpoint (SHA256
`299ed156a84004a56fbff36a858845448727426e8e3e23f45c92a5db17bb1fb1`)
is preserved under artifact 15011610639, but is **not** reusable across a
changed commit/script under our exact provenance contract.

The independently inspected checkpoint exposes the underlying cause:
`CMakeFiles/3.31.7/CMakeSystem.cmake` recorded
`CMAKE_SYSTEM_PROCESSOR=aarch64` for the *32-bit* Alpine container because
the Arm64 host kernel supplies uname. The source-matched WebKitCommon.cmake
therefore selected `WTF_CPU_ARM64`, leaving both `USE_MIMALLOC` and
`USE_SYSTEM_MALLOC` `OFF`. For actual ARM32, BPlatform.h cannot use libpas,
so the allocator selection was impossible.

WebKit 2.54.2 already supports `FORCE_32BIT` on an AArch64 host in
WebKitCommon.cmake. The builder now requests `FORCE_32BIT=ON`, explicitly
pins the upstream ARM Thumb-2 mimalloc backend, and checks the resulting
CMake cache for `armv7l`, mimalloc ON and system malloc OFF **before**
invoking Ninja. The formerly failing `WPEPlatformHeadless` object library
is the first build target, before compiling the rest of WebKit, so its
allocator configuration fails early if still wrong. This correction still
requires a real hosted-runner compile,
link, ISA and runtime qualification; none is inferred from static CMake logic.

## Running and interpreting

After the source revision is intentionally published, dispatch the
`NuraBrowser WPE ARMv7 musl candidate` workflow with `probe_only=true`.
Do **not** supply resume inputs. Inspect the
`hosted-arm64-native-armv7-probe-...` artifact, especially
`RESULT.txt`, `HOST-PROCESS-ELF.txt`, `TARGET-ELF.txt` and
`TARGET-ATTRIBUTES.txt`. A passing script reports
`HOSTED_ARM64_NATIVE_ARMV7=PASS`; otherwise treat the claim as falsified.

Do not infer native ARMv7 support on other Arm64 runners or future runner
images without repeating the host-ELF witness. A passing native-ISA probe
is not evidence that WPE built or can be deployed.
