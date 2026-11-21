# NuraLoumi 0.0.8-rc.1 guarded deployment gate — 2026-11-19

OCP: NURALOUMI-20261119-SL101-008-RC1-GUARDED-DEPLOYMENT-20261118-258616

Status: NEEDS_RETRIGGER. No device mutation, reboot, suspend, remote Git write or CI trigger.

## Verified observations

- Original 0.0.8-rc.1 candidate: 00215f57d80ce0c6a9864f46888a11060f363105, locally committed and ancestor of current HEAD.
- Current HEAD: 0c8622986875e7f5c8ab8ad31411b3e313b10c3e; main is 70 commits ahead of origin/main; worktree has unrelated tracked/untracked changes.
- Read-only gh run list for sl101-musl-armv7.yml main: latest success run 43566257797 at SHA 9753fae3895e34735cdb4e3269a86f6590911283 (0.0.7-dev.1, November 14). Host CI latest success run 43566257804, same SHA.
- There is no observed successful official main-branch musl CI run for either the RC candidate or current HEAD. Consequently no exact-SHA validated 0.0.8 artifact, artifact ID, package hash or verified ABI closure can be reported.
- scripts/sl101_userspace_release.py line 26 still pins EXPECTED_VERSION = 0.0.7-dev.1. It is NOT a safe 0.0.8 installer.
- The repository's docs/qualification/SL101-DEPLOYMENT.md prohibits local host-build deployment and requires verified original CI artifact plus target-specific qualification.
- No target SSH preflight attempted: official exact-SHA artifact gate has already failed. Device identity, kernel, free space, current symlink, Sway/player state and rollback availability remain UNKNOWN.

## Gate matrix

| Gate | State | Required proof |
|---|---|---|
| Initial source candidate exists | PASS | 00215f57 committed, ancestor of HEAD |
| Freeze final source SHA | PENDING | Decide candidate versus later committed main |
| Official successful main ARMv7/musl CI for exact SHA | FAIL | Only 0.0.7 success observed |
| Artifact SHA256, manifest, ELF/ISA/linker ABI closure | PENDING | verify-ci-artifact.py with exact SHA/run |
| Version-correct deployment helper | FAIL | 0.0.7 hard-pin still active |
| Read-only target identity, kernel, ABI, dependencies | PENDING | Safely inspect device |
| Space, idle Sway/player, staging and rollback | PENDING | Device preflight and operator idle approval |
| Staged/activated/smoked | PENDING | Not attempted |
| Existing Sway, pixman, Grate, player untouched | PASS | No device commands performed |
| No reboot/suspend | PASS | Neither performed |

## Precise authorization boundary

Owner separately authorizes the frozen candidate's required remote Git push and/or CI trigger, specifically naming the source SHA and avoiding accidental publication of unrelated subsequent commits. Device deployment authorization alone does not permit these. Obtain official successful main-branch Alpine ARMv7/musl run and passing host CI for that exact SHA.

## Guarded resume runbook

1. Freeze and review exact committed SHA/version, assess intervening commits, preserve all unrelated edits. Read-only inspect official CI run and artifact list.
2. Verify on host with: python3 scripts/verify-ci-artifact.py --lane alpine --run-id RUN --sha SHA --out ISOLATED_OUTPUT. Preserve run/artifact IDs, SHA256SUMS, manifest, ELF audit, no-NEON/VFPv3-D16 evidence, loader and full dependency closure. Reject old 0.0.7 artifacts.
3. Read-only preflight .106 using existing task-scoped SSH: sanitize identity; inspect uname, os-release, runtime loader and installed libraries, df, current release/symlink/staging, Sway and player process state, private video stack, known pixman fallback and rollback. Confirm idle user session/recovery path. Stop if any uncertain.
4. Use an explicitly reviewed version-correct release procedure. First DRY RUN. Stage only the verified original tar in a new unprivileged, versioned path, no overwrite and no system APK install. Rehash on tablet, inspect target loader/libs, do not change active symlink.
5. Activate only if atomic/process-scoped handoff and restoring previous release are proven. Never use global pkill or kill Sway. If uncertain, stop after staging without activation.
6. Target smoke: no SIGILL, panel startup, keyboard and touch, compositor/PID preserved, player/playerd with private Grate/GStreamer stack. On mandatory failure restore original release target and process state, verify compositor. Record source/artifact/package hashes and before/after/rollback evidence. No reboot, suspend, firmware, kernel, DTB, Mesa, Sway or system-lib changes.

Next task: authorize a specific remote Git/CI operation for a selected SHA, complete official CI, then retry artifact verification and device preflight. This report does not claim deployment.
