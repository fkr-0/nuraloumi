# 0.2 package and performance qualification (host/cross/device)

## Bounded release package gate

The former four-binary package list silently missed later surface binaries.
`cargo xtask package` now enumerates the **binary targets** of
nuraloumi-shell, nuraloumi-wayland and nuraloumi-providers from Cargo metadata;
xtask and examples are deliberately not shipped. A missing declared binary is
a hard failure, not a PENDING/PASS for the remainder of the set.

On a stable tree (build first, never launch anything during the package step):

    cargo xtask build-release
    cargo xtask package --out target/review-package-<unique-id>
    cargo xtask package-check --dir target/review-package-<unique-id>

The check validates exact Cargo binary enumeration, executable bits, the
manifest's SHA-256 digests, no duplicate/path-traversal entries and no
unlisted staged files. An ARMv7 package additionally runs the heuristic ELF
audit over **each packaged binary**. For cross builds, use the exact SL101
musl target, sysroot and non-NEON flags documented in ARMV7.md:

    scripts/build-sl101-armv7.sh
    cargo xtask package --target armv7-unknown-linux-musleabihf --out target/review-armv7-<unique-id>
    cargo xtask package-check --dir target/review-armv7-<unique-id>

These checks validate package contents; they do **not** establish real SL101
compatibility, no-SIGILL, first-frame latency, compositor behavior, or a
release. The package manifest marks a dirty worktree; retain commit hash,
binary digests, exact rustc/toolchain, cross sysroot identity, and test logs.

## Read-only live resource sampling

Use the bounded standard-library sampler on a running process set. It never
launches, signals, suspends, or restarts a process:

    python3 scripts/measure-process.py --pid 1234 --pid 1235 --samples 10 --interval 1 > samples.ndjson

Every JSON-line sample contains monotonic time, each PID's start-time identity,
RSS KiB from /proc/stat, cumulative CPU ticks, interval CPU use relative to one
core, and aggregate process RSS. Limits: at most 16 distinct PIDs, 60 samples,
0.1–2s interval, 120s total. Exited/reused PIDs fail closed. Keep original
NDJSON and record the process command lines and actual device identity.

Performance acceptance on the real SL101:
- combined idle RSS <= 35 MiB, menu-open incremental RSS <= 12 MiB;
- first interactive frame <= 250 ms; steady CPU render budget <= 33 ms
  (16 ms aspirational), measured via explicit instrumented event stamps;
- no idle continuous redraw; check per-surface open/close life-cycle.

A /proc sampler **cannot** measure first frame, user-perceived latency,
Cairo-to-wl_shm frame budgets, or touch/keyboard correctness; collect those
from instrumented runtime and physical interaction. Host readings are
development diagnostics only, not substitute device performance evidence.

For fixture-level verification:

    python3 -m unittest discover -s scripts -p 'test_measure_process.py'
    python3 scripts/measure-process.py --help

## Rollback and gates

This qualification lane makes no device mutation. For a *future separately
authorized* manual device trial: retain previous versioned stage, copy a new
tarball into a different versioned directory, validate its MANIFEST and SHA-256,
stop only the **known PIDs of this trial**, and start the previous binary from
the previous stage. Do not kill unrelated NuraLoumi sessions via a global
name match; never overwrite the pixman/labwc fallback or register boot services.

The SL101 deployment playbook is in SL101-DEPLOYMENT.md. On failure, preserve
kernel/DTB, libc/Cairo, environment, error logs, SIGILL trace, and last known
working stage before attempting another trial. Never use suspend as an
unattended qualification step. No 0.1/0.2 release is approved by this document.
