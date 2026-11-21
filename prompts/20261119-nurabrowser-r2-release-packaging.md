# Complete NuraBrowser release package gate after ABI closure

## Goal
Close Gate 5's NuraBrowser package provenance and installable ABI artifacts without using a historical mixed-source browser binary.

## Context
Prior NURABROWSER-R1-05-90EC0E: Python 10/10, Meson 8/8, cargo shell/workspace and Clippy PASS; release held because Gate2 ARMHF closure false and real installable APK/DEB absent. Prior old 'browse' script remains legacy, not release artifact.

## Scope
In: scripts/qualify-nurabrowser-release.py and its tests, docs/qualification/NURA-BROWSER-R2-RELEASE-PACKAGING.md, packaging manifest and desktop contract review. Do not edit Gate2-owned ARM audit or CI-owned workflows; coordinate before touching shared package scripts. No real device install, push or remote CI.

## Acceptance
Verify source/binary provenance, native packaged browser, manifest/desktop/ABI closure, no unexpected glibc/musl crossmix, deterministic archive checks and rollback/launch contract; run focused Python/Meson/Cargo tests; report PENDING if Gate2 not green. Common requirements: Read AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md and relevant previous gate checkpoint. Check workspace status, project presence and path claims. Preserve unrelated and uncommitted work; acquire claims before writing; do not edit another active lane's files. No Git push/fetch, remote CI trigger, publish, device SSH, reboot, live Sway mutation or global process kill. Checkpoint exact canonical OCP ID at milestones and terminal; include narrow self-contained follow-ups. Record actual tests with exit codes and PASS/FAIL/PENDING, do not equate host proof with SL101 deployment. Latest observed main was ahead 52 commits, 52GB free, one unrelated player_engine.rs local modification.
