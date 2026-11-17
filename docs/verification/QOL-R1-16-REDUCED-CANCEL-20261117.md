# R1-16 Child C — independent reduced-motion and cancellation audit

- Canonical OCP: `NURALOUMI-20261117-QOL-R1-16-CHILD-C-REDUCED-CANCEL-20261117-A9E15A`
- Parent: `NURALOUMI-20261117-QOL-R1-16-GENERIC-PRESSED-MOTION-20261117-41B424`
- Unique child: `NURALOUMI-R1-16-CHILD-C-REDUCED-CANCEL-20261117`
- Scope: **this report only**. Core, renderer, Wayland, shell, fixtures and existing R1-01 claims were read-only. No deployment, SSH, ABI, CI, push or installation.
- Snapshot: `git rev-parse HEAD` -> `c9d63c256ba11c6af2bada4a0021907b154dd059` (read at audit start). Concurrent changes are possible.

## Source-grounded audit

1. **Pure press transition — PASS within unit-test scope.** `crates/nuraloumi-core/src/motion.rs:28-90` distinguishes Idle/Held/Released/Cancelled, clamps finite progress and returns no animation for cancellation and reduced motion. `:173-219` tests 0/60/120 ms, nonfinite input and reduced treatment. It is intentionally paint-only and cannot ensure real shell event cancellation.
2. **Panel visual state — partial integration PASS.** `crates/nuraloumi-shell/src/bin/panel.rs:63-110` holds a target, releases over finite 120 ms, clears immediately on cancellation, and disables fading for reduced motion; `:1917-1931` invalidates/presents only on dirty/fading. `:857-870` schedules transition frames while animating; once `sample()` clears a completed fade, no press-related idle loop should remain. Caveat: animation path calls `roundtrip()` after a 16 ms sleep, so compositor timing/CPU load are not proven on SL101. `:3790-3813` is only a clock-primitive test, not an input-to-paint E2E.
3. **Panel pointer cancellation / wrong-button defect — ACTIONABLE.** `panel.rs:1060-1093` matches `PointerButton { x,y,pressed,.. }` on the panel without requiring `button == 0x110`; a secondary/middle button press-release on an active hit can dispatch `activate_panel_target`. Other input suppression at `:1017-1034` is scoped to primary button, so it does not fix this. `:1148-1162` does cancel on pointer drag outside/leave, but independent focus/seat-loss cancellation is not shown in the panel match and needs a deterministic fixture.
4. **Touch cancellation, repeated press, contact ownership — GAP.** `panel.rs:1094-1147` tracks touch IDs and clears on TouchCancel, but `:1120-1139` performs global visual `release()` before verifying a matching contact, and `:1141-1147` globally cancels feedback for *any* cancelled IDs. In concurrent touches, an unrelated up/cancel can clear the held visual of another contact; delivery for unrelated touch IDs must not activate a control. `TouchMotion` invalidates off-hit contact (`:1109-1119`) and a later up then does not activate; missing explicit tests include second contact, wrong ID, repeated down, drag-out and re-entry, and cancel-only-matching-contact.
5. **Keyboard parity — panel partially PASS; general menu gap.** `panel.rs:1163-1221` handles Tab focus and both Enter/Space down/up activation with a stored identity, ignores auto-repeat while armed and cancels on Escape. The general shell `crates/nuraloumi-shell/src/lib.rs:3407-3420` activates on **pressed Enter only**, and `PlatformKey` has no Space variant in this path; `crates/nuraloumi-shell/src/bin/menu.rs:657-661` forwards pressed keybindings but does not model visual held/release. Thus Space parity and generic menu press tint cannot be claimed across surfaces. Preserve text-input Space ownership when adding a fixture.
6. **Generic menu event cancellation — partial PASS, visual GAP.** `crates/nuraloumi-shell/src/lib.rs:3366-3405` validates same-region down/up and ignores mismatched releases, but no explicit pointer leave or focus-loss event clears `pointer_pressed_region` in this reducer. `crates/nuraloumi-shell/src/bin/menu.rs:577-655` forwards pointer buttons (without explicit primary-button check), stores `touch_regions`, and maps TouchCancel into TouchUp(region=None), preventing dispatch. `:628-640` changes tracked touch regions without redraw; renderer `crates/nuraloumi-render-cairo/src/layout.rs:402-407` knows `pressed_id` but `crates/nuraloumi-shell/src/lib.rs:385` constructs `pressed_id: None`. No end-to-end menu pressed visual can be asserted from these call sites.
7. **Disabled/status-only and immutable geometry — partially guarded.** `panel.rs:1165-1176` filters disabled/nonactionable keyboard targets and clock/battery. `:1065-1069` explicitly removes battery/clock only from visual feedback; pointer activation still passes any hit to `activate_panel_target`, so tests must prove this dispatcher fails closed for status/disabled hits. `nuraloumi-render-cairo/src/layout.rs:402-407` calculates `enabled` but the pressed tint checks only `pressed_id`; a future caller must not pass disabled IDs. Visual paint-only claims are not substitutes for a geometry assertion.
8. **Wayland lifecycle boundary — unqualified.** `crates/nuraloumi-wayland/src/backend.rs:1674,1704` emits touch cancellation; `crates/nuraloumi-wayland/src/types.rs:209-230` declares PointerLeave and TouchCancel. There is no demonstrated end-to-end focus/seat-unplug -> held-state reset for these panel/menu consumers. Host tests cannot establish compositor focus semantics.

## Deterministic fixture handoff (do not modify sources in this child)

- **R1-17 / pointer and cancel seam:** table-driven panel event sequences: BTN_LEFT, BTN_RIGHT, BTN_MIDDLE down/up; primary down -> move outside -> release; leave -> up; contact 1 held -> contact 2 up/cancel; second down of same ID; touch cancel -> synthetic pointer-up; status/disabled hits. Assert **zero action** on invalid sequences, idempotent visual cancel, correct target ID, no ghost click, and stable 48px hit rects. Source: `panel.rs:1017-1162`.
- **R1-18 / keyboard and shell reducer:** Enter/Space keydown/repeat/keyup for panel and menu, focus-switch during held key, search-field whitespace, pointer press -> leave/focus-loss -> release, disabled target. Assert one action per valid pair, no activation after cancellation, never on keyboard repeat. Source: `panel.rs:1163-1221`, `lib.rs:3366-3420`, `menu.rs:577-661`.
- **R1-18 / timing and paint:** fake monotonic clock 0/60/120/121 ms and reduced-motion release at 0ms; assert no scheduled frames after settle or cancel, no animation under reduced motion, Cairo pixels/press tint at 800x480, 854x480, 1280x800 and scale >1, logical regions unchanged and nonactionable items unpainted. Source: `motion.rs:54-90`, `panel.rs:63-110,857-870,1917-1931`, `layout.rs:402-425`.

## Commands executed and observed exit codes

| Command | Exit | Outcome |
| --- | ---: | --- |
| `git rev-parse HEAD; git status --short; nl -ba crates/nuraloumi-core/src/motion.rs ...; nl -ba crates/nuraloumi-shell/src/bin/panel.rs ...; nl -ba crates/nuraloumi-shell/src/bin/menu.rs ...` | 0 | Source inspection; unrelated dirty/untracked paths preserved |
| `nl -ba crates/nuraloumi-shell/src/bin/panel.rs ...; rg -n ...` | 0 | Event and redraw seam inspection |
| `cargo test -p nuraloumi-core motion::tests --lib` | **0** | 7 passed, 29 filtered |
| `cargo test -p nuraloumi-shell --bin nuraloumi-panel panel_press_hold_release_cancel_and_reduced_motion_are_finite` | **0** | 1 passed, 19 filtered |

No hardware run, broad workspace test, clippy or compositor E2E qualification was performed. These observations are review findings, **not** a shipping acceptance.

## Integration status

**Conditional / not signed off.** Pure transition logic passes narrowly. R1-17 must fix or explicitly guard wrong-button activation, mismatched multitouch visual release/cancel and focus-loss; R1-18 must independently verify event-to-frame and generic menu keyboard/pressed integration. Distinct follow-up OCP prompts should own (a) pointer primary-button + status boundary, (b) multitouch cancel and pointer focus-loss, (c) keyboard Space and finite repaint, each read-only evidence first and product changes only after owner coordination. No existing R1-01 renderer claim was touched.

## OCP follow-up dispatch and fail-closed evidence

- **C1** `NURALOUMI-R1-16-C1-PRIMARY-STATUS-20261117-E19A32`: primary/status seam; catalog create exit 0; task add exit 0; task dispatch exit **1** (fleet capacity).
- **C2** `NURALOUMI-R1-16-C2-MULTITOUCH-CANCEL-20261117-B47F20`: touch/focus cancellation; catalog create exit 0; task add exit 0; task dispatch exit **1** (fleet capacity).
- **C3** `NURALOUMI-R1-16-C3-KEYBOARD-REPAINT-20261117-739AC1`: Enter/Space and finite paint; catalog create exit 0; task add exit 0; task dispatch exit **1** (fleet capacity).

Each canonical task remains **queued, not delivered**. ABC reports 80 page targets across eight physical windows and browser-wide allocation ceiling 80 across five fleet slots; slots 2-4 cannot create a target, slots 0-1 require one safe target removal but **zero** inactive recyclable targets are proven. No unsafe fleet cleanup or retry performed. Full prompt bodies including unchanged delegation contract reside in durable OCP catalog and were also materialized with exit 0 into three fully executable local prompt files (outside the repository write scope):

- `/tmp/nuraloumi-r1-16-c-reduced-cancel-prompts/c1-primary-status-20261117-e19a32.md`
- `/tmp/nuraloumi-r1-16-c-reduced-cancel-prompts/c2-multitouch-cancel-20261117-b47f20.md`
- `/tmp/nuraloumi-r1-16-c-reduced-cancel-prompts/c3-keyboard-repaint-20261117-739ac1.md`

The temporary files are a convenience copy, **not** the canonical task identity. Resume only through canonical dispatch when safe capacity exists. Neither catalog admission nor attempted dispatch constitutes completed verification.
