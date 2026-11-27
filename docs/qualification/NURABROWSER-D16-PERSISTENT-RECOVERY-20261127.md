# NuraBrowser D16 persistent recovery (2026-11-27)

**Later update:** SSH authentication was restored and the complete 6-Crypto + 8-Graphics ARMv7/musl root passed the host-only loader and 16/16 RTLD_NOW probes. See `docs/qualification/NURABROWSER-D16-FULL-CLOSURE-20261127.md` for the latest state. The historical SSH blocker below is retained as incident evidence.

This is a build-host recovery record, **not** a release approval. The exact GitHub WPE producer is still marked `UNQUALIFIED` and `device_deployment=FORBIDDEN`. Do not install this host candidate on Nura.

## State layout

`local-state/` is anchored at the **nuraloumi repository root**, ignored by Git. It must remain on a persistent filesystem; no source tree, compiler object, runtime root, or evidence record should live only in `/tmp`.

- `local-state/imported-dotlocal/share/nuraloumi/recovery-20261125/`: two historical audit archives plus `preservation-branches.bundle`. The three SHA-256 values were checked before/after moving.
- `local-state/imported-dotlocal/state/nura-browser/`: navigation state.
- `local-state/imported-dotlocal/share/nurabrowser-core/`: per-browser storage/media-key salts.
- The original locations under `~/.local` are **symlinks** to these imported paths so existing XDG consumers keep resolving them.
- `~/.local/share/nuraloumi/keys/` (private release signing material) was **not moved**, copied, or committed. Unrelated contents of `~/.local` were not changed.
- `local-state/build/nurabrowser-d16/sources/`: twelve independently SHA-512-verified OpenSSL 3.5.9-r2, Nettle 3.10.2, libgcrypt 1.10.3 source/patch inputs. `SOURCE-MANIFEST.json` SHA-256: `58524fc081cadb3b1d3d807d7bf369620b0c9965839a5a3df4c1bdcfeb27e416`.
- `local-state/build/nurabrowser-d16/producer/`: exact GitHub ARMv7 WPE 2.54.2 artifact from run `45595341644`, source `844dc30c7885027d06ef3940dcf402607c981720`; producer tar SHA-256 `8444a91b6d92b5f08589898b9cb8d0f0fd563ad0f225a2bfd87569c43e37fbcd`. The tar is complete and the Alpine image contains byte-identical copies of four required WPE objects.
- `local-state/build/nurabrowser-d16/alpine-root/`: an extracted standalone copy of Docker image `nuraloumi-wpe-runtime:45595341644-graphics`, not a live root or a deployment package.
- `local-state/build/nurabrowser-d16/overlay/`: **completed** source-pinned ARMv7/musl VFPv3-D16 crypto rebuild (source, logs, stages and replacements), Docker exit 0. Previous interrupted 2-core attempt was preserved as `overlay-pre20cores/`.

## Build and verification

Inspect the detached named container:

```sh
docker ps -a --filter name=nura-d16-r2-persistent
docker logs --tail 30 nura-d16-r2-persistent
ls -lh local-state/build/nurabrowser-d16/overlay/logs
df -h .
```

Started with:

```sh
python3 scripts/fetch_nurabrowser_wpe_d16_sources.py local-state/build/nurabrowser-d16/sources
sh scripts/launch_nurabrowser_d16_persistent.sh --preflight
sh scripts/launch_nurabrowser_d16_persistent.sh --start
```

The launcher uses an existing Alpine ARMv7 builder image without pulling, no network, read-only container root, non-root UID, no Linux capabilities, `NoNewPrivileges`, source mounts read-only, and persistent output bind. Nettle, libgcrypt and patched OpenSSL use **20 make jobs**, Docker `--cpus 20` and a 16 GiB memory cap. `BUILD-TOOLCHAIN.txt` records `build_jobs=20`.

**Restart semantics:** the detached container survives a ChatGPT/browser disconnect. After a Docker/host reboot the `overlay/` bytes and logs persist, but the current source script intentionally refuses to overwrite a pre-existing `work/`. **Automatic continuation of an interrupted compilation is not implemented.** Inspect and explicitly arrange a verified resume/rebuild instead of deleting or silently overwriting `overlay/`.

The following commands were executed after the builder exited successfully and produced `overlay/CANDIDATE-ONLY.txt`. Both output paths now exist; for a fresh rerun, choose **new** output filenames rather than overwriting them:

```sh
python3 scripts/assemble_nurabrowser_alpine_d16_root.py \
  local-state/build/nurabrowser-d16/alpine-root \
  local-state/build/nurabrowser-d16/producer \
  local-state/build/nurabrowser-d16/overlay \
  local-state/build/nurabrowser-d16/assembled-no-graphics
python3 scripts/probe_nurabrowser_alpine_loader.py \
  local-state/build/nurabrowser-d16/assembled-no-graphics \
  local-state/build/nurabrowser-d16/producer \
  --output local-state/build/nurabrowser-d16/assembled-loader-preflight.json
```

This is **host-only** loader preflight, not a native GPU/WebKit/Seccomp PASS. The recovered *base* Alpine root loads all three WPE process ELF dependencies, but had six NEON/non-D16 crypto attribute failures before replacement; evidence: `local-state/build/nurabrowser-d16/base-loader-preflight.json`.

## Observed host-only results (2026-11-27)

- Docker `nura-d16-r2-persistent`: **exited 0, OOMKilled=false**. Input archive/patch hashes were verified. The `openssl-3.5.9-r2` patchset was applied; `build_jobs=20`, image `nuraloumi-crypto-d16:alpine322-r2`.
- `overlay/replacements/` has exactly **six** objects: `libcrypto.so.3`, `libssl.so.3`, `libnettle.so.8`, `libhogweed.so.6`, `libgcrypt.so.20`, `ossl-modules/legacy.so`. `sha256sum -c ../REPLACEMENTS-SHA256SUMS.txt` returned **6/6 OK**. Every one has `Tag_FP_arch: VFPv3-D16` and `Tag_ABI_VFP_args: VFP registers`, with **no NEON/Advanced_SIMD attribute**.
- `assembled-no-graphics/` was created as a *fresh* private root. The assembler reported `replacements=6 graphics=0`, preserving all four original WPE producer bytes. Its `candidate/ROOTFS-SHA256SUMS.txt` validated without error. `candidate/PROVENANCE.json` records precisely two remaining NEON-tagged objects: `usr/lib/gallium-pipe/pipe_kmsro.so` and `usr/lib/libgallium-25.1.9.so`. They belong to the **old**, not-yet-replaced graphics stack.
- `assembled-loader-preflight.json`: isolated musl `--list` succeeded for **3/3** WPE executables (`nurabrowser-core`: 53 paths; `WPEWebProcess`: 52; `WPENetworkProcess`: 52), with **0 crypto ISA issues** and status `LOADER_PREFLIGHT_ONLY`. The genuine `nurabrowser-core --help` command also exited 0 inside network-disabled, read-only ARMv7 chroot.
- Rebuilt `scripts/fixtures/nurabrowser_d16_dlopen_probe.c` as ARMv7 D16-hardfloat (`local-state/build/nurabrowser-d16/dlopen-probe/d16-dlopen-probe`, SHA-256 `d02bd5ff4e1cf6d1afa34c274dad622073409e05ce326b5f79a9e1de35916cf2`). On a **separate copy** `assembled-rtldnow-test/` with only this probe installed, a read-only, networkless, chroot-isolated `RTLD_NOW|RTLD_LOCAL` cycle loaded and closed **8/8**: the five crypto libraries, OpenSSL legacy provider, and both GIO proxy modules. A nonexistent SONAME failed as expected with exit **1** (`No such file or directory`). No original root was modified for this test.
- These facts prove source/ABI, direct loader and selected `dlopen` preflights only. No TLS handshake, GPU/Wayland rendering, WPE subprocess sandbox, Seccomp negative control or device soak has passed.

## Qualification blockers recorded before SSH restoration (historical)

1. Fetch the exact eight SHA-256-pinned Nura Mesa **26.2.4**, LLVM **23.1** and Wayland **0.26** objects through the user-managed SSH alias `sl101`, then run the assembler with `--graphics-overlay`. On 2026-11-27 the read-only `scripts/fetch_nurabrowser_wpe_d16_graphics.sh` transfer failed at SSH authentication (`Permission denied`). Do not replace these objects with the older Mesa/LLVM 20 files from the recovered Docker image.
2. Extend the passing GIO/Crypto `RTLD_NOW` checks to the **entire Mesa/Wayland/plugin dlopen closure** after acquiring pinned graphics; test Web-/NetworkProcess namespaces, D-Bus/socket policy and Seccomp denial rules.
3. Only after independent review, qualify Wayland pixels, TLS, navigation, touch and clean exit on the actual SL101. **Do not create `runtime.json` or activate the browser from the host-only evidence.**

For original provenance and prior failures see `docs/qualification/NURABROWSER-GITHUB-ARMV7-LOADER-PREFLIGHT-20261125.md` and the D16 source/build scripts.
