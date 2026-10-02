# ARMv7 / no-NEON qualification

## Target selection

The current SL101 Nura bring-up evidence shows a musl userspace. The Rust target
for Wave 1 is therefore armv7-unknown-linux-musleabihf, not a GNU/glibc target.

Tegra20 is ARMv7 with hard-float VFPv3-D16 support but no NEON. The repository
target configuration disables NEON, the upper d16-d31 VFP register bank, and ARM
hardware divide instructions. This is required by real-device evidence: disabling
NEON alone still allowed LLVM to emit `vmov.f64 d17`, which SIGILLed on Tegra20.
The committed linker wrapper uses host Clang/LLD, while libc/Cairo and the matching
musl/GCC startup ABI are derived from the tablet's package repository and runtime.

## Evidence levels

    RUST_TARGET=PASS
        rustup reports armv7-unknown-linux-musleabihf installed.

    CROSS_LINKER=PASS
        a known musl ARM linker command is present.

    CROSS_CHECK=PASS
        cargo check completed for the target. This proves compilation only.

    ELF_AUDIT=PASS
        produced cross-release binaries are ARM32 and the available ELF
        attributes/disassembly heuristics did not detect NEON.

    DEVICE_RUNTIME=PENDING
        remains pending until binaries run on the actual SL101 without SIGILL
        and the target evidence template is completed.

A cross compiler result must never be reported as device qualification.

## Reproducible SL101 cross build

The repository can derive a minimal link sysroot from the actual tablet without
installing a compiler or headers on it. The generated sysroot stays under
`target/sl101-cross` and is never committed. The preparation step uses
`apk fetch` only, extracting the tablet-matched `musl-dev`, `gcc`, and
`libgcc-static` packages into the generated sysroot alongside the live loader,
Cairo, Wayland and libgcc_s runtime libraries. Dynamic PIEs therefore use musl's
matching `Scrt1.o/crti.o/crtn.o` and GCC's `crtbeginS.o/crtendS.o/libgcc.a`.
Do not substitute Rust's bundled musl CRT: on the SL101 that mix corrupted DSO TLS
and crashed pixman/font paths after `__tls_get_addr`.

    rustup target add armv7-unknown-linux-musleabihf
    scripts/prepare-sl101-sysroot.sh root@192.168.23.106
    scripts/build-sl101-armv7.sh
    cargo xtask qualify-armv7 --cross-check
    scripts/smoke-sl101-armv7.sh root@192.168.23.106

`build-sl101-armv7.sh --prepare` combines the first two project-specific
steps. The linker wrapper uses Clang/LLD as an ARMv7 hard-float driver with
the generated tablet-matched musl/GCC CRT and runtime sysroot. Rust codegen
disables NEON, d32, hwdiv and hwdiv-arm; the linker forces VFPv3-D16 and
dynamically links the shell against the tablet's Cairo ABI. The probe remains eligible for a fully static Rust-musl build, but
the common build script deliberately uses the same dynamic musl policy for all
three runtime binaries so one target ABI is qualified.

The target build strips debug information only. ARM mapping symbols are retained
because fully stripping symbols removes the metadata LLVM needs to distinguish
ARM, Thumb and inline data; disassembling such a fully stripped image can
misdecode data as NEON instructions. Mapping symbols do not affect runtime RSS.

The device smoke is copy-only: binaries are copied to `/tmp`, hashes are
checked, `--help` is executed to catch loader/SIGILL failures, and the files
are removed. It does not alter services, packages, boot state or the desktop.

After a real cross release exists:

    scripts/inspect-armv7-elf.sh \
      target/armv7-unknown-linux-musleabihf/release/nuraloumi-panel \
      target/armv7-unknown-linux-musleabihf/release/nuraloumi-menu \
      target/armv7-unknown-linux-musleabihf/release/nuraloumi-probe

The inspection is intentionally heuristic. Final no-NEON acceptance is a real
tablet start/run test plus absence of SIGILL under representative interaction.
