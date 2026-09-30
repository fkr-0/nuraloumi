@projmgrauth
Set repo: nuraloumi

# NuraLoumi Wave 1 — Semantic Core

Canonical OCP task: NURALOUMI-CORE-R1-20260930
Lane: core
Priority: 95

Treat the OCP task above as the durable work identity. Do not create a legacy next.md or substitute task. Before writes, use the project workflow authority, adopt/bind the exact task packet if required, claim only your assigned scope, and preserve all unrelated concurrent work.

## Mission

Build the renderer-neutral semantic foundation that every NuraLoumi binary can rely on. This is a production library for a Tegra20/ARMv7 tablet, not a mock architecture exercise. Produce a coherent, tested API that is small, deterministic, serializable, and independent of GUI/window-system/runtime frameworks.

Read first:
- AGENTS.md
- ROADMAP.md
- DESIGN.md
- docs/ARCHITECTURE.md
- docs/CONTRACTS.md
- docs/AGENT-LANES.md

Reference material is design input, not a copy target:
- /home/user/work/code/unilii/docs/menu-design-system.md
- /home/user/work/code/unilii/unilii/core/src/menu.rs
- /home/user/work/code/unilii/unilii/core/src/ui_tokens.rs
- /home/user/work/code/unilii/unilii/core/src/ui_motion.rs

## Exclusive write scope

- crates/nuraloumi-core/**

Read the rest of NuraLoumi freely. Do not edit Cargo.toml at repository root, bridge.yml, docs, or any sibling crate.

## Required implementation

1. Menu data model
   - MenuModel with stable menu ID/title/items.
   - MenuItem with stable item ID, label, optional subtitle, semantic kind, enabled/visible state, optional typed action, optional children.
   - Action/Submenu/Checkable/Section/Status/Separator semantics.
   - No renderer-specific fields.

2. Typed actions
   - activation/toggle/adjust/navigate/confirm/close/custom capabilities.
   - explicit destructive confirmation representation.
   - serialization that is stable enough for fixture files.
   - validation limits for IDs/text/depth/item count to prevent unbounded dynamic provider data.

3. Navigation state machine
   - deterministic selected identity, not fragile row-index state.
   - Up/Down over actionable rows only, wrap behavior.
   - Right/Left submenu semantics.
   - Back/Escape hierarchy behavior.
   - query/search text and backspace.
   - activation returns an explicit semantic outcome rather than executing anything.

4. Search and quick-select
   - Unicode-safe case-insensitive practical filtering.
   - stable result ordering.
   - deterministic quick-select labels for actionable visible rows.
   - headings/status/separators never receive quick-select labels.

5. Theme/design tokens
   - dark + light semantic palettes based on DESIGN.md.
   - spacing/type/icon/hit-target/border/elevation/opacity tokens.
   - minimum primary touch target >= 48 logical px.
   - contrast-oriented tests for primary/secondary text where feasible.

6. Motion primitives
   - 120/180/240 ms classes.
   - enter/exit/fade/reduced-motion.
   - pure time/progress functions with bounded output.
   - no timer/runtime dependency.

7. Ergonomics
   - constructors/builders for common row types.
   - clear error types.
   - serde fixtures/examples.
   - rustdoc on the public contract.

## Dependency ceiling

Allowed dependencies should remain small: serde/serde_json and perhaps unicode-normalization only if it demonstrably improves correctness. Do not add Iced, Tokio, Cairo, Wayland, X11, async executors, DBus, or system-service crates.

## Acceptance

- cargo test -p nuraloumi-core passes with meaningful navigation/model/validation/theme/motion tests.
- cargo clippy -p nuraloumi-core --all-targets -- -D warnings passes.
- crate has no GUI/window-system/runtime dependency.
- duplicate IDs, excessive depth/items/text and invalid structural combinations are rejected deterministically.
- selection survives filtering/submenu transitions by semantic identity.
- non-actionable rows cannot accidentally activate.
- fixture serialization round-trips.
- CRATE_READY placeholder is removed/replaced by real API.

## Handoff

Run the narrow checks, then attempt cargo test --workspace without editing other lanes if concurrent work makes the full workspace temporarily fail. Submit a typed phase result/checkpoint with exact changed paths, commands, evidence, API notes for downstream renderer/shell/provider agents, and any remaining integration risk.
