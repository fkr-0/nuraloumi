# NuraBrowser R2: current-source CI root cause (2026-11-19)

Canonical OCP: `NURALOUMI-20261119-NURABROWSER-R2-CI-ROOTCAUSE-20261119-6888ED`

## Provenance and scope

- Local committed HEAD: `387c5e61ca2a338f205a70c7b9f458e7135b65df` on `main`, 52 commits ahead of `origin/main` at inspection.
- Snapshot: `git archive HEAD | tar -x -C /tmp/nuraloumi-ci-6888ed-QQ5Nev`; all tests below run in this clean committed-source snapshot. Concurrent working-tree modifications were excluded.
- Remote history: `gh run list --limit 12 --json databaseId,workflowName,headSha,headBranch,status,conclusion,createdAt` exit 0. Latest listed main CI, Alpine musl ARMv7 and Debian armhf were **success** on `9753fae3895e34735cdb4e3269a86f6590911283` on 2026-11-14; no run of `387c5e61` was observed. This is a **no-run**, not a failed GitHub run.
- CI workflow `.github/workflows/ci.yml` already includes Rust fmt, check, test, clippy, native-package provenance and targeted browser Python contracts. Two SL101 workflows exist. No artifact verification or ABI check was weakened.

## First-failure matrix

| Gate | Exact command (from snapshot root) | Exit | Evidence / classification |
|---|---|---:|---|
| Rust format | `cargo fmt --all -- --check` | 1 | Committed `crates/nuraloumi-shell/src/player_engine.rs:793` contains `<<<<<<< HEAD`; notifications.rs also requires formatting. **Source failure**, not a CI harness failure. |
| Native CI provenance contracts | `python3 -m unittest scripts/test_native_packages_ci.py scripts/test_verify_ci_artifact.py scripts/test_musl_ci.py scripts/test_debian_armhf_ci.py -v` | 0 | 25/25 passed. |
| Browser Python test inventory | `python3 -m unittest discover -s components/nura-browser-engine/tests -p 'test_*.py' -v` | 1 | 70 tests; 1 failure, 1 skipped. `test_offline_qualification.BuildContractTests.test_explicit_pkg_config_linkage` asserts `rename: ',browse'` in Meson, while current `meson.build` installs `nurabrowser-core` privately. **Stale test assertion / ownership drift**. |
| Strict Clippy | `cargo clippy --workspace --all-targets --locked -- -D warnings` | 101 | First hard parse error at committed `player_engine.rs:793` conflict marker; also `clippy::let_and_return` in committed `crates/nuraloumi-shell/src/bin/notifications.rs:97`. **Source failures**, not CI-runner defects. |
| Workspace tests | `cargo test --workspace --locked` | 101 | Committed `player_engine.rs:793` conflict marker prevents `nuraloumi-playerd` test-bin compilation. |
| ARM target / physical SL101 | Not executed | PENDING | Host checks do not substitute for ARM no-NEON closure or Wayland/SL101 qualification. |

## Root causes, ownership and repair boundary

1. **Blocker P0 – Rust parse:** remove committed merge-conflict marker and reconcile affected player implementation under its owning runtime lane. A CI-script change cannot legitimately repair it.
2. **Blocker P0 – Rust formatting and lint:** format the committed notifications source and repair `let_and_return` in the owning notifications runtime lane. Do not suppress warnings.
3. **Blocker P1 – browser test expectation:** update the stale Meson alias assertion to enforce the private `nurabrowser-core` install path and absence of a public browser alias; preserve cross-ABI and source inventory validation. File is in browser-engine test ownership and is deliberately untouched in this CI-only lane.
4. **Coverage P1 – CI browser contracts:** the current GitHub main workflow runs migration/integration tests but not all browser-engine offline and closure unit suites. Once current source is made green, consider a narrow CI workflow addition invoking the full browser Python suite, plus corresponding regression contract checks. Do not add a blanket skip or allow-failure.

No changes to CI workflows were justified before the broken committed source is repaired. No Git push/fetch, remote trigger, device operation, or live session change was made.

## Remote qualification

After separately authorized source repairs and locally green gate matrix, a new GitHub CI run requires explicit owner authorization for a **specific push and SHA/branch**; this task grants none. Remote success must be observed separately for main, Alpine musl and Debian armhf before claiming CI qualification. Device release gate remains independent.
