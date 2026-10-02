# ARMv7 / no-NEON qualification

## Target selection

The current SL101 Nura bring-up evidence shows a musl userspace. The Rust target
for Wave 1 is therefore armv7-unknown-linux-musleabihf, not a GNU/glibc target.

Tegra20 is ARMv7 with hard-float support but no NEON. The repository target
configuration disables neon explicitly and leaves target-cpu unspecified. A
toolchain-specific linker is not committed because the actual cross toolchain
must be observed on each build host.

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

## Commands

    rustup target add armv7-unknown-linux-musleabihf
    cargo xtask qualify-armv7
    cargo xtask qualify-armv7 --cross-check

After a real cross release exists:

    scripts/inspect-armv7-elf.sh \
      target/armv7-unknown-linux-musleabihf/release/nuraloumi-panel \
      target/armv7-unknown-linux-musleabihf/release/nuraloumi-menu \
      target/armv7-unknown-linux-musleabihf/release/nuraloumi-probe

The inspection is intentionally heuristic. Final no-NEON acceptance is a real
tablet start/run test plus absence of SIGILL under representative interaction.
