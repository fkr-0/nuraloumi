# NuraBrowser R2 — Python CI closure (565813)

Date: 2026-11-19. Scope: local host-only tests and main CI discovery.

## Source and ownership

- Baseline: `387c5e61ca2a338f205a70c7b9f458e7135b65df`, shared dirty worktree.
- `.github/workflows/ci.yml` now discovers `test_*.py` for the entire engine test directory instead of invoking only integration and migration modules.
- `test_ci_workflow_coverage.py` checks the exact CI discovery command, its uniqueness, lack of a blanket disabled step, essential module inventory, and the exact private Meson executable/install/absence-public-alias contract.
- `test_offline_qualification.py` was already concurrently modified by owner `sandbox-r2-follow-20261119` under an active path claim. The historical `rename: ',browse'` assertion is presently replaced with internal executable, libexec install path and no public alias checks. This lane deliberately did not overwrite that owner's changes; the independent exact Meson executable/install checks are now enforced in the new CI regression test without editing the other owner's claimed file.
- Other lanes own `.github/workflows/nurabrowser-engine-armhf.yml` and `scripts/test_browser_engine_ci.py`; neither was edited here.

## Local checks

| Command | Result |
| --- | --- |
| `python3 -m unittest discover -s components/nura-browser-engine/tests -p 'test_*.py' -v` | Exit 0, 75 tests, 2 skips (the runtime core was not supplied) |
| `python3 -m unittest scripts/test_native_packages_ci.py scripts/test_verify_ci_artifact.py scripts/test_musl_ci.py scripts/test_debian_armhf_ci.py -v` | Exit 0, 25 tests |

## Remaining gates

This qualification is local only. No Git push, remote CI, deployment, ABI closure or real sandbox/Wayland startup is implied. A CI run can still fail from independently owned Rust source conflicts or missing native cross-build dependencies. The workflow deliberately runs all browser Python tests without a blanket skip; individual fixture-specific skips remain visible.
