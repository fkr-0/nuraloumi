# NuraLoumi media release audit — 2026-11-12

OCP task: `NURALOUMI-OCP-NURALOUMI-MEDIA-RELEASE-REVIEW-20261112-20261112-B36E4C`.

Audit base: integration commit `bd887669b79b686da6a501a70d93ffb780ec267a`, built from feature `f7ac77758c322facbc6b2adf99d8bfe44ac59248` and local main `85f3730ccff2d0d789381484945276f4ff984113`. Review changes are tracked by the subsequent scoped commit. The dirty source main checkout is **never** used as an unsafe deployment artifact.

## Findings (source paths and reviewed line locations)

| Severity | Location | Finding and disposition |
| --- | --- | --- |
| P1 (fixed) | `crates/nuraloumi-shell/src/player_engine.rs:323` | `Player::command` previously committed playlist mutations even when `select_and_play` failed (e.g. advancing to a missing track, removing an active entry and failing to open the successor). Persisted state could reference a track that never played. Roll back playlist state on error; deterministic absent-file transition regression added. |
| P1 (fixed) | `crates/nuraloumi-shell/src/bin/player.rs:154`, `:845` | A second touch ID overwrote the pending activation, and unmatched releases consumed it; compositor `TouchCancel` was not handled. Now only a matching single-contact release activates a row. Multi-contact and cancellation tests added. |
| P1 (fixed) | `crates/nuraloumi-shell/src/player_engine.rs:299` | A rejected GStreamer open after pipeline reset could leave the UI reporting `Playing` for a stopped pipeline. Explicit NULL cleanup resets player state before reporting failure. |
| P2 (fixed) | `crates/nuraloumi-shell/src/player_engine.rs:509`, `:558` | Failed dynamic symbol lookup and `gst_init_check` could retain handles and an allocated GLib error. RAII closes already loaded handles on all exit paths, reports/frees initialization error, and checks null error messages before dereferencing. |
| P1 (fixed) | `packaging/applications/nuraloumi-player.desktop:4,11` | Advertised a video player and video MIME defaults while video is explicitly `UNSUPPORTED_GPU`. Restrict XDG registration to audio types; manual playback of an audio stream inside a video container remains possible. |
| Release blocker (open) | `.github/workflows/sl101-debian-armhf.yml` and `scripts/inspect-armv7-elf.sh` | Debian ARMhf run 43137891854 rejects `nuraloumi-calendar`: `upper-VFP-register-d16-d31-detected`. The fail-closed architecture gate is **not** relaxed. This may arise from concurrent non-media commits or toolchain differences; root cause not yet established. |
| Release blocker (open) | `scripts/verify-ci-artifact.py:validate_run` | The verified artifacts must have successful `headBranch=main` and exact SHA. Successful feature/integration CI runs cannot pass this criterion. Ephemeral CI APK signature does not enroll device trust. |
| Qualification pending | `crates/nuraloumi-shell/src/player_engine.rs:525` and `:687` | GStreamer miniobject/message layout and GLib variadic calls match declared C types on source review, but AAPCS-VFP hard-float correctness, DSO/decoder closure, and real sink selection have **not** been proven on the SL101 with this binary. |
| P1 (fixed) | `crates/nuraloumi-shell/src/bin/playerd.rs`, `crates/nuraloumi-shell/src/bin/player.rs` | Per-call 300/600ms socket read timeouts could be extended indefinitely by slow-trickle clients or servers. Both directions now have total monotonic read deadlines, bounded buffers and explicit newline termination, with timed regressions. |
| P2 (fixed) | `crates/nuraloumi-shell/src/bin/playerd.rs` | Daemon cleanup previously unlinked any replacement socket at the same path. It now checks original device/inode identity, and stale-socket removal revalidates the path. A malicious same-UID actor and the residual race between the final metadata check and unlink are not proven excluded. |
| Qualification pending | `crates/nuraloumi-shell/src/bin/player.rs` | No physical touch/pixel captures, user-audible sound confirmation, MPRIS export, panel pinning or interactive slider evidence. Only source/unit tests are qualified. |

## Build / CI qualification

- `cargo fmt --all -- --check`: PASS.
- `cargo test -p nuraloumi-shell --offline --bin nuraloumi-player --bin nuraloumi-playerd`: PASS (11 UI and 11 daemon tests after IPC hardening).
- `cargo test --workspace --offline`: PASS.
- `cargo clippy --workspace --all-targets --offline -- -D warnings`: PASS.
- `python3 -m unittest scripts/test_package_integrity.py scripts/test_native_packages_ci.py scripts/test_musl_ci.py scripts/test_debian_armhf_ci.py -v`: PASS (22 integration-tree tests, 20 clean origin/main-based PR tests).
- `desktop-file-validate packaging/applications/nuraloumi-player.desktop`: PASS.
- `git diff --check`: PASS.
- Integration CI exact `bd887669b79b686da6a501a70d93ffb780ec267a`:
  - [Generic CI 43137845819](https://github.com/fkr-0/nuraloumi/actions/runs/43137845819): success.
  - [Alpine ARMv7 musl 43137888456](https://github.com/fkr-0/nuraloumi/actions/runs/43137888456): success.
  - [Debian ARMhf glibc 43137891854](https://github.com/fkr-0/nuraloumi/actions/runs/43137891854): failure (upper VFP D-register gate, calendar executable).
- Clean media-only draft [PR #1](https://github.com/fkr-0/nuraloumi/pull/1) is based on `origin/main` (`6d4f7b2`) without the eight unrelated local commits. At prior PR head `1d2c8fb`, Generic CI [43140170036](https://github.com/fkr-0/nuraloumi/actions/runs/43140170036) and Debian ARMhf [43140169953](https://github.com/fkr-0/nuraloumi/actions/runs/43140169953) completed successfully, while Alpine [43140169905](https://github.com/fkr-0/nuraloumi/actions/runs/43140169905) was still running when checked. This is branch-specific evidence, not main-only verification, and it precedes the latest IPC hardening commit.
- The failed older Debian integration result is not a license to disable the opcode-level check: its HEAD included eight other commits absent in this PR. Compare exact SHA/assembly before attributing differences.
- These branch CI runs **do not** certify the final revision. A fresh exact-head CI is required after commit, then a fresh **main** CI after authorized integration.

## Stage/rollback authority

**NO DEVICE STAGE.** No exact SHA verified from a successful main-branch build and no device-enrolled APK trust exist. `.106` must retain the current NuraLoumi binary, Sway, kernel and symlinks. `.100` is excluded. Hardware AAC/PipeWire evidence from the SL101 coordinator is **not** an execution test of this player.

GPU status: `UNSUPPORTED_GPU`. Never substitute software video or a synthetic pixel outcome for a real Grate/VDE/GLES2 test.

An authorized follow-up must (1) review the media-only PR against origin/main without sweeping in the eight non-media commits, (2) independently qualify the clean PR's exact ARM ABI and investigate the older integration-branch Debian failure without weakening checks, (3) merge only with appropriate authority, (4) use unchanged main-only artifact verifier, (5) stage inactive and run actual playerd/UI WAV/AAC/MP3/seek/volume/EOS/WayVNC tests, capturing package manifest SHA, ELF/closure and sink/pixel/timing observations before device-ready claims.
