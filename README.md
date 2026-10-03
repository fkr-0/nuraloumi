# NuraLoumi

NuraLoumi is a low-resource, touch-first Wayland menu shell for the ASUS Eee Pad Slider SL101 ("Nura") and similar constrained Linux tablets.

It is intentionally **not a desktop environment**. NuraLoumi runs on top of a compositor such as labwc/wlroots and keeps a software-rendered pixman/Cairo path as the reference baseline. EGL/GLES acceleration is optional.

## What it provides

- native Wayland layer-shell panel and menu surfaces
- Cairo + `wl_shm` rendering with no XWayland or GUI toolkit requirement
- touch, pointer, and physical-keyboard navigation through one semantic action path
- launcher discovery from XDG `.desktop` files
- conservative existing-window reuse through foreign-toplevel `app_id` matching
- Wi-Fi, Bluetooth, audio, brightness, power, task, window, workspace, media, and notification surfaces where the backing provider is available
- fixture-driven/headless operation for development and testing
- ARMv7 / Tegra20 qualification with no NEON requirement

## Architecture

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

The main boundaries are documented in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and [docs/CONTRACTS.md](docs/CONTRACTS.md). The planned transparent/configurable keyboard model is specified separately in [docs/KEYBINDINGS.md](docs/KEYBINDINGS.md); it is a design contract, not yet a runtime feature.

## Build

A current stable Rust toolchain plus the native Cairo/Wayland development libraries are required.

```sh
cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

For an optimized native build:

```sh
cargo build --workspace --release --locked
```

The SL101 cross-build and target ABI checks are documented in [docs/qualification/ARMV7.md](docs/qualification/ARMV7.md) and [docs/qualification/SL101-DEPLOYMENT.md](docs/qualification/SL101-DEPLOYMENT.md).

## Run

Headless menu inspection is useful on a development machine:

```sh
cargo run -p nuraloumi-shell --bin nuraloumi-menu -- --family launcher
cargo run -p nuraloumi-shell --bin nuraloumi-menu -- --family system
```

On a compositor exposing layer-shell:

```sh
cargo run -p nuraloumi-shell --bin nuraloumi-panel -- --live
cargo run -p nuraloumi-shell --bin nuraloumi-menu -- --live --family launcher
```

System-changing actions remain provider-gated. Power actions require explicit opt-in; suspend has an additional safety gate. See [docs/menu-integration/README.md](docs/menu-integration/README.md).

## Configuration

Both `nuraloumi-menu` and `nuraloumi-panel` accept:

```text
--config <path>
```

Without an explicit path, NuraLoumi automatically looks for `nuraloumi/config.toml` (then `config.json`) under an absolute `$XDG_CONFIG_HOME`; otherwise it uses `$HOME/.config`. If no file exists, built-in defaults are used.

A complete TOML example is included at [examples/nuraloumi-config.toml](examples/nuraloumi-config.toml):

```sh
config_root="${XDG_CONFIG_HOME:-$HOME/.config}"
case "$config_root" in
  /*) ;;
  *) config_root="$HOME/.config" ;;
esac
mkdir -p "$config_root/nuraloumi"
cp examples/nuraloumi-config.toml "$config_root/nuraloumi/config.toml"
```

Launcher configuration remains a preference layer over discovered XDG desktop entries. It can pin, hide, or relabel discovered apps and define exact `app_id` aliases for conservative existing-window reuse; it cannot introduce arbitrary executable commands.

See [docs/CONFIGURATION.md](docs/CONFIGURATION.md) for the full schema, precedence, validation, and safety rules.

## Releases

Release history and compatibility notes are tracked in [CHANGELOG.md](CHANGELOG.md). The current release line is `0.0.2`; `0.0.1` remains the first tagged release.

## Workspace

```text
crates/
  nuraloumi-core/          renderer-neutral menu/action/tokens/state model
  nuraloumi-render-cairo/  layout + Cairo scene renderer
  nuraloumi-wayland/       Wayland shm/layer-shell/input/output backend
  nuraloumi-providers/     system/app/process provider adapters
  nuraloumi-shell/         panel + launcher/control surfaces
  nuraloumi-xtask/         build, packaging, fixtures, target qualification helpers
docs/
  ARCHITECTURE.md
  CONTRACTS.md
  CONFIGURATION.md
  SL101-QUALIFICATION.md
  qualification/
examples/
  nuraloumi-config.toml
  menu-fixtures/
```

## Development policy

The software renderer is the compatibility baseline. Startup must never require EGL/GLES, external commands must remain bounded and fixture-testable, and render code must not execute system commands.

Before submitting changes:

```sh
cargo fmt --all -- --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

See [ROADMAP.md](ROADMAP.md), [DESIGN.md](DESIGN.md), and [docs/AGENT-LANES.md](docs/AGENT-LANES.md) for project direction and ownership boundaries.

## License

MIT. See [LICENSE](LICENSE).
