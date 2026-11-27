# NuraBrowser D16 persistent recovery (2026-11-27)

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
- `local-state/build/nurabrowser-d16/overlay/`: active source-pinned ARMv7/musl VFPv3-D16 crypto rebuild (source, logs, stages and replacements). Previous interrupted 2-core attempt was preserved as `overlay-pre20cores/`.

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

Once the builder has **exited successfully** and produced `overlay/CANDIDATE-ONLY.txt`:

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

## Remaining qualification blockers

1. Fetch the exact eight SHA-256-pinned Nura Mesa **26.2.4**, LLVM **23.1** and Wayland **0.26** objects through the user-managed SSH alias `sl101`, then run the assembler with `--graphics-overlay`. On 2026-11-27 the read-only `scripts/fetch_nurabrowser_wpe_d16_graphics.sh` transfer failed at SSH authentication (`Permission denied`). Do not replace these objects with the older Mesa/LLVM 20 files from the recovered Docker image.
2. Explicitly load plugins/dlopen closure under `RTLD_NOW`, prove musl loader paths and D16 ISA for each replacement, and test Web-/NetworkProcess namespace, D-Bus/socket and Seccomp denial rules.
3. Only after independent review, qualify Wayland pixels, TLS, navigation, touch and clean exit on the actual SL101. **Do not create `runtime.json` or activate the browser from the host-only evidence.**

For original provenance and prior failures see `docs/qualification/NURABROWSER-GITHUB-ARMV7-LOADER-PREFLIGHT-20261125.md` and the D16 source/build scripts.
