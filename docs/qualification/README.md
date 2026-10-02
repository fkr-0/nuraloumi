# Qualification tooling

This directory separates three evidence classes that must not be conflated.

1. Host proof
   - nuraloumi-xtask builds and tests on the development host.
   - Renderer fixture PNG and menu fixture/headless smoke are independent.
   - scripts/smoke-host.sh may start an isolated Weston headless compositor with
     the pixman renderer. It replaces XDG_RUNTIME_DIR for that child only and
     never switches, stops, or reconfigures the user's live desktop session.

2. Cross-build proof
   - Intended Nura target: armv7-unknown-linux-musleabihf.
   - Rationale: the current Nura SL101 userspace is musl on ARMv7 hard-float.
   - Tegra20 has no NEON, so .cargo/config.toml explicitly disables the neon
     target feature and deliberately does not set target-cpu.
   - cargo xtask qualify-armv7 reports Rust target, linker, optional compiler
     check, and cross-release ELF state as PASS, PENDING, or FAIL.
   - scripts/inspect-armv7-elf.sh checks ARM ELF attributes and disassembly
     heuristically. It is useful evidence, not a proof that every path is safe.

3. Real SL101 proof
   - Only a real tablet run can establish no SIGILL, touch transform, keyboard,
     RSS/CPU, first-frame latency, suspend/resume, repeated menu lifecycle, and
     compositor recovery.
   - Host/cross results must never promote this class to PASS.

Useful commands:

    cargo xtask --help
    cargo xtask check --xtask-only
    cargo xtask size
    cargo xtask deps
    cargo xtask qualify-host
    cargo xtask qualify-armv7
    cargo xtask qualify-armv7 --cross-check
    scripts/smoke-host.sh

When runtime lanes have landed, set exact non-mutating smoke commands if their
final CLI differs from the automatic probe:

    NURALOUMI_FIXTURE_SMOKE='exact renderer fixture command' \
    NURALOUMI_MENU_SMOKE='exact menu fixture/headless command' \
    cargo xtask qualify-host --strict

See SL101-DEPLOYMENT.md for copy-only target deployment and
EVIDENCE-TEMPLATE.md for target recording.
