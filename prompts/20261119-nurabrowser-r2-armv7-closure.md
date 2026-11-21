# Resolve NuraBrowser ARMv7 no-NEON ELF closure

## Goal
Turn the Gate 2 static ARMHF audit into an evidence-backed complete no-NEON/ABI closure, including dlopen/WebProcess module inventory.

## Context
Prior NURABROWSER-R1-02-126482: ARMHF cross-build PASS, QEMU no-NEON PASS, Python 31 tests PASS, 148 ELFs, 128 conservative instruction suspects, 0 missing; aggregate gate stayed false. Disk is now free (52GB), previous test blocked by quota. Existing Gate2 path claim may expire ~2026-11-19T01:09:24Z; never bypass a live claim.

## Scope
In: components/nura-browser-engine/scripts/{audit-runtime-closure.py,gate-armhf-closure.py}, packaging ARMHF mapping/ELF audit helpers, tests/isa and tests/test_runtime_closure.py, docs/qualification/NURA-BROWSER-R2-ARMV7-CLOSURE.md. Out: CI workflows and app chrome.

## Acceptance
Independently classify suspect Thumb/ARM mappings and reachable instructions, validate hard float/VFP D16 and no NEON including dynamic modules, rerun QEMU and audited tests, output exact PASS/FAIL without waiving security checks. If real unqualified code remains, document count/location and narrow fix. Common requirements: Read AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md and relevant previous gate checkpoint. Check workspace status, project presence and path claims. Preserve unrelated and uncommitted work; acquire claims before writing; do not edit another active lane's files. No Git push/fetch, remote CI trigger, publish, device SSH, reboot, live Sway mutation or global process kill. Checkpoint exact canonical OCP ID at milestones and terminal; include narrow self-contained follow-ups. Record actual tests with exit codes and PASS/FAIL/PENDING, do not equate host proof with SL101 deployment. Latest observed main was ahead 52 commits, 52GB free, one unrelated player_engine.rs local modification.
