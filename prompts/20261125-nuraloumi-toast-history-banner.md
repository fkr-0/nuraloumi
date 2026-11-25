# Restore floating notification banners while keeping history

## Goal
Fix NuraLoumi so a new notification is visible as a floating on-screen banner when delivered, while it also appears in notification history. The user currently sees the history entry but no floating banner.

## Context
The panel owns IPC in crates/nuraloumi-shell/src/bin/panel.rs and starts IpcRuntime with sibling nuraloumi-toast. crates/nuraloumi-shell/src/surface_catalog.rs handles IpcCommand::Notify, probes toast executable, and calls notification_reply which may suppress launch depending on observed surfaces. crates/nuraloumi-shell/src/bin/toast.rs is a one-shot Wayland surface. A separate worker owns notification transport CLI/daemon usage docs; do not overwrite their work.

## Scope
Own toast launch gating, lifecycle and rendering/position/visibility diagnostics plus targeted tests. Read IPC response codes and notification-history flow; distinguish accepted notification, history insert, a spawned process, a Wayland-surface commit, and a physically visible toast. Check the Sway layer-shell anchors/exclusive zone, display size 1024x600, panel interaction, output/seat, environment and run-directory inheritance. The notification center must retain history and never steal/disable the floating banner by being merely registered but hidden. Preserve intended DND, explicit suppression and rate limits.

## Acceptance
1. Reproduce with a deterministic host fixture: incoming notification persists in history while toast is absent; determine if suppression occurs before spawn, during child start, or during Wayland display.
2. Correct the precise cause using a bounded solution; verify toast spawn, visibility and dismissal with tests for center closed/open, overlapping arrivals, missing executable, transient Wayland failure and DND state. Avoid unbounded child queues and blocking the panel.
3. Provide logs/diagnostic status that discriminate history success from banner rendering without leaking private notification content. Confirm keyboard/touch dismiss behavior does not ghost-activate the underlying menu.
4. Run focused tests; save docs/qualification/SL101-NOTIFICATION-BANNER-20261125.md showing a safe local canary and explaining the unverified physical SL101 gate if device access is blocked.
5. Make a scoped commit only. Do not deploy or visually mark PASS without observed banner on hardware.

## Coordination and safety
Read AGENTS.md and the chatgpt-fleet + ws-bridge worker contracts. Use authenticated projmgrauth workspace tools. Pin and report the actual starting Git commit; baseline includes UI fix 95bb56bc20033cb9db6b2d1570af18598d86dcfe. Inspect active path claims and preserve pre-existing edits. If another owner has a matching claim, stop that mutation and checkpoint the conflict. Do not create persistent .ws-bridge/worktrees or preservation branches; close any temporary managed worktree before completion. Never push, publish, tag, install system packages, switch /opt/nuraloumi/current, restart Sway or reboot the SL101. Do not edit WPE/browser engine, native VDE/Grate/GStreamer, video_engine.rs or player_engine.rs; the parent owns those. Distinguish host-simulated tests from physically observed SL101 results. Report exact evidence, commands, exit codes, and paths.

## Worker receipt
Use agent_checkpoint with the exact injected OCP task id and project root at meaningful milestones, then a terminal complete/blocked/needs_retrigger checkpoint. Include evidence, any commit SHA, test counts, and self-contained follow-up tasks. Do not dispatch child tasks or alter OCP state without new explicit scope.
