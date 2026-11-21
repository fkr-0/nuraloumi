# SL101 GPU video bundle consumer (source-verified, not hardware-qualified)

The ordinary `sl101-musl-armv7.yml` CI tarball/APK remains **software-only**.
An explicit `workflow_dispatch` may additionally produce a separately named
`nuraloumi-gpu-source-verified-armv7-*` tarball. No browser engine or secondary
player is included. CI must not use host-local private binaries.

## Independent producer contract v1

The `sl101_linux` producer publishes a GitHub Actions artifact named
`sl101-gpu-video-bundle`, containing exactly `video-bundle.tar.gz`.
The archive has a flat root with `provenance.json`, `lib/*.so*`, and
`gst/*.so*` regular files; no symlinks, devices, or directory traversal.
`provenance.json` is UTF-8 JSON with:

- `schema`: `sl101-gpu-video-bundle-v1`
- `project`: `sl101_linux`
- `git_revision`: the full 40-character commit of the reproducible producer
- `target`: `armv7-unknown-linux-musleabihf`
- `abi`: `armv7-musleabihf-vfpv3-d16-no-neon-no-idiv`
- `files`: object mapping **every** ELF path to its SHA256 hex digest

The producer must include one `lib/libnuraloumi-video.so`, one
`gst/libgstv4l2codecs.so`, Grate Gallium, EGL/GLES, GBM, libdrm and the
complete private transitive GStreamer/ELF dependency closure. This is a
consumer proposal until the independent 0092 producer adopts exactly this
layout and publishes an immutable, verified output. Version mismatches fail.

## CI invocation

Configure `SL101_GPU_PRODUCER_REPOSITORY` as the trusted repository variable
and `SL101_GPU_PRODUCER_READ_TOKEN` as a read-only Actions-artifact secret.
Provide three explicit dispatch inputs:

1. `gpu_producer_run_id`: the immutable producer workflow run number/id
2. `gpu_producer_revision`: full commit digest from the reviewed source tree
3. `gpu_archive_sha256`: SHA256 of the producer's `video-bundle.tar.gz`

Blank `gpu_producer_run_id` selects software-only output. Nonblank producer
selection with missing metadata, inaccessible artifact, mismatched archive SHA,
wrong source/ABI, unlisted files, missing plugins, duplicate SONAME, unresolved
ELF dependencies, or forbidden instructions fails the GPU variant without a
fallback. The archive SHA is checked before any extraction. The packager
repeats validation before copying the private libraries. `provenance.json` is
retained inside the GPU package manifest with the file hashes. The consumer
copies the **entire** verified `lib/` and `gst/` trees, including additional
transitive GStreamer modules (not only `libgstv4l2codecs.so`). After packing,
the GPU CI job uses `scripts/verify_packaged_sl101_video.py` against the
actual release tarball: it checks the NuraLoumi Git SHA, exact version, ABI,
producer Git revision, complete file manifest, original per-module producer
hashes and the absence of dropped or injected private libraries. A passing
check remains a host-only provenance result, not VDE or Sway hardware proof.

The same check may be repeated **offline** on the original untampered GPU tar
with exact reviewed pins:

    python3 scripts/verify_packaged_sl101_video.py nuraloumi-gpu-ci.tar \
      --source-sha <NURALOUMI_MAIN_COMMIT> \
      --expected-version <NURALOUMI_VERSION> \
      --producer-revision <SL101_LINUX_COMMIT>

It cannot validate GitHub run identity by itself; independently verify the
successful official main-branch workflow run and artifact digest first.

The check is a static ABI/source gate; it does **not** establish that VDE,
DMA-BUF, Wayland compositor, WPE, or audio work on physical SL101 hardware.
The legacy browser remains separately gated.

## Guarded .106 staging / rollback (operator-run only)

1. Retrieve only the official GPU variant and its `GPU-SHA256SUMS.txt`;
   verify its full artifact name, CI run SHA, tar SHA, package manifest and
   embedded producer provenance against the reviewed inputs.
2. Check the reported target and all binary hashes again on a staging host;
   **never** use an anonymous `target/` or host-local private library.
3. Preserve the existing `.106` binaries, libraries and configuration in an
   independently hash-recorded rollback archive before any replacement.
4. Stage in a separate versioned directory; test library/plugin loading and
   the existing one `nuraloumi-player`/`playerd` invocation without replacing
   the active session. Do not deploy the browser from this video artifact.
5. Require physical-device 414-frame/VDE/audio, no-SIGILL, compositor and
   launcher qualification plus human sign-off before activation.
6. On failure, restore the previous hash-verified tree and restart only the
   specifically affected user services using the normal guarded procedure.

No live-device operation, remote push or release activation is authorized by
this CI contract.
