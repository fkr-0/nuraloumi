# Offline map fixture

- tile_256.hex: real 256×256 solid-black PNG, hex-encoded so it can be
  reviewed without binary tooling; decodes to 270 bytes (CRC-valid).
- module_harness.rs: compile the unexported module with the existing command
  runner/error modules without editing provider-owned lib.rs.

Run from the workspace root:

    rustc --edition=2021 --test tests/fixtures/maps/module_harness.rs -o target/nuraloumi-maps-tests
    target/nuraloumi-maps-tests

Integrate later in provider-owned lib.rs with pub mod maps; and a shell-owned
Cairo/wl_shm surface. Fetch is intentionally opt-in: no default public tile URL.
