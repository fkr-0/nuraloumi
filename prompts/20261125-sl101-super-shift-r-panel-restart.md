# Diagnose and repair Super+Shift+R panel restart on the SL101

## Goal
Make the user’s Super+Shift+R shortcut reliably restart the NuraLoumi panel on SL101. The observed bug is that the hotkey terminates the existing panel, but no replacement panel appears.

## Context
The SL101 uses Sway/OpenRC and a source-matched NuraLoumi ARMv7/musl release in /opt/nuraloumi/current. The shortcut may be defined in the sibling sl101_linux Sway configuration or deployment scripts rather than in the NuraLoumi Rust repository. Do not assume a key binding or root cause without inspecting the actual configured command and startup environment. The existing panel owns IPC socket and child toast process.

## Scope
Own only panel restart lifecycle, Sway binding and launching shell/service glue, plus deterministic restart tests and qualification documentation. First locate the real config and compositor binding, process supervision, env vars including WAYLAND_DISPLAY, XDG_RUNTIME_DIR, PATH, release symlink, launcher binary and log destinations. If needed, open the sibling sl101_linux repository read-only to discover ownership; obtain a separate path claim and obey its AGENTS instructions before changing it. Do not touch menu geometry, scrolling, icon layout, notification banner implementation, WPE, media or kernel.

## Acceptance
1. Reproduce or prove the failure path using the configured key binding and bounded host fixtures. Distinguish a bad key binding, missing binary path, process-group termination, environment loss, compositor layer-shell failure, IPC socket collision and panel crash.
2. Implement a minimally scoped, idempotent restart path with verified child readiness, a bounded timeout, observable error/log path, and safe recovery when the new panel fails. Do not orphan multiple panels or kill unrelated session processes. Preserve the intended Super+Shift+R semantics.
3. Add targeted tests for successful replacement, failed replacement, repeated rapid presses, old process not running and Sway session environment preservation. Run relevant shell/Rust tests and verify diff checks.
4. Save docs/qualification/SL101-PANEL-RESTART-20261125.md with exact reproduction, proposed config/install path and on-tablet acceptance steps. If SSH remains key-rejected, mark physical verification BLOCKED; source/test success is not on-device PASS.
5. Commit only scoped changes. Never remotely invoke the live hotkey, kill panel, change Sway config, restart services or reboot without explicit owner-approved canary and rollback.

## Coordination and safety
Read AGENTS.md and the chatgpt-fleet + ws-bridge worker contracts. Use authenticated projmgrauth workspace tools. Pin and report the actual starting Git commit; baseline includes UI fix 95bb56bc20033cb9db6b2d1570af18598d86dcfe. Inspect active path claims and preserve existing edits. If another owner holds a matching claim, stop that mutation and checkpoint the conflict. No persistent .ws-bridge/worktrees or preservation branches; close any temporary managed worktree. Never push, publish, tag, install packages, switch /opt/nuraloumi/current or restart Sway. Parent owns WPE browser and native VDE/GPU player. State evidence, commands, exit codes and residual gaps.

## Worker receipt
Use agent_checkpoint with exact injected OCP task id and project root at meaningful milestones, then a terminal complete/blocked/needs_retrigger checkpoint. Include exact evidence, commit SHA, test counts and clear next steps. Do not dispatch child tasks or alter OCP state without new scope.
