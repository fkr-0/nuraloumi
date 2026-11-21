# Repair committed Rust source failures blocking NuraBrowser CI

## Evidence
OCP CI root-cause 6888ED isolated clean commit 387c5e6 and proved cargo fmt exit 1, cargo test exit 101, strict Clippy exit 101. Committed crates/nuraloumi-shell/src/player_engine.rs:793 has a <<<<<<< HEAD conflict marker; the live working tree currently removes that single line already, so preserve it and identify the owner. Committed notifications.rs also needs formatting and a clippy::let_and_return correction at line 97. Do not just suppress warnings or alter Cargo CI gates.

## Scope
Read AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, owner/claims. In: crates/nuraloumi-shell/src/player_engine.rs and src/bin/notifications.rs after coordination, with focused tests and docs/qualification/NURA-BROWSER-R2-CI-RUST-CLOSE.md. Out: browser UI/engine, package/CI workflow, unrelated pending edits, network Git, remote CI, device.

## Acceptance
Resolve conflict *semantically*, not only strip marker; preserve current user/agent edits; repair notifications formatting and lint. Reproduce fmt/test/clippy on a clean committed or isolated worktree snapshot to show green; if unrelated new failures occur, record them. Claim exact paths before writes, leave unrelated changes intact, checkpoint the exact task, and hand off a reviewable commit or patch, not a blind merge.
