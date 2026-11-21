# Qualify NuraBrowser nested compositor pixel/touch E2E

## Goal
Close Gate 3C: real WPE browser screenshot pixel geometry, focus, touch and latency/RSS measurements in a dedicated isolated nested Wayland compositor.

## Context
Prior R1-03C-3A44F8 delivered a harness that exits 77 when sway/labwc, grim or wtype missing; Python syntax and shellcheck PASS, but no real pixel evidence. Its scoped harness claim may remain until ~2026-11-19T01:25:46Z; never bypass it.

## Scope
In: scripts/qualify-browser-nested-wayland.sh, scripts/qualify-browser-nested-wayland.py, docs/qualification/NURA-BROWSER-R2-NESTED-E2E.md and isolated fixture files. No live compositor/session mutation, no package manager installation without authorization, no browser Rust UI modifications.

## Acceptance
Verify actual tool availability after disk recovery, execute isolated nested compositor if supported; capture grounded scene/pixel and event traces, touch/keyboard parity, first frame and RSS budget. If host lacks prerequisites, produce exact missing commands/evidence and reproducible safe command; preserve honest exit 77. Common requirements: Read AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md and relevant previous gate checkpoint. Check workspace status, project presence and path claims. Preserve unrelated and uncommitted work; acquire claims before writing; do not edit another active lane's files. No Git push/fetch, remote CI trigger, publish, device SSH, reboot, live Sway mutation or global process kill. Checkpoint exact canonical OCP ID at milestones and terminal; include narrow self-contained follow-ups. Record actual tests with exit codes and PASS/FAIL/PENDING, do not equate host proof with SL101 deployment. Latest observed main was ahead 52 commits, 52GB free, one unrelated player_engine.rs local modification.
