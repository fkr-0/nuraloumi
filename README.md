# NuraLoumi

NuraLoumi is a low-resource, touch-first menu shell for the ASUS SL101/Nura and similar constrained Linux tablets.

It combines the strongest parts of two existing projects without importing either desktop stack wholesale:

- **DeskHalloumi / unilii** supplies the semantic model: typed actions, sections, status rows, submenus, search, confirmation flows, design tokens, motion rules, and keyboard/pointer parity.
- **modrelease-2** supplies the renderer direction: compact direct Cairo primitives, predictable layout, simple hit-testing, rounded surfaces, gradients, and software-first rendering.
- **NuraLoumi** adds a native Wayland layer: wl_shm buffers, layer-shell popups/panels, touch input, output scaling/rotation, and a labwc/pixman-safe runtime.

## North-star stack

```text
providers ──> shell adapters ──> semantic menu model ──> scene/layout ──> Cairo ImageSurface
                                                       │
                                                Wayland wl_shm
                                                       │
                                                layer-shell
                                                       │
                                              labwc / wlroots
                                                       │
                                     pixman baseline / GPU optional
```

NuraLoumi must remain usable when GPU acceleration is unavailable.

## Workspace

```text
crates/
  nuraloumi-core/          renderer-neutral menu/action/tokens/state model
  nuraloumi-render-cairo/  layout + Cairo scene renderer
  nuraloumi-wayland/       Wayland shm/layer-shell/input/output backend
  nuraloumi-providers/     battery/backlight/network/audio/session snapshots
  nuraloumi-shell/         panel + launcher/system/audio/network menu binary
  nuraloumi-xtask/         build, packaging, fixtures, target qualification helpers
docs/
  ARCHITECTURE.md
  CONTRACTS.md
  AGENT-LANES.md
  SL101-QUALIFICATION.md
ROADMAP.md
DESIGN.md
tasks.yml
```

## Development

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo build --workspace --release --locked
```

See **ROADMAP.md** for release sequencing and **docs/AGENT-LANES.md** for the parallel implementation wave.
