# Close NuraBrowser WPE sandbox runtime gate

## Goal
Finish R1 Gate 1's real WebProcess isolation/bus-proxy evidence with fail-closed behavior.

## Context
Prior NURABROWSER-R1-01-2CAD40: Meson 7/7, Python sandbox fixtures 5/5 PASS, but historical private-glibc bus-proxy failure and actual sandboxed WebProcess were UNPROVEN. The local disk-pressure failure is reportedly fixed.

## Scope
In: components/nura-browser-engine/src/engine/**, sandbox-specific test fixtures, docs/qualification/NURA-BROWSER-R2-SANDBOX-RUNTIME.md. Out: crates/nuraloumi-shell browser UI, packaging/ARM scripts, live desktop.

## Acceptance
Reproduce original bus-proxy behavior against the installed engine where available; ensure startup refuses unsafe unsandboxed fallback; prove process isolation via actual runtime observation or report exact unavailable host prerequisite; Meson and sandbox tests PASS; preserve IPC/control contract. Common requirements: Read AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md and relevant previous gate checkpoint. Check workspace status, project presence and path claims. Preserve unrelated and uncommitted work; acquire claims before writing; do not edit another active lane's files. No Git push/fetch, remote CI trigger, publish, device SSH, reboot, live Sway mutation or global process kill. Checkpoint exact canonical OCP ID at milestones and terminal; include narrow self-contained follow-ups. Record actual tests with exit codes and PASS/FAIL/PENDING, do not equate host proof with SL101 deployment. Latest observed main was ahead 52 commits, 52GB free, one unrelated player_engine.rs local modification.
