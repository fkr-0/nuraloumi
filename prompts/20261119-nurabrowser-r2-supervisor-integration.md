# Finish NuraBrowser supervised lifecycle and multitab resume

## Goal
Close Gate 4 by integrating an isolated crash-safe WPE engine supervisor, startup readiness, bounded restart, and multi-tab restore with observable failure states.

## Context
Prior NURABROWSER-R1-04-0D6E63: seven fake-engine tests and Meson 7/7 PASS, shell and workspace Clippy PASS; live browser.rs wiring and full multi-tab resume not proven. Gate4 claim may remain until ~2026-11-19T01:09:28Z.

## Scope
In: crates/nuraloumi-shell/src/browser/supervisor.rs and transport.rs, dedicated browser_supervisor_lifecycle.rs tests, components/nura-browser-engine/src/control/** only if claim allows, docs/qualification/NURA-BROWSER-R2-SUPERVISOR.md. Avoid bin/browser.rs/mod.rs while Tabstrip worker owns them. Deliver a precise wiring patch proposal/API contract for Tabstrip worker, or coordinate handoff before any overlap.

## Acceptance
Deterministic process start/ready, crash/retry, bounded stop and multi-tab persistence tests; no orphan helpers or fake success; tests and strict Clippy pass; real WPE/sway integration either proven or marked PENDING with exact seam. Common requirements: Read AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md and relevant previous gate checkpoint. Check workspace status, project presence and path claims. Preserve unrelated and uncommitted work; acquire claims before writing; do not edit another active lane's files. No Git push/fetch, remote CI trigger, publish, device SSH, reboot, live Sway mutation or global process kill. Checkpoint exact canonical OCP ID at milestones and terminal; include narrow self-contained follow-ups. Record actual tests with exit codes and PASS/FAIL/PENDING, do not equate host proof with SL101 deployment. Latest observed main was ahead 52 commits, 52GB free, one unrelated player_engine.rs local modification.
