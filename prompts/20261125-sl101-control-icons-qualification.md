# Verify font-independent Control Center category icons

## Goal
Qualify Media, Network, Display, System and Notifications icons so the SL101 does not show missing-glyph rectangles.

## Context
Commit 95bb56bc introduced Cairo stroke icons for control.tab.* and control.media.*, control.network.*, control.display.*, control.system.*, and control.notifications.* in crates/nuraloumi-render-cairo/src/layout.rs. The installed release still contains private-use Nerd Font glyphs and the user sees rectangles; the new code is not deployed or visually qualified.

## Scope
Own only Control Center icon geometry/visibility and deterministic pixel tests. No menu placement, scrolling or media-engine changes. Avoid adding font, image, EGL or GPU runtime dependencies to Cairo. Device screenshots only read-only and only with existing authenticated access.

## Acceptance
1. Render all five categories at tablet 448px layout and at a narrower viewport; verify nonempty visible strokes, geometry within 18x18 icon bounds and absence of private glyph text.
2. Check paint order and contrast for dark/light palettes and selected/disabled state, 1x/2x output scale.
3. Run cargo test -p nuraloumi-render-cairo --lib and relevant shell integration tests. If visual defect remains, provide isolated correction with tests.
4. Report host source evidence separately from any physically observed tablet result.

## Coordination and safety
Read AGENTS.md and the chatgpt-fleet + ws-bridge worker contracts. Use authenticated projmgrauth workspace tools. Pin and report the actual starting Git commit; expected baseline includes UI fix 95bb56bc20033cb9db6b2d1570af18598d86dcfe. Inspect active path claims and preserve all pre-existing edits. If another owner has a matching claim, stop that mutation and checkpoint the conflict. Do not create persistent .ws-bridge/worktrees or preservation branches; any temporary managed worktree must be closed before completion. Never push, publish, tag, install system packages, switch /opt/nuraloumi/current, restart Sway, or reboot the SL101. Do not edit WPE/browser engine, native VDE/Grate/GStreamer, video_engine.rs or player_engine.rs: the parent owns them. Only report proven outcomes, with commands, exit codes and exact paths.

## Worker receipt
Use agent_checkpoint with the exact injected OCP task id and project root at meaningful milestones, then a terminal complete/blocked/needs_retrigger checkpoint. Include evidence, any commit SHA, test counts, and self-contained follow-up tasks. Do not dispatch child tasks or alter OCP state without new explicit scope.