# QoL visual geometry QA — 2026-11-17

## Evidence classification

- **Automated:** `python3 -m unittest discover -s scripts -p test_qol_ux_audit.py -v` (six checks pass).
- **Synthetic fixtures only:** `python3 scripts/qol_ux_audit.py tests/fixtures/qol/scenes.json`.
- **Not demonstrated:** runtime scene export, compositor screenshots, pixel comparisons, SL101 pointer/touch capture or post-merge device qualification.
- The `after-*` fixtures describe **desired contracts**, not observed fixes. The `baseline-*` fixture represents screenshot-reported defects rather than machine-captured before-state.

## Source-grounded geometry seams (read-only to this lane)

| Concern | Source / dependent owner | Fixture defect code |
| --- | --- | --- |
| Panel-to-menu separation | `crates/nuraloumi-shell/src/bin/menu.rs` popup `margin_top`; `crates/nuraloumi-shell/src/bin/panel.rs` panel height and popup placement | `menu-gap` |
| Model scene/hit regions | `crates/nuraloumi-render-cairo/src/layout.rs` `layout_menu_scaled`; `crates/nuraloumi-shell/src/lib.rs` `hit_regions` | `tap-target:*`, `hit-overlap:*` |
| Menu dismissal | `crates/nuraloumi-shell/src/lib.rs` `dismiss_backdrop_margins`; `crates/nuraloumi-shell/src/bin/menu.rs` dismiss backdrop | `outside-dismiss` |
| Compact file status/navigation | `crates/nuraloumi-shell/src/bin/files.rs` menu rows / location | `duplicate-label`, `status-fields`, `toolbar-overlap:*` |
| Scroll and tabs | `crates/nuraloumi-render-cairo/src/layout.rs` scene; `crates/nuraloumi-shell/src/bin/menu.rs` input | `scroll-visibility`, `scroll-drag`, `tabs` |
| Glyph/title presentation | `crates/nuraloumi-render-cairo/src/layout.rs` scene + paint | `phantom-glyph`, `thick-title` |

Any patch to these seams belongs to the owning lane, not this QA lane.

## Contract exercised

The JSON scene export contract records `viewport=[width,height]`, `scale`, `panel/menu=[x,y,w,h]`, `controls[].hit`, `item_rows`, three status rectangles, tabs, outside-dismiss capability and scroll metrics. Units are **logical pixels**, not device pixels. Logical minimum for each control hit rectangle is 48×48 regardless of fractional output scale. Menu start is within 1 logical pixel of the panel's bottom edge, and menu rectangle must fit the viewport. A scrollbar appears iff `content > viewport`; when present, it must have a draggable thumb and track hit width >=48 logical pixels. Distinct toolbar buttons cannot overlap each other or menu item rows.

The tool **does not currently export real Rust scenes**: fixture authors must generate compatible JSON from a known runtime snapshot. Consequently, a clean synthetic fixture is not a runtime pass.

## Deterministic results

| Scenario | Fixture assertion |
| --- | --- |
| baseline-1280x800 | 9 predicted failures: duplicate-label, menu-gap, outside-dismiss, phantom-glyph, scroll-visibility, status-fields, tabs, tap-target:mute, thick-title |
| after-1280x800 | 0 predicted failures |
| after-854x480 | 0 predicted failures |
| after-800x480 | 0 predicted failures |
| after-fractional-854x480 (scale 1.25) | 0 predicted failures |

Mutation tests independently check exact-fit scrolling, one-row-short, one-pixel overflow, 47.99px controls, overlapping hit regions, and NaN rejection.

## Required owner-lane requalification

1. Export actual current Rust layout/hit-regions in the JSON contract and compare before/after on all widths; do not pass through manually constructed idealized geometry.
2. Independently verify scrollbar track/drag and outside pointer/touch dismissal with an instrumented event harness. A compositor backdrop cannot be inferred from scene JSON.
3. Verify control vs list row semantics, status row fields and glyph rendering optically on the target; the JSON-only checks are **necessary, not sufficient**.
4. Check `cargo fmt --all -- --check`, `cargo test --workspace` and strict `cargo clippy --workspace --all-targets -- -D warnings` against a quiescent integration HEAD; concurrent dirty files may introduce unrelated failures.

## Safety

No runtime crates, GPU/Grate ABI, Wayland session, installation or hardware state were changed by this lane. The audit has no subprocess besides its own test CLI and performs no network I/O.
