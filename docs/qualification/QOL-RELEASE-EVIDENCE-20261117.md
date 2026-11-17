# QoL release integration evidence — 2026-11-17

Canonical task: NURALOUMI-20261117-QOL-R1-15-RELEASE-INTEGRATION-20261117-7FB90E

## Gate decision: BLOCKED / no tag

Read AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md, docs/QOL-RELEASE-20261117.md and the operator/fleet/worker contracts. Current main HEAD observed: `27f7e0b`; most recent 0.0.7 tag: `0.0.7-dev.1`.

OCP lanes 02–15 were running at the time of inspection; lane 01 `NURALOUMI-20261117-QOL-R1-01-WIDGET-SEMANTICS-20261117-3A4458` recorded terminal `blocked_retriable` from unverified browser delivery; concurrent source changes exist in core/renderer. No lane completion receipts/checkpoint refs yet for this campaign. Do not conflate working-tree modifications with accepted implementation.

Current tree has active claims for core/renderer, notifications, providers, files, player, QA and packaging. Includes preserved unrelated dirty example/fixture manifests and existing untracked prompts. Integrator modified no worker-owned code.

## Host checks (snapshot; other agents changing files concurrently)

| Command | Status | Evidence |
| --- | --- | --- |
| `cargo fmt --all -- --check` | FAIL exit 1 | formatting drift in `crates/nuraloumi-shell/src/bin/files.rs` and `player_engine.rs`, among others |
| `cargo test --workspace` | FAIL exit 101 | E0063 `crates/nuraloumi-shell/src/bin/player.rs:196`: `Playlist` initializer lacks `loop_mode` |
| `cargo clippy --workspace --all-targets -- -D warnings` | FAIL exit 101 | `crates/nuraloumi-render-cairo/src/layout.rs:1065,1082`: manual implementation of `div_ceil` |

These are current integration breakages, **not independently established preexisting baseline failures**. Pending worker writes can change all three outcomes. Rerun after handoffs/settled HEAD. Avoid fixing inside active worker claims; assign player initializer seam to lanes 08/09, renderer lint to lane 01 and format drift to respective file owners.

## Required release contract graph

core semantics and stable ids -> renderer geometry/hit regions -> Wayland pointer/touch/keyboard events -> shell menu actions and regular windows -> provider snapshots and explicit acknowledgements -> package binaries/services and artifacts.

Specific missing validated seams:
- Slider, tab, scroll-thumb and pointer dismissal E2E with bounds and touch parity.
- Notifications DBus ownership, desktop activation and notify-send E2E.
- Panel mute refresh and audio/backlight state provenance.
- Files binary/path behavior, compact/full cross-launch and navigation.
- Player M3U security and seek/loop/pause contract through compact/expanded UI.
- Packaging real ELF/ARMv7 no-NEON, musl+glibc dependency closure and installation manifest.
- SL101 live input, display, audio, notification, browser and Grate video remain untested for this integration cycle.

Preservation: no alterations to 0.0.7-dev.1 Grate three-pass playback or WPE browser isolation; no evidence yet proving full regression behavior.

## Delegated independent verification prompts (prepared, no dispatch receipt)

- `prompts/20261117-qol-r1-15-child-a-contract-graph.md`
- `prompts/20261117-qol-r1-15-child-b-player-security.md`
- `prompts/20261117-qol-r1-15-child-c-packaging-abi.md`

Dispatch is **not established**: current authenticated ws-bridge OCP exposed catalog/list/create/transition but no task-dispatch action, and these prompts are not yet proven catalogued. Do not bypass hard authority by claiming task delivery. Queue only after canonical operator admission/dispatch becomes available, checking duplicates and capacity.

## Release decision

No CHANGELOG version bump, Cargo version change, integration commit or local tag until lanes resolve source ownership; all host checks pass on a stable commit; reviewed child evidence and actual package artifacts exist; real device tests are either verified or explicitly classified as unfulfilled release gates. Reconcile OCP work against checkpoints, not timestamps or dispatch receipts.
