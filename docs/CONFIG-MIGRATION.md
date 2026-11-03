# 0.3 configuration migration foundation

This is a **host-tested, pure migration foundation**, not a claim that IPC
live reload is wired or that the configuration CLI has an installed binary.

## Version policy

The current runtime schema is **v1** (`ShellConfig::default().config_version = 1`).
Missing `config_version` means legacy v0. The only supported upgrade is
**v0 -> v1**, which inserts `config_version = 1` (or JSON equivalent).
Both versions use the same existing semantic fields; no speculative field
renames, missing versions, or v2/v3 conversions are invented. Future versions
are rejected; invalid strict sections are errors. Per-surface and top-level
extensions survive semantic round trips. TOML comments and key order may change
when explicitly migrating; the backup retains the original text.

## Run without shared Cargo registration

The isolated implementation can be compiled as a Cargo example:

    cargo run -p nuraloumi-shell --example config_migrate -- validate --config ./config.toml
    cargo run -p nuraloumi-shell --example config_migrate -- migrate --config ./config.toml
    cargo run -p nuraloumi-shell --example config_migrate -- migrate --config ./config.toml --write

`migrate` is dry-run by default; `--dry-run` may be explicit. `--json`
returns structured diagnostics and the proposed migrated content. `--write`
is never inferred or combined with `--dry-run`. On explicit write the tool:

1. Verifies a regular (not symlink) source file and unchanged contents;
2. Creates `PATH.bak` exclusively with restrictive permissions, never overwrites it;
3. Writes and syncs a same-directory temporary file, retains source mode;
4. Rechecks the source, atomically renames into place, syncs the directory.

If `PATH.bak` already exists the write fails closed. Failures may leave a
backup for manual inspection. No startup path writes or migrates config.
The same-directory atomic rename prevents torn file contents but cannot
fully exclude hostile concurrent writers; ordinary config editing should be
serialized. The CLI accepts only explicit .toml/.json file paths and caps
input size at 1 MiB.

## Integration contract for lane 02

- `src/config_migration.rs::prepare_reload(path, contents)` is pure and
  returns `ValidatedReload { config, diagnostics, source_version }`.
- Runtime owns a **validate -> prepare -> apply** boundary: only after full
  parsing and validation succeeds should it replace the runtime config
  atomically at an input/render-safe point; failed validation must leave the
  active config unchanged and return structured diagnostics to IPC.
- `plan(path, contents)` and `write_explicit(path, plan)` are *operator
  tooling only*, never invoked by startup or IPC reload. Reload accepts
  supported legacy v0 unchanged; migration is opt-in.
- Source file `src/bin/config.rs` is ready for a shared-lane Cargo.toml
  `[[bin]]` registration as `nuraloumi-config`; the standalone example is
  buildable now. Do not claim IPC live reload until lane 02 wires it and
  tests active runtime snapshot apply/rollback and IPC reporting.

Validation and regression tests:

    cargo test -p nuraloumi-shell --test config_migration
    cargo clippy -p nuraloumi-shell --all-targets -- -D warnings

All qualification here is host-only; no SL101 deployment or suspend testing.
