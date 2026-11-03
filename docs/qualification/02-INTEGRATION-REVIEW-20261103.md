# 0.2 qualification integration review — 2026-11-03

**OCP task:** `NURALOUMI-09-QUALIFY-20261103-48EE45`
**Reviewer scope:** independent inspection of landed integration commits and
self-contained tests for the qualification lane; no runtime source edits.
**Verdict:** **NOT RELEASE QUALIFIED**. This is an intermediate evidence record,
not a 0.1.0 or 0.2 release authorization.

## Reviewed baseline and changes

Review baseline (observed HEAD): `a4cd6ff`. Reviewed committed lane commits
via `git show --check` and scoped `git show --stat`:

- `31a8268`: Wayland rotated touch and keyboard capability work.
- `88ec750`: calendar and weather providers.
- `c053fa5`: shell Wave 2 binaries, glyphs and bounded IPC.
- `5e7b5aa`: offline map tile and viewport foundation.
- `a4cd6ff`: RSS/Atom feed parser and offline fixtures.

`git show --check` passed on all five. **This is a commit whitespace/
metadata check, not behavioral review of each module.** The current worktree
has active uncommitted provider/shell changes from parallel lanes; results
are a point-in-time observation, not a frozen release candidate.

## Qualification tooling delivered by lane 09

- Runtime package binaries enumerate Cargo metadata for the shell,
  Wayland and providers crates (no hardcoded initial four).
- `package-check --dir` fails closed on missing/extra package files,
  SHA-256 mismatch, unsafe manifest paths, wrong binary set, missing
  required config/license/docs, symlinks, or missing ELF magic.
- For an ARMv7 package, each packaged binary must pass the existing
  ELF attribute/disassembly heuristic. `qualify-armv7` now refuses
  whole-set ELF PASS if any declared target binary is missing.
- Bounded read-only RSS/CPU sampler (max 16 PIDs, 60 samples, 120 s),
  with per-PID start-time identity to detect reuse; no device mutation.
- Copy-only future rollback instructions scoped to recorded trial PIDs.
- Isolated temporary Cargo workspace for end-to-end tamper/omission
  fixture tests, independent of nine concurrent workers.

## Host verification and evidence boundaries

| Check | Observed result |
| --- | --- |
| `cargo test --locked -p nuraloumi-xtask` | PASS, 12/12 |
| `python3 -m unittest discover -s scripts -p 'test_*.py'` | PASS, 11/11 |
| `cargo fmt --package nuraloumi-xtask -- --check` | PASS |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | PASS |
| `git diff --check` | PASS |
| `sh -n scripts/build-sl101-armv7.sh scripts/inspect-armv7-elf.sh` | PASS |
| `cargo test --locked --workspace` | **FAIL** in uncommitted screenshot test |
| `python3 scripts/package-binaries.py` | **FAIL**, unprefixed screenshot target |
| `cargo xtask qualify-armv7` | **FAIL** at package target enumeration; no whole-set ELF PASS |
| Real SL101 input, RSS, startup, no-SIGILL | **NOT TESTED** |

Prior four-binary ARM cross outputs passed the existing VFPv3-D16/no-NEON
heuristics, but that is not proof of **all** recently declared binaries.
The ARM target and musl linker are installed and `.cargo/config.toml`
disables NEON, d32, hwdiv and hwdiv-arm. Cross-check compilation was not
requested in this run; target hardware was not contacted.

## Integration findings to resolve

1. **Release blocker — runtime binary naming:** a concurrent screenshot
   file creates Cargo binary `screenshot`, which violates the
   `nuraloumi-*` package target contract. The owner must explicitly name
   it `nuraloumi-screenshot` or reconcile the contract after review.
   Do not bypass enumeration by silently dropping the binary.
2. **Release blocker — workspace test:** `crates/nuraloumi-shell/src/bin/screenshot.rs`
   fails `tests::fixture_actions_never_implicitly_copy` at line 467
   (actual 1, expected 3). Owner: screenshot/shell integration lane.
   The qualification lane has not modified that file.
3. **Independent-review authority pending:** the project independent LLM
   review endpoints reported `Tool ... not found`; this lane has performed
   its own read-only inspection but must **not** self-approve. The managed
   work packet should be submitted to an eligible separate reviewer.
4. **Hardware gates:** no fresh ARM cross-release bundle was produced for
   the new target set. No real SL101 labwc/pixman startup, screenshot
   interaction, rotated touch/slider keyboard, no-SIGILL, RSS/CPU or frame
   latency tests were run. No deploy/suspend was attempted.

## Next gate and rollback

Once the owners settle the screenshot target and fixture: rerun exact
Cargo-metadata binary discovery, workspace tests, strict Clippy, host fixtures
and all-package ARM cross ELF checks on a frozen HEAD. Then request an
independent reviewer of the integrated commits and packet. Human-attended
physical-device trials are separately authorized and follow
`SL101-DEPLOYMENT.md`, not this document.

When changing trial versions on a future authorized device run, retain the
known-good stage and only terminate **recorded trial PIDs**, verifying PID
start time before escalation; never kill all NuraLoumi processes or replace
labwc/pixman recovery. Never claim a release from host results.

## Later same-day integration update (after qualification commit)

After the screenshot lane advanced, a **direct** rerun of
`cargo test --locked --workspace` returned exit code **0** across the
workspace, including screenshot fixtures; strict workspace Clippy again
returned exit code **0**. Therefore the screenshot unit failure above is a
**historical, resolved point-in-time observation**, not a current blocker.
The package binary naming problem **remains reproducible**:
`python3 scripts/package-binaries.py` exits 2 with
`invalid binary name: 'screenshot'`. The runtime Cargo metadata must be
reconciled by its owning shell lane. Verify the **final Cargo target list**;
an explicit `[[bin]]` alone may not suppress implicit autobin discovery
without `autobins = false` or a filename adjustment.

Package completeness, target-cross compile and hardware acceptance remain
pending after this update. The reviewed qualification commit is
`59bef4769bfbe2a2630639bc33cccac2191a3144`.
