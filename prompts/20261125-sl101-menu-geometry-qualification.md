# Independently qualify SL101 launcher placement and eliminate remaining gap

## Goal
Verify that the launcher actually opens immediately under the Sway top panel and remains entirely within the 1280x800 display. The owner photographed a roughly panel-height blank gap and bottom clipping.

## Context
Source commit 95bb56bc changed menu overlay margins to zero in crates/nuraloumi-shell/src/bin/panel.rs and menu.rs; that is a source hypothesis, not physical qualification. Sway top layer has exclusive zone 48 logical px, compositor GLES2 active. User reported bad placement on the installed pre-fix release.

## Scope
Own only menu/panel placement geometry, its regression fixtures and focused tests. No wheel/touch or glyph changes; no live desktop mutation. Read-only on SL101 if SSH is already authenticated, otherwise mark optical verification blocked. Amend code only if independently proven necessary, acquire exact claims, and keep edits isolated from other workers.

## Acceptance
1. Derive Sway/wlroots layer-shell layout geometry and account once for panel exclusive zone, output scale and top/left/bottom panel edges; avoid guessing from old screenshots.
2. Run cargo test -p nuraloumi-shell --bin nuraloumi-panel and --bin nuraloumi-menu plus targeted geometry tests, reporting exit codes.
3. Verify expected menu bounds at 1280x800 and at a reduced fixture viewport, and identify if the zero-margin fix still produces overlap/clipping.
4. Deliver a concise evidence report or corrected isolated commit; do not promote to production.

## Coordination and safety
Read AGENTS.md and the chatgpt-fleet + ws-bridge worker contracts. Use authenticated projmgrauth workspace tools. Pin and report the actual starting Git commit; expected baseline includes UI fix 95bb56bc20033cb9db6b2d1570af18598d86dcfe. Inspect active path claims and preserve all pre-existing edits. If another owner has a matching claim, stop that mutation and checkpoint the conflict. Do not create persistent .ws-bridge/worktrees or preservation branches; any temporary managed worktree must be closed before completion. Never push, publish, tag, install system packages, switch /opt/nuraloumi/current, restart Sway, or reboot the SL101. Do not edit WPE/browser engine, native VDE/Grate/GStreamer, video_engine.rs or player_engine.rs: the parent owns them. Only report proven outcomes, with commands, exit codes and exact paths.

## Worker receipt
Use agent_checkpoint with the exact injected OCP task id and project root at meaningful milestones, then a terminal complete/blocked/needs_retrigger checkpoint. Include evidence, any commit SHA, test counts, and self-contained follow-up tasks. Do not dispatch child tasks or alter OCP state without new explicit scope.