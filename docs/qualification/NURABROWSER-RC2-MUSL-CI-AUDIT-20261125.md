# NuraBrowser 0.0.8-rc.2 ARMv7/musl CI audit — 2026-11-25

## Source/ref and reconciliation

- Official release worktree baseline: `415f7b8414aff1b26f44256dd8a3232a2e8a3100` (`release/0.0.8-rc.2`), already containing upstream `a9c75a449e87b2d5fa2f01e55abb430881611ed6` and the eight patch-equivalent browser commits from the separate local `main` lineage.
- The ninth main-only commit adds a historical operator prompt with private local-path context; do not merge or publish that prompt merely to close the ancestry graph.
- The preserved nine-file local working-tree edit set is content-identical to the release baseline for seven files; the release also contains an extra same-commit WPE-producer verification test (ten lines) which must be kept. Its Alpine builder missed the 33-line source-compatibility and gettext fixes from the local working tree. Those isolated build fixes are now applied without changing either dirty checkout.
- This packet is evidence for the pre-existing release source and a proposed follow-up builder change. A new commit is not verified by the earlier artifact until new official CI runs.

## Independently retrieved official CI evidence (pre-builder-fix SHA)

- GitHub Actions workflow: `SL101 Alpine armv7 musl package`, official run `44967344615`, event `push`, branch `release/0.0.8-rc.2`, exact source SHA `415f7b8414aff1b26f44256dd8a3232a2e8a3100`, conclusion `success`.
- Artifact: `nuraloumi-musl-armv7-build-audit-40-415f7b8414aff1b26f44256dd8a3232a2e8a3100`. Downloaded independently from GitHub Actions using the authenticated GitHub CLI.
- Downloaded members and locally recomputed SHA-256:
  - `BINARY-LIST.txt`: `7c3ac02ca48d5bcecb35cd1dbfcf4952c74a5f6814f3764891a3be1b1d26d6f1`.
  - `BUILD-LOG.txt`: `7ec4ec375244aca813d47fdad725e8dcc81e761cca81597228433b1e87195267`.
  - `ELF-AUDIT.txt`: `4536ebee16e4e60d0c59a5182c86ac30041d45c885389e23b6579519e65ce02e`.
- Independent file-to-file consistency: 28 unique binary names, 28 matching `PASS ... ELF32 ARM VFPv3-D16 hardfloat musl no-NEON-attributes` lines, and `SL101_CROSS_BUILD=PASS` in the build log.
- **Limitation:** this is a text-only toolchain-build audit. No ELF objects were uploaded for independent re-disassembly; no `nuraloumi-ci.tar`, `runtime.json` or native WPE runtime exists inside that artifact. It is **not** a deployable browser runtime/package and cannot satisfy `scripts/verify-ci-artifact.py`.

## WPE producer block

- Official `NuraBrowser WPE ARMv7 musl candidate` run `44967349248`, same pre-fix SHA, was cancelled after its six-hour build window, while `Build native Alpine 3.22 ARMv7 candidate in isolated container` was running. No candidate upload followed.
- The producer intentionally marks output `runtime_qualification=UNQUALIFIED` and does not generate a `runtime.json` with sandbox, instruction, dlopen, WebProcess and independent-review PASS marks. This is correct fail-closed behavior.
- The ARMv7/musl `workflow_dispatch` release package job requires an exact successful same-commit producer run ID, exact manifest SHA and validated runtime. The normal push produces only build audit when that input is absent.
- A previous private device canary ran the *glibc* WPE runtime with a private D16 Mesa stack. That does not demonstrate a musl ABI/ELF/dlopen closure nor authorize promotion to the regular musl package.

## Qualification state

| Gate | Result |
| --- | --- |
| Source/release revision identity for downloaded audit | PASS (pre-fix SHA only) |
| 28 Rust ARMv7/musl build audit entries | PASS (CI text evidence; binary re-disassembly unavailable) |
| Native ARMv7/musl WPE producer completion | BLOCKED (six-hour QEMU timeout) |
| Hash-pinned complete WPE runtime and sandbox/dlopen/ISA closure | BLOCKED (no artifact) |
| Exact-source ordinary browser package tar/APK | BLOCKED (requires qualified runtime) |
| Independent device pixels, WebProcess and touch for this musl runtime | BLOCKED (not deployed/tested) |
| Production deployment authorization gate | NO-GO until all above pass |

## Concrete next action

Use a bounded, reproducible cross-build or faster native ARMv7 builder to complete WPE 2.54.2 inside the CI window; capture a producer artifact that includes the full target runtime tree. Validate every ELF instruction and dependency, including WebProcess spawned modules and sandbox executables, and run actual source-matched musl sandbox/page tests. Only after independently reviewed PASS evidence may the source-pinned `runtime.json` and regular release job be generated. Keep the device's existing validated software-rendering fallback and rollback unchanged.

## Later same-day official CI re-verification: exact RC.2 integration baseline

The ordinary `SL101 Alpine armv7 musl package` workflow subsequently
completed successfully at exact SHA
`41a901b99cd142009f7e603c8440264d3201d6d5` on both refs:

| Ref | Official run | Independently downloaded artifact |
| --- | --- | --- |
| `main` | `45521908820` | `nuraloumi-musl-armv7-build-audit-45-41a901b99cd142009f7e603c8440264d3201d6d5` |
| `release/0.0.8-rc.2` | `45521908746` | `nuraloumi-musl-armv7-build-audit-44-41a901b99cd142009f7e603c8440264d3201d6d5` |

The three files downloaded from each run are **byte-identical** between
`main` and the release ref. Independent local SHA-256:

- `BINARY-LIST.txt` (516 bytes):
  `7c3ac02ca48d5bcecb35cd1dbfcf4952c74a5f6814f3764891a3be1b1d26d6f1`
- `BUILD-LOG.txt` (14,121 bytes):
  `7ec4ec375244aca813d47fdad725e8dcc81e761cca81597228433b1e87195267`
- `ELF-AUDIT.txt` (13,588 bytes):
  `4536ebee16e4e60d0c59a5182c86ac30041d45c885389e23b6579519e65ce02e`

The 28 distinct binary names match the 28 `PASS` ELF-audit rows exactly,
in order, and the log contains `SL101_CROSS_BUILD=PASS`. **This verifies the
official text audit for the exact source baseline**, not the existence or
independent disassembly of the binary bytes: neither download includes ELF
objects, a native WPE runtime, `runtime.json`, nor `nuraloumi-ci.tar`.
The separate WPE producer still has no qualified same-commit artifact.

The follow-up D16 crypto/auditor code is committed *locally* as
`393c7c5564e5b99f086dd4e5bd219bc4fe8916ed` on an isolated integration
worktree. It has **not** been published or built by these earlier official
runs, and must not inherit their PASS conclusions.
