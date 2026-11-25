# Independently audit RC2 source-matched package CI and safe deployment gate

## Goal
Reconcile local 0.0.8-rc.2 source, CI outputs, GPU private video bundle provenance and installed release status without altering the device, enabling an honest next deployment qualification decision.

## Context
Exact main pre-UI source was 623609cbe4413af3313682b9478bb505a883510d; new UI source is 95bb56bc. The 28 ARMv7/musl executable build passed ISA checks and was installed as a new /opt release before reboot. Its private lib/config/share assets were copied byte-identical from RC1, but this is not an official complete-video artifact and the installed player has never been qualified. WPE musl headless candidate crashed, remains staged/unqualified. Authenticated SSH to user@SL101 was rejected by OpenSSH after reboot even though TCP is reachable.

## Scope
Own read-only GitHub/CI provenance, local release packaging manifest, source/ELF/hash policy, and network/SSH diagnosis using existing config. Do not change SSH keys, authentication policies, /opt/current, playerd, kernel, GPU drivers or any system service. No production mutation or reboot. Keep code and browser/player source out of scope.

## Acceptance
1. List exact current branch/commit/CI check state and prove which official artifacts are binary versus text-only; never substitute a local tarball for an official producer.
2. Cross-check GPU bundle manifest with actual library chain, ABI, exported SONAMEs, loader and provenance requirements; identify why installed package is not complete.
3. Reproduce bounded SSH auth evidence (no passwords/secret disclosure), distinguish network reachability from successful login and propose a safe recovery step.
4. Deliver docs/qualification/SL101-RC2-DEPLOY-GATE-20261125.md with clear GO/NO-GO, rollback path and required independent video/browser tests.

## Coordination and safety
Read AGENTS.md and the chatgpt-fleet + ws-bridge worker contracts. Use authenticated projmgrauth workspace tools. Pin and report the actual starting Git commit; expected baseline includes UI fix 95bb56bc20033cb9db6b2d1570af18598d86dcfe. Inspect active path claims and preserve all pre-existing edits. If another owner has a matching claim, stop that mutation and checkpoint the conflict. Do not create persistent .ws-bridge/worktrees or preservation branches; any temporary managed worktree must be closed before completion. Never push, publish, tag, install system packages, switch /opt/nuraloumi/current, restart Sway, or reboot the SL101. Do not edit WPE/browser engine, native VDE/Grate/GStreamer, video_engine.rs or player_engine.rs: the parent owns them. Only report proven outcomes, with commands, exit codes and exact paths.

## Worker receipt
Use agent_checkpoint with the exact injected OCP task id and project root at meaningful milestones, then a terminal complete/blocked/needs_retrigger checkpoint. Include evidence, any commit SHA, test counts, and self-contained follow-up tasks. Do not dispatch child tasks or alter OCP state without new explicit scope.