# Wire an exact-source SL101 GPU video bundle into NuraLoumi CI release artifacts

## Goal
Make the official Alpine ARMv7/musl CI release able to produce and verify a unified NuraLoumi runtime artifact containing its one existing player/playerd and a source-proven private Grate/GStreamer video bundle, without treating host-local private binaries as eligible CI output.

## Context
- Current NuraLoumi 0.0.7-dev.1 on SL101 .106 already has a private video bundle, but its normal GitHub .github/workflows/sl101-musl-armv7.yml produces Rust binaries; the dedicated GPU bundle is presently opt-in via NURALOUMI_VIDEO_BUNDLE_DIR. This is not yet a clean cross-repository CI provenance chain.
- sl101_linux peer project is implementing clean 0092 Grate/GStreamer producer in a separate concurrent worker. Independently define the consumer contract now, and fail closed pending its exact source artifact. Do not fetch unpublished private trees.
- Target ARMv7 musleabihf hard-float VFPv3-D16, no NEON, no hardware division. Never mix with Debian armhf glibc. Legacy browser C/WPE engine remains independently gated, not automatically included.
- Read docs/qualification/NURALOUMI-PLAYER-E2E.md, docs/qualification/MUSL-GITHUB-CI.md, docs/qualification/SL101-DEPLOYMENT.md, scripts/test_nuraloumi_video_staging_20261114.py, crates/nuraloumi-xtask/src/package.rs, scripts/build-ci-alpine-apk.sh, scripts/sl101_userspace_release.py.
- Separate CI Rust/Browser Python repairs occurred in R2; preserve parallel changes and do not duplicate the R2 browser ownership lanes.

## Scope
- In: focused CI workflow/package manifest/validator changes and tests enabling opt-in, exact-source GPU artifact consumption with a versioned independent producer provenance. Verify full private ELF and GStreamer plugin closure; source hashes, artifact SHA and ABI are mandatory.
- Out: browser engine/core, player runtime behavior, untrusted URLs, device mutation, Git push/tag, release activation, suppressing CI failures, or globally replacing Mesa/GStreamer.
- Respect current .github/workflows/ci.yml and any other active path claims. Only edit paths after acquiring claims; isolate work if overlap exists.

## Acceptance
1. Provide a reproducible CI contract: same source revision and target ABI in producer and consumer metadata; refuse absent/untrusted bundle, mismatched SHA, wrong plugin, loader/soname collisions or missing no-NEON module coverage.
2. Existing software-only CI artifacts remain honestly software-only; a GPU-video-qualified artifact appears only when the pinned private bundle is actually built and verified. No silent fallback and no inaccurate hardware qualification claim.
3. Test that packaging contains exactly one nuraloumi-player and playerd, libnuraloumi-video.so, private GStreamer plugin and full Grate/EGL/GBM/libdrm closure, no duplicate media player, no browser contamination.
4. Run Python packaging/CI contract tests, cargo format/test/clippy where practical. If external CI access requires a separately authorized push, stop before push and report the exact commit and required CI actions.
5. Produce a concise guarded .106 staging/rollback checklist using only SHA-verified official CI artifacts, without touching the live device.

## Deliver
- A clean, independently reviewable commit in governed worktree if feasible, no remote push.
- Final agent_checkpoint with exact files, tests, source-chain remaining blockers and follow-up for official CI artifact build.