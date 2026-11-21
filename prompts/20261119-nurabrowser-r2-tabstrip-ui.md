# Finish NuraBrowser tab strip and Wayland UI integration

## Goal
Close Gate 3A: fixed minimum 48-logical-pixel tab strip, full-width address row, focus/keyboard/touch parity and reliable Wayland layout.

## Context
Prior NURABROWSER-R1-03-A01D14 Rust host tests PASS; R1-03A-727871 blocked by a predecessor claim on browser/mod.rs and bin/browser.rs until ~2026-11-19T01:09:31Z. Do not duplicate previous in-flight integration; inspect latest files first.

## Scope
In: crates/nuraloumi-shell/src/bin/browser.rs, crates/nuraloumi-shell/src/browser/mod.rs, narrowly named browser layout tests, docs/qualification/NURA-BROWSER-R2-TABSTRIP-UI.md. Do not edit supervisor.rs, transport.rs, engine, or browser nested compositor harness. Coordinate interface requirements with Gate4.

## Acceptance
Deterministic geometry tests for 48px hit regions, address row, keyboard/touch/pointer release/cancel, offline headless fixture and strict Clippy PASS; explicit real-compositor evidence as PASS or PENDING, no fabricated pixel proof. Common requirements: Read AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md and relevant previous gate checkpoint. Check workspace status, project presence and path claims. Preserve unrelated and uncommitted work; acquire claims before writing; do not edit another active lane's files. No Git push/fetch, remote CI trigger, publish, device SSH, reboot, live Sway mutation or global process kill. Checkpoint exact canonical OCP ID at milestones and terminal; include narrow self-contained follow-ups. Record actual tests with exit codes and PASS/FAIL/PENDING, do not equate host proof with SL101 deployment. Latest observed main was ahead 52 commits, 52GB free, one unrelated player_engine.rs local modification.
