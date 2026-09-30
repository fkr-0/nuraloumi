@projmgrauth
Set repo: nuraloumi

# NuraLoumi Wave 1 — Operating Shell/Menu Binaries

Canonical OCP task: NURALOUMI-SHELL-R1-20260930
Lane: shell
Priority: 93

Use this exact canonical task. Adopt its packet before mutation if required. Never solve missing sibling APIs by editing sibling crates; keep compatibility/adapters inside your scope and report integration assumptions.

## Mission

Turn the frozen contracts into operating user-facing binaries immediately. The shell lane should produce a coherent fixture-driven panel/menu experience even while hardware providers and Wayland internals are landing in parallel.

Read:
- AGENTS.md
- DESIGN.md
- ROADMAP.md
- docs/ARCHITECTURE.md
- docs/CONTRACTS.md
- docs/AGENT-LANES.md

## Exclusive write scope

- crates/nuraloumi-shell/**
- examples/menu-fixtures/**

Do not edit core/renderer/wayland/providers/xtask/root files.

## Required binaries

1. nuraloumi-menu
   - generic menu runner.
   - fixture/demo model mode requiring no real system services.
   - load JSON/TOML menu data when practical.
   - keyboard navigation and typed action reporting.
   - headless/model-test mode so logic is testable without a compositor.
   - when current renderer/Wayland APIs are available, open and display a real software-rendered menu.

2. nuraloumi-panel
   - top strip matching DESIGN.md.
   - Apps / Network / Audio / Battery / Clock affordances.
   - fixture provider mode with deterministic values.
   - one transient menu at a time.
   - panel never needs keyboard focus until a menu is intentionally interactive.

3. Initial menu families
   - launcher/search sheet.
   - network summary sheet.
   - audio/volume sheet.
   - system sheet with brightness and suspend/restart/power confirmation rows.
   - visible unavailable/stale/error states rather than disappearing sections.

4. Interaction state
   - PlatformEvent -> semantic core navigation.
   - hit-region activation -> same core action path as Enter.
   - press/release semantics for touch.
   - Escape/back hierarchy.
   - selected semantic identity preserved across provider refresh where possible.
   - reduced-motion flag.

5. Config
   - small serde config for panel edge/height/menu width/theme/reduced-motion.
   - sane defaults for the 1280x800-era SL101 class.
   - strict errors for invalid geometry rather than panics.

## Parallel-lane strategy

Core/render/Wayland/providers may not yet expose their final APIs. First build pure shell state + fixture adapters against docs/CONTRACTS.md. Then adapt only inside this crate to APIs that are already landed when you run. Do not wait idly for other agents; produce binaries/headless demos that are useful independently.

## Acceptance

- cargo test -p nuraloumi-shell passes with state-machine and fixture tests.
- cargo clippy -p nuraloumi-shell --all-targets -- -D warnings passes.
- both nuraloumi-menu and nuraloumi-panel are real binaries and --help works.
- fixture/headless mode works without NetworkManager/audio/Wayland.
- destructive system actions cannot execute without confirmation semantics.
- touch and keyboard activation converge on one action dispatch path.
- no system shell command is constructed directly in rendering/input code.

## Handoff

Provide exact commands for fixture/headless and live attempts, note which sibling APIs were available, list any adapter slated for integration cleanup, and submit typed phase result/checkpoint.
