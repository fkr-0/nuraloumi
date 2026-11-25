# Qualify NuraLoumi notifications end-to-end and provide usage instructions

## Goal
Produce verified commands for local and SSH-triggered NuraLoumi notifications, including CLI, daemon, freedesktop D-Bus and on-screen toast/notification-center behavior.

## Context
The user explicitly asked how to send notifications in 0.0.8-rc.2. Inspect crates/nuraloumi-shell/src/bin/ctl.rs, bin/notifications.rs, notificationd, toast, docs/qualification/notifications-* and the SL101 OpenRC desktop session. The desktop session may use an isolated XDG_RUNTIME_DIR and a private D-Bus socket; verify process-scoped paths before documenting them.

## Scope
Own notification usage docs, host tests and a bounded probe script if useful; do not modify slider state detection (separate worker), WPE, player or the live .106 desktop. Read-only SSH may probe if authenticated, but never send surprise owner-facing notifications without a local explicit canary opt-in.

## Acceptance
1. Demonstrate exact parser-supported nuraloumi-ctl notify flags and expected output, plus a notify-send equivalent if daemon ownership exists.
2. Distinguish IPC socket and D-Bus session bus, startup order, missing-daemon fallback, permissions and notification history behavior.
3. Run focused tests; create docs/qualification/SL101-NOTIFICATION-USAGE-20261125.md with copyable commands and a no-UI fallback explanation.
4. Mark real tablet toast and D-Bus delivery pass only if observed; otherwise state the single remaining device verification step.

## Coordination and safety
Read AGENTS.md and the chatgpt-fleet + ws-bridge worker contracts. Use authenticated projmgrauth workspace tools. Pin and report the actual starting Git commit; expected baseline includes UI fix 95bb56bc20033cb9db6b2d1570af18598d86dcfe. Inspect active path claims and preserve all pre-existing edits. If another owner has a matching claim, stop that mutation and checkpoint the conflict. Do not create persistent .ws-bridge/worktrees or preservation branches; any temporary managed worktree must be closed before completion. Never push, publish, tag, install system packages, switch /opt/nuraloumi/current, restart Sway, or reboot the SL101. Do not edit WPE/browser engine, native VDE/Grate/GStreamer, video_engine.rs or player_engine.rs: the parent owns them. Only report proven outcomes, with commands, exit codes and exact paths.

## Worker receipt
Use agent_checkpoint with the exact injected OCP task id and project root at meaningful milestones, then a terminal complete/blocked/needs_retrigger checkpoint. Include evidence, any commit SHA, test counts, and self-contained follow-up tasks. Do not dispatch child tasks or alter OCP state without new explicit scope.