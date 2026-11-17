# Press animation independent release qualification — 2026-11-17

Canonical OCP: `NURALOUMI-20261117-QOL-R1-18-PRESSED-INTERACTION-QA-20261117-C2172A`.
Exclusive owner: this document and `scripts/test_qol_pressed_feedback.py`; runtime source is read-only.

## Source-grounded baseline (read at audit start)

- `core/src/motion.rs`: Fast 120 ms; finite normalized progress and reduced motion exist for popup transitions; generic press transition is not yet present.
- `render-cairo/src/model.rs` and `layout.rs`: menu `pressed_id` affects accent fill, but no release interpolation is demonstrated.
- `shell/src/bin/panel.rs`: `panel_pointer_press` and `panel_touch_press` only track activation; pointer/touch down handlers do not immediately repaint; `build_panel_scene` receives no ephemeral pressed id. Touch cancellation and synthetic pointer filtering already exist.
- Existing panel tests establish 48px hit width on 800, 1024, 1280 (not yet 854 or scale >1), touch/pointer suppression, drag cancellation and multi-touch behavior.

## Executable qualification

```sh
python3 -m unittest discover -s scripts -p test_qol_pressed_feedback.py -v
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Source-based tests are intended **red** for missing generic motion and panel down/release visual wiring. Existing protections should remain green. They do not substitute for event-injection tests or actual Cairo pixel and hit-region equivalence checks.

## Independent gate matrix

| Case | Acceptance / evidence | Current proof |
|---|---|---|
| Pointer down/up, move-out | visual down immediately; matching release activates once; leave clears | down repaint missing |
| Touch down/move/up/cancel | visual held state only for one pointer; cancel never activates | prior cancel logic exists; visual gate missing |
| Synthetic click / rapid double taps | at most one action per valid sequence | existing synthetic filter unit test; repeat stress not demonstrated |
| Keyboard Enter/Space | identical pressed and release semantics | visual parity not demonstrated |
| Focus lost / surface destruction / config refresh | held state clears and no delayed activation | separate runtime event test needed |
| Fast release at 0, 60, 120, 121 ms | exact finite interpolation (full); no idle frames after 120 | generic primitive missing |
| Reduced motion | instant visual settle; no scheduled animation frame | press primitive missing |
| Noninteractive status / disabled | never transient pressed highlight | verify semantic paint gating |
| 800×480, 854×480, 1280×800, scale 2 | >=48 logical px for primary hits; scene rect/hit rect stable | existing 800/1280 host test only |
| CPU fallback & ARMv7/no NEON | Cairo/wl_shm unchanged; native target tests separately | no SL101 session touched |

## Release decision

**BLOCKED / NO TAG** until R1-16 and R1-17 merge with deterministic behavioral tests and R1-15 receives green qualification. A successful host build is not physical SL101 validation. No production session was altered.
