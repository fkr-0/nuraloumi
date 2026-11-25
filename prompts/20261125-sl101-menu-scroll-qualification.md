# Qualify launcher wheel and touch swipe scrolling without ghost clicks

## Goal
Independently review and test the new scrolling path; close demonstrable gaps in mouse-wheel movement, touch dragging, scrollbar visibility, clipping and click suppression.

## Context
Commit 95bb56bc adds Wayland PointerAxis, MenuScroll touch state and manual Cairo scroll offsets in crates/nuraloumi-wayland and nuraloumi-shell. The tablet user reports its scrolling did not work. Earlier QoL contract docs/qualification/QOL-VISUAL-20261117.md also call out scrollbar track dragging and touch visibility.

## Scope
Own scroll-only tests and fixes in Wayland event routing, menu scroll state or scrollbar hit/drag interactions. Do not modify placement or icons. Prefer host fixture tests; no synthetic claim of physical touch pass. Never contact /opt or alter Sway.

## Acceptance
1. Test repeated positive/negative wheel axes, fractional trackpad deltas, upper/lower clamp, empty content, and fresh-provider redraw while scrolled.
2. Test touch tap versus 8px threshold, multi-touch cancellation, touch-up not activating a row after a swipe, and synthetic pointer suppression.
3. Check whether the visible scrollbar thumb can actually be dragged. If absent, either implement a bounded, testable gesture or record a precise residual blocker without claiming completion.
4. Run cargo test -p nuraloumi-wayland --lib, cargo test -p nuraloumi-shell --lib and the two shell menu binary tests. Provide exact results/commit or a bounded report.

## Coordination and safety
Read AGENTS.md and the chatgpt-fleet + ws-bridge worker contracts. Use authenticated projmgrauth workspace tools. Pin and report the actual starting Git commit; expected baseline includes UI fix 95bb56bc20033cb9db6b2d1570af18598d86dcfe. Inspect active path claims and preserve all pre-existing edits. If another owner has a matching claim, stop that mutation and checkpoint the conflict. Do not create persistent .ws-bridge/worktrees or preservation branches; any temporary managed worktree must be closed before completion. Never push, publish, tag, install system packages, switch /opt/nuraloumi/current, restart Sway, or reboot the SL101. Do not edit WPE/browser engine, native VDE/Grate/GStreamer, video_engine.rs or player_engine.rs: the parent owns them. Only report proven outcomes, with commands, exit codes and exact paths.

## Worker receipt
Use agent_checkpoint with the exact injected OCP task id and project root at meaningful milestones, then a terminal complete/blocked/needs_retrigger checkpoint. Include evidence, any commit SHA, test counts, and self-contained follow-up tasks. Do not dispatch child tasks or alter OCP state without new explicit scope.