@projmgrauth
Set repo: nuraloumi

# NuraLoumi Wave 1 — Build, Packaging and SL101 Qualification Tooling

Canonical OCP task: NURALOUMI-XTASK-R1-20260930
Lane: qualification
Priority: 82

Keep this exact durable task identity. Claim only the tooling/qualification lane.

## Mission

Create the tooling that makes NuraLoumi rapidly buildable, inspectable, packageable and safe to try on the Tegra20 SL101. Produce useful operating host artifacts now, while clearly separating host proof from target proof.

Read:
- AGENTS.md
- ROADMAP.md
- docs/ARCHITECTURE.md
- docs/SL101-QUALIFICATION.md
- docs/AGENT-LANES.md

Relevant SL101 evidence lives in /home/user/code/android-infra, especially docs/sl101-nura-bringup.md and the Nura qualification artifacts. Read only what is necessary; do not modify android-infra.

## Exclusive write scope

- crates/nuraloumi-xtask/**
- .cargo/**
- packaging/**
- scripts/**
- docs/qualification/**

Do not edit any runtime crate, root Cargo.toml, bridge.yml or product docs.

## Required implementation

1. nuraloumi-xtask
   Subcommands or equivalent:
   - check: fmt/check/test/clippy orchestrator with clear failure boundaries.
   - build-release: produce release binaries.
   - size: enumerate binary sizes and optionally section/readelf summary.
   - deps: cargo metadata/tree report highlighting prohibited heavy deps in runtime crates.
   - package: stage a deterministic directory/tarball layout from existing binaries.
   - qualify-host: run fixture binaries and smoke checks without live desktop mutation.
   - qualify-armv7: inspect target/toolchain availability and produce explicit PASS/PENDING evidence, never fake a cross build.

2. ARMv7/no-NEON
   - .cargo config/target documentation for the intended GNU/musl target only where justified by the actual Nura userspace.
   - do not add target-cpu flags that imply NEON.
   - script/tool to inspect ELF attributes/disassembly heuristically for VFP/NEON assumptions when cross binaries exist.
   - distinguish compiler target success from on-device runtime qualification.

3. Software-rendering smoke
   - host script that can run a nested compositor when available, but skips with a precise reason if not installed.
   - smoke should never switch the user's live compositor/session.
   - verify fixture PNG/menu headless modes independently.

4. Packaging
   - stage bin/, share/nuraloumi/, config example, license/readme.
   - manifest with sha256, target triple, git commit and build command.
   - no installation into /usr in Wave 1.

5. SL101 deployment/rollback plan
   - copy-only staging path.
   - start from a terminal/SSH with known labwc/pixman recovery.
   - process kill/disable rollback.
   - resource measurement commands.
   - evidence template for touch, keyboard, RSS, CPU, startup latency, suspend/resume and 100x menu open/close.

## Acceptance

- cargo test -p nuraloumi-xtask passes.
- cargo clippy -p nuraloumi-xtask --all-targets -- -D warnings passes.
- xtask --help and at least check/size/deps/qualify-host run on current host.
- tooling never claims ARMv7 PASS if the target/toolchain/binaries were absent.
- packaging output is deterministic enough to hash/compare.
- no script mutates the current desktop session.
- qualification docs distinguish host, cross-build and real SL101 evidence.

## Handoff

Report commands, produced manifests/artifacts, exact ARMv7 readiness state, and what the future integration agent should run after the runtime lanes land. Submit typed phase result/checkpoint.
