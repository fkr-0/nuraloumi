# SL101 Sway compositor renderer probe — 2026-11-09

## Scope and provenance

Host-only NuraLoumi implementation, OCP task `NURALOUMI-15-NURALOUMI-RENDERER-INDICATOR-20261109-AE22EA`.
No tablet command, deployment, compositor switch, or helper invocation is authorized by this work.
Source checkout was newer than the installed panel, which came from release
`menu-dismiss-e4d6301` (2026-10-23); a new panel build/deployment is required.

Coordinator-measured (tablet, 2026-11-09):

- `/run/sl101-desktop.pid` = `23991`; `/proc/23991/comm` = `sl101-sway`.
- Same uid (10000) as panel; `comm`, `stat`, `environ`, and `maps` readable
  at 11, 264, 672 and 20833 bytes respectively.
- `WLR_RENDERER=gles2`, `WLR_RENDERER_ALLOW_SOFTWARE=0`,
  `MESA_LOADER_DRIVER_OVERRIDE=grate`.
- `/opt/grate-mesa25-musl-0052-20261108/lib/libgallium-25.0.7.so` was mapped.
  Future prefix example: `/opt/grate-mesa25-musl-prod70-20261109`.

The `tests/fixtures/providers/renderer/sl101-sway-20261109.maps` fixture
preserves the measured mapped library pathname; its address, offset and
device/inode columns are synthetic valid `/proc/maps` formatting, **not** a
verbatim device dump. The test environment bytes preserve the three measured
values (NUL-delimited). Fixture start time is synthetic.

## Classification contract

`Pixman` requires `WLR_RENDERER=pixman`.
`Grate` requires all three measured environment values and a library
**pathname** in a parsed `/proc/<pid>/maps` row matching one of:

- `/opt/grate-mesa25-musl-<label>/lib/libgallium-25.0.7.so`
- `/opt/grate-mesa25-clean/lib/arm-linux-gnueabihf/libgallium-25.0.7.so`
- `/usr/local/lib/sl101-grate-<label>/lib/libgallium-25.0.7.so`
  (also the legacy direct `<label>/libgallium-25.0.7.so`)

Other `WLR_RENDERER=gles2` observations yield `GLES2`. Missing PID, unsupported
`comm`, unreadable or oversized files, failed `stat` start-time verification,
and unsupported environment yield `Unknown`. All reads remain bounded.

This is process/config/library evidence, not a direct GPU hardware utilization
counter. It does not establish error-free pixels, rendering performance, or
browser GPU operation.

## Host verification

Run from the NuraLoumi Git root:

```sh
cargo test --locked -p nuraloumi-providers --lib renderer::tests
cargo test --locked -p nuraloumi-shell --lib renderer_switch_is_confirmed_and_requires_capability
cargo test --locked -p nuraloumi-shell --lib unknown_renderer_is_unavailable_in_panel_and_system_controls
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Host results (2026-11-09): `cargo test --workspace` PASS; strict Clippy PASS;
changed Rust files format-check PASS; `cargo fmt --all -- --check` FAIL on
pre-existing formatting drift in `nuraloumi-render-cairo/src/model.rs` and
four unrelated `nuraloumi-shell` files (panel/run/windows/config). No unrelated
formatting was committed. `git diff --check` PASS.

## Build the exact ARMv7 hard-float musl release panel (host only)

Follow `docs/qualification/ARMV7.md` and `scripts/build-sl101-armv7.sh`.
The existing wrapper builds **all** declared shipping binaries and audits ARMv7
ELF attributes; it includes the panel. It requires the already prepared,
matching sysroot and pkg-config files under `target/sl101-cross`.

```sh
# Only if the target is not installed:
rustup target add armv7-unknown-linux-musleabihf

# If the tablet-matched sysroot is already populated:
sh scripts/build-sl101-armv7.sh
# The wrapper is currently not marked executable in Git; invoke it with sh.
# This is equivalent to the repository's release cross-build recipe;
# it sets the matching linker, sysroot, pkg-config and -neon/-d32 flags.

# Inspect but do not install:
file target/armv7-unknown-linux-musleabihf/release/nuraloumi-panel
scripts/inspect-armv7-elf.sh target/armv7-unknown-linux-musleabihf/release/nuraloumi-panel
cargo xtask qualify-armv7 --cross-check
```

Host result (2026-11-09): `sh scripts/build-sl101-armv7.sh` PASS for all
22 declared shipping binaries, including ELF audit. `cargo xtask qualify-armv7
--cross-check` exits 0 with `CROSS_CHECK=PASS`, `ELF_AUDIT=PASS` and
`DEVICE_RUNTIME=PENDING`, as expected before tablet validation. The panel
binary has SHA-256 `a152054ffe2ae0bcc160bd633bc253e844f5f5f09d083b66cd93d24a8dce72f2`
at the time of the build; use a fresh hash after any subsequent rebuild.

If the sysroot is missing or stale, the coordinator (not this host-only task)
must explicitly authorize `sh scripts/build-sl101-armv7.sh --prepare` or the
documented `scripts/prepare-sl101-sysroot.sh <tablet-SSH-target>`. Do not
silently fetch a new sysroot or use generic glibc/NEON builds. Cross-link success
does **not** constitute tablet qualification.

## Post-deployment measurement required from coordinator

With the **new** panel binary running in the actual Sway session, record
its binary hash/build identity and the compositor PID/comm. Confirm the top-right
label and the System control-center `Compositor renderer` both show **Grate**.
Check that no switch helper was called by observation, and inspect that its
command remains `sudo -n /usr/local/sbin/sl101-renderer-switch <mode>` only
behind explicit authorization. Verify `GLES2` (other renderer or missing
required grate precondition), `Pixman` (pixman compositor), and `Unknown`
(uninspectable compositor) only when corresponding sessions are safely
available; do **not** switch the active desktop to create these observations.

The frozen panel battery and clock values are a separately scoped event-loop
problem and were not modified. No causal connection was established by the
renderer probe changes.
