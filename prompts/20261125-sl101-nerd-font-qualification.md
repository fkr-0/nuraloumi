# Qualify a bounded Nerd Font for the ARMv7 musl SL101 desktop

## Goal
Select, legally package, and qualify a specific Nerd Font glyph set for SL101 and NuraLoumi, to eliminate rectangle glyphs in surfaces that still use font glyphs.

## Context
The user observes rectangle icons in the Control Center and top bar on the 1024x600 SL101. NuraLoumi has crates/nuraloumi-shell/src/glyphs.rs and a packaged-font renderer in crates/nuraloumi-render-cairo/src/packaged_font.rs; the renderer intentionally avoids fontconfig fallback. An independent worker owns font-free Cairo Control Center category icons. Do not replace or conflict with that fix; instead establish a reliable optional Nerd Font for remaining glyph consumers and define the exact fallback.

## Scope
Own font choice, upstream license/attribution verification, glyph-coverage and memory/storage measurements, packaging manifest and deterministic host tests/documentation. Discover the current source-matched musl release packaging and its available font-loader path before coding. Select a pinned version and small subset if technically/licensing feasible. Do not curl arbitrary font binaries, add runtime Internet access, or change renderer ownership without a bounded, test-proven reason.

## Acceptance
1. Inventory the exact code points used by glyphs.rs and other on-device UI paths; name missing characters, rendered boxes, and target loader/font-family lookup rules. Prove whether a Nerd Font file is opened directly or needs a bundled fontconfig registration on musl.
2. Choose a trusted, redistributable font family/version and license with a reproducible checksum/source reference; if unavailable, provide an offline package recipe rather than fabricated verification.
3. Add a deterministic test for glyph coverage and graceful fallback for missing glyphs, including panel and menu at 1024x600; demonstrate no rendering path relies solely on private-use glyph availability.
4. Verify ARMv7/musl packaging integration without mutating the live tablet; record size/RSS tradeoffs, expected font paths and exact physical rendering qualification gate.
5. Place findings in docs/qualification/SL101-NERD-FONT-20261125.md. Commit only your scoped changes.

## Coordination and safety
Read AGENTS.md and the chatgpt-fleet + ws-bridge worker contracts. Use authenticated projmgrauth workspace tools. Pin and report the actual starting Git commit; baseline includes UI fix 95bb56bc20033cb9db6b2d1570af18598d86dcfe. Inspect active path claims and preserve pre-existing edits. If another owner has a matching claim, stop that mutation and checkpoint the conflict. Do not create persistent .ws-bridge/worktrees or preservation branches; close any temporary managed worktree before completion. Never push, publish, tag, install system packages, switch /opt/nuraloumi/current, restart Sway or reboot the SL101. Do not edit WPE/browser engine, native VDE/Grate/GStreamer, video_engine.rs or player_engine.rs; the parent owns those. Distinguish host-simulated tests from physically observed SL101 results. Report exact evidence, commands, exit codes, and paths.

## Worker receipt
Use agent_checkpoint with the exact injected OCP task id and project root at meaningful milestones, then a terminal complete/blocked/needs_retrigger checkpoint. Include evidence, any commit SHA, test counts, and self-contained follow-up tasks. Do not dispatch child tasks or alter OCP state without new explicit scope.
