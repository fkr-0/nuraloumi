# Detect the SL101 slider open or closed and notify only on real changes

## Goal
Implement a bounded hardware-state observer for the ASUS SL101 sliding keyboard/display mechanism and emit one NuraLoumi notification when it opens or closes.

## Context
The owner explicitly requested this as a distinct sixth worker. The SL101 runs ARMv7 postmarketOS/musl, Linux 7.0.1-postmarketos-grate-dmavde1 with Sway. NuraLoumi has notification IPC/daemon surfaces but the actual slider switch source and polarity have not been established. Do not assume SW_LID, SW_TABLET_MODE, GPIO or an accelerometer without evidence.

## Scope
Own source discovery in nuraloumi-providers and platform adapter, an isolated slider state detector and its tests, plus narrowly scoped notification integration or docs. If exact hardware evidence requires the sibling sl101_linux repository, open one read-only workspace there and obey its instructions. Do not alter display/KMS, power, media/browser, compositor state or kernel GPIO configuration. No production autostart/deployment until the physical signal and transition polarity are independently verified.

## Acceptance
1. Inventory actual input/sysfs/udev candidate signals with read-only commands; record kernel capability and state/polarity uncertainty explicitly.
2. Implement a state machine with initial-state snapshot (no false boot notification), debounce, duplicate suppression, hotplug/suspend-resume handling, bounded polling/event loop and graceful missing-signal behavior.
3. Emit exact 'Slider opened' and 'Slider closed' notifications through the supported NuraLoumi notification transport; never fire repeatedly on unchanged input. Include fixture tests for both transitions, bounce, reconnect and unavailable sensor.
4. Run relevant host tests and, if the signal is verified on real SL101 and notifications are safe, perform a narrowly scoped read-only or opt-in canary. Return a local commit/report and exact physical acceptance gap.

## Coordination and safety
Read AGENTS.md and the chatgpt-fleet + ws-bridge worker contracts. Use authenticated projmgrauth workspace tools. Pin and report the actual starting Git commit; expected baseline includes UI fix 95bb56bc20033cb9db6b2d1570af18598d86dcfe. Inspect active path claims and preserve all pre-existing edits. If another owner has a matching claim, stop that mutation and checkpoint the conflict. Do not create persistent .ws-bridge/worktrees or preservation branches; any temporary managed worktree must be closed before completion. Never push, publish, tag, install system packages, switch /opt/nuraloumi/current, restart Sway, or reboot the SL101. Do not edit WPE/browser engine, native VDE/Grate/GStreamer, video_engine.rs or player_engine.rs: the parent owns them. Only report proven outcomes, with commands, exit codes and exact paths.

## Worker receipt
Use agent_checkpoint with the exact injected OCP task id and project root at meaningful milestones, then a terminal complete/blocked/needs_retrigger checkpoint. Include evidence, any commit SHA, test counts, and self-contained follow-up tasks. Do not dispatch child tasks or alter OCP state without new explicit scope.