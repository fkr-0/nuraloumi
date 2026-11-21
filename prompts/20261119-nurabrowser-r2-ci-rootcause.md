# Diagnose and repair NuraBrowser current-source CI

## Goal
Establish a reproducible green CI qualification for the current committed NuraLoumi/NuraBrowser source, fix demonstrable CI-script/workflow failures without masking failing tests, and produce an exact blocking-gate report.

## Context
The last visible GitHub Actions main CI, Alpine musl ARMv7, and Debian armhf runs succeeded on 9753fae/0.0.7-dev.1; no official CI run exists for the current 0.0.8-rc.1 development HEAD. Older NuraBrowser local gate failures included rust-lld SIGBUS, disk quota, and ARM closure pass=false; disk has now been recovered (observed 52GB free). Do not claim GitHub CI is failing without identifying an actual failed run.

## Scope
In: .github/workflows/ci.yml and the SL101 musl/debian workflows; narrowly relevant CI orchestration scripts/tests; docs/qualification/NURA-BROWSER-R2-CI-ROOTCAUSE.md. Own CI configuration and harness scripts, not runtime browser/engine/UI, ARM gate2 audit scripts, or release packaging.

## Required
- Inspect read-only GitHub Actions run history and exact SHA/branch; differentiate no-run from failed run, and CI from target qualification.
- Reproduce clean committed HEAD (not dirty working tree) using isolated temporary snapshot and non-destructive test/build commands: cargo fmt --all -- --check, cargo test --workspace --locked, cargo clippy --workspace --all-targets --locked -- -D warnings, relevant Python/Meson browser test suites. Record first reproducible failure with complete commands and log tail.
- Fix only directly evidenced CI configuration/harness defects. No skip/allow-failure blanket suppression, no relaxed ARM flags or ABI checks.
- Report whether local parity is green and exact remote push/CI authorization needed to test new SHA.

## Acceptance
A first-failure/root-cause matrix, exact command/exit code, relevant regression tests, no regressions to CI-only artifact provenance, and checkpoint with paths/hashes. Common requirements: Read AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md and relevant previous gate checkpoint. Check workspace status, project presence and path claims. Preserve unrelated and uncommitted work; acquire claims before writing; do not edit another active lane's files. No Git push/fetch, remote CI trigger, publish, device SSH, reboot, live Sway mutation or global process kill. Checkpoint exact canonical OCP ID at milestones and terminal; include narrow self-contained follow-ups. Record actual tests with exit codes and PASS/FAIL/PENDING, do not equate host proof with SL101 deployment. Latest observed main was ahead 52 commits, 52GB free, one unrelated player_engine.rs local modification.
