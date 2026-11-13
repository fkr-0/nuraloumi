# NuraLoumi 0.0.4-rc.1 — staged source candidate evidence

Date: 2026-11-13
OCP: NURALOUMI-05-RC1-RELEASE-INTEGRATION-20261113-ED472E
Candidate branch: work/nuraloumi-0.0.4-rc1-final-20261113
Base origin/main: 34170f5 (includes media commits and pinned-nightly Debian build-std experiment)
Proposed tag: 0.0.4-rc.1 (**NOT CREATED**)
Status: **BLOCKED for release, tagging and deployment**

## Scope and isolation

- Based on latest observed origin/main 34170f5: media implementation and IPC
  hardening are already integrated there (c7c6194, 77c71b2, 24dd60b).
  This branch does NOT duplicate those commits. Candidate-only changes
  cover ARMHF CI, seek/error guard, release version and evidence.
- Dedicated worktree; no modifications to the dirty main checkout.
  Panel/lib/UX path claims and notification lanes are explicitly excluded.
- Candidate workspace version and Cargo.lock: 0.0.4-rc.1.
- Local-only panel UX commit c374642 is not in origin/main; it changes
  panel.rs/lib.rs and remains outside this branch pending separate scoped
  review/integration. No claim that its clock/icon changes run on .106.
- Base 34170f5 already rebuilds Rust std using pinned nightly-2026-11-12.
  This candidate adds mapping-symbol retention and checks the rust-src
  component before building; exact-SHA CI still required.
- No 0.0.5 resource-manager or 0.0.6 notification-center features promoted.
- No deployment, package installation, service restart, production tag,
  or push to main. A draft candidate PR may be published solely for CI
  without authorizing merge or hardware activation.

## Debian ARMHF VFPv3-D16 failure and candidate correction

Previous Debian job 43137891854 failed at nuraloumi-calendar with
upper-VFP-register-d16-d31-detected. Its exact binary is not in this
worktree; whether it was a real illegal opcode or disassembly of data
was not established.

The candidate CI:
- keeps pinned nightly and rust-src, rebuilding Rust std/panic_abort with
  -neon,-d32,-hwdiv,-hwdiv-arm target features as application code;
- sets CARGO_PROFILE_RELEASE_STRIP=debuginfo, retaining ARM mapping
  symbols so the disassembler can distinguish inline data from code;
- retains exact ELF attribute checks, instruction-based D16-D31,
  NEON and hardware-divide rejection, hard-float ABI and loader checks.

**Fix is candidate-only; ARMHF cross-build and ELF audit MUST still run
against the new exact commit and may reject it. No downgrade of the guard.**

## Host evidence (isolated worktree)

| Gate | Command | Result |
| --- | --- | --- |
| Formatting | cargo fmt --all -- --check | PASS |
| Unit + integration | cargo test --locked --workspace | PASS |
| Strict warnings | cargo clippy --locked --workspace --all-targets -- -D warnings | PASS |
| Player and daemon IPC/seek | cargo test --locked -p nuraloumi-shell --bin nuraloumi-player --bin nuraloumi-playerd | PASS, 24 tests |
| CI/ABI/package verifier contracts | python3 -m unittest scripts/test_debian_armhf_ci.py scripts/test_musl_ci.py scripts/test_native_packages_ci.py scripts/test_package_integrity.py scripts/test_verify_ci_artifact.py -v | PASS, 27 tests |
| Diff whitespace | git diff --check; git diff --cached --check | PASS |

Tests above are host/source tests, not evidence of target ABI or
physical device execution.

## Required gates before RC authorization

1. Freeze exact candidate commit SHA and verify generic, Alpine ARMv7 musl
   and Debian ARMHF glibc CI jobs on that **same SHA** all succeed.
2. Validate each exact artifact: complete binary inventory, ELF32 ARM
   hard-float VFPv3-D16 with no NEON/D32/hwdiv opcode, glibc/musl ABI
   closure, MANIFEST git_dirty=false, and per-file/package SHA256.
3. Independently review this integration diff and all security/IPC/FFI
   and packaging changes. OpenCode review backend was unavailable during
   preparation; this is **not** a substitute for independent approval.
4. Review rollback and inactive-stage plan. Candidate binaries may be
   placed under /home/user/testbins only after all preceding gates pass;
   never overwrite production.
5. For authorized .106 Sway/WayVNC hardware validation, capture a real
   screenshot showing HH:MM in the panel rather than the old literal
   'Clock'. Verify input, audio seek/EOS, fakesink vs audible output,
   memory, no-SIGILL and service rollback. No source-only claim satisfies it.
6. .100 remains untouched and must be separately ABI-qualified later.

Previous CI runs for other revisions, including successful media-only
PR #1 Debian jobs, cannot attest this candidate. The main job 43169325297
and integration Debian 43137891854 were recorded as failed; these results
are not waived.

## Next tag decision

**DO NOT CREATE 0.0.4-rc.1** until exact-SHA CI green, independent review,
ABI/artifact integrity, device validation/rollback authorization and
operator approval. If any gate fails, keep this as an untagged staging
branch and prepare a corrected candidate rather than moving a tag.
