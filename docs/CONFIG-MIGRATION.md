# 0.3 configuration migration foundation

This includes the **host-tested, opt-in migration foundation** and a
bounded transactional live-reload path for the running panel. It does not
claim a deployed SL101 binary or hardware-qualified Wayland reload.

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

## Transactional live reload (host-qualified)

`nuraloumi-ctl reload-config` requests a fresh bounded JSON/TOML read
from the explicit panel `--config` path, or from the same XDG config
discovery order as startup. An absent/unreadable file **does not** reset the
currently running configuration. Inputs are capped at 1 MiB.

- `config_migration::prepare_reload` parses and validates the candidate.
  `config_reload::prepare_config_reload` classifies immutable changes
  and validates the replacement keybinding registry before any mutation.
- The IPC worker hands the command to the Wayland event owner through a
  one-byte wake descriptor and waits for a bounded acknowledgment. The owner
  alone stages and commits the replacement menu/application/config state.
  There is **no background config polling** and no provider mutation from
  rendering or the IPC thread.
- Supported live changes: dark/light theme, reduced motion, launcher
  labels/pinning/filter preferences, keybinding customization, and
  `menu_dismissal.after_one_shot_action`. The UI is redrawn and
  selected item identity is retained when still valid.
- Structural changes return `restart_required` without applying any
  part: panel edge/height, menu width, row height, quick-launch slots,
  provider sections, per-surface settings, and outside-press backdrop policy.
- Parse/validation/structural/staging/render errors return an explicit IPC
  error and restore previous in-memory runtime state and selection. If
  surface presentation fails after partial Wayland buffer submission, a
  visual rollback cannot be guaranteed; the active state still rolls back.
- The migration planner/writer remains separate operator tooling only.
  Reload never writes a migration or backup, and supported v0 sources may
  reload unchanged.

Regression commands:

    cargo test -p nuraloumi-shell --test config_migration
    cargo test -p nuraloumi-shell --test config_reload
    cargo test -p nuraloumi-shell
    cargo clippy -p nuraloumi-shell --all-targets -- -D warnings

Qualification is **host-only**. SL101 Wayland/Cairo integration, latency and
ARMv7/no-NEON runtime verification remain separate gates; no deployment or
suspend experiment was performed here.
