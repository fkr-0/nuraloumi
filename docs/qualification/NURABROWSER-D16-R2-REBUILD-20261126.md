# WPE ARMv7/musl D16 crypto rebuild, Alpine 3.22 OpenSSL r2

Date: 2026-11-26. Status: **BUILD IN PROGRESS / UNQUALIFIED**.

## Original producer stays pinned

- WPE 2.54.2 native ARMv7 producer: GitHub run `45595341644`, source
  `844dc30c7885027d06ef3940dcf402607c981720`, tar SHA256
  `8444a91b6d92b5f08589898b9cb8d0f0fd563ad0f225a2bfd87569c43e37fbcd`.
- A fresh read-only archive comparison proved exact binary hashes for
  `nurabrowser-core`, `WPEWebProcess`, `WPENetworkProcess`, and
  `libWPEWebKit-2.0.so.1.11.5` in the existing Alpine root:
  `WPE_PRODUCER_BYTES=PASS 4`.
- This provenance does not make the original Alpine root reproducible or
  qualify it for Nura. See `NURABROWSER-GITHUB-ARMV7-LOADER-PREFLIGHT-20261125.md`.

## Security-current source inputs

The previously pinned OpenSSL 3.5.9-r1 rebuild is outdated on the 3.22-stable
branch. Source pin now follows **OpenSSL 3.5.9-r2**, including its two added
patches `CVE-2026-107493.patch` and `CVE-2026-124709.patch`, while preserving
all five r1 security patches and the Alpine `auxv.patch`. No patch was omitted
merely to make compilation pass.

- Alpine aports Git revision:
  `c1a97dc220098b43d75d03ed5776a948908abfaf`.
- `SOURCE-MANIFEST.json` SHA256:
  `58524fc081cadb3b1d3d807d7bf369620b0c9965839a5a3df4c1bdcfeb27e416`.
- The source fetcher validated **12/12** vendor SHA-512 pins (OpenSSL 3.5.9,
  Nettle 3.10.2, libgcrypt 1.10.3 and nine Alpine patch inputs). It refuses
  preexisting destinations, oversized or hash-mismatched downloads.
- Source/builder consistency and synthetic failure tests: **10/10 pass**.

Independent build environment: Linux ARMv7/musl Alpine 3.22 Docker image
`sha256:cd4d0a952d38d42d376bcbf178818ed567b4640f60e6049b8dcffbc406ae433f`
with GCC 14.2, GMP and libgpg-error headers; this is ARMv7 userspace under
emulation on an x86_64 host, not an ARMv7-native GitHub runner. Host-side
`BUILD-TOOLCHAIN.txt` SHA256 at build start:
`363586a17dcddeab66a090cc3d84df839622541bf9ddf2c12fabc5774ea973f9`.

## Reproducible commands (no tablet access)

The repo scripts perform all URL selection and checksum checks. Run with a
fresh output under a scratch root; no existing output may be reused:

    python3 scripts/fetch_nurabrowser_wpe_d16_sources.py "$SCRATCH/sources"

    docker build --platform linux/arm/v7 --pull=false \
      -f scripts/docker/nurabrowser-d16-crypto.Dockerfile \
      -t nuraloumi-crypto-d16:alpine322-r2 scripts/docker

    docker run --rm --pull=never --platform linux/arm/v7 \
      --network none --read-only --cap-drop ALL \
      --security-opt no-new-privileges --memory 3g --cpus 2 \
      --pids-limit 256 --user 1000:1000 --tmpfs /tmp:rw,size=256m \
      --mount type=bind,src="$PWD",dst=/work,readonly \
      --mount type=bind,src="$SCRATCH/sources",dst=/inputs,readonly \
      --mount type=bind,src="$SCRATCH/overlay",dst=/out \
      nuraloumi-crypto-d16:alpine322-r2 \
      sh /work/scripts/rebuild_nurabrowser_wpe_d16_crypto.sh /inputs /out

The pre-existing source tar and Alpine root are not modified. The build
process writes compiler logs into the new overlay directory and, if it passes,
five replacement SONAMEs, the OpenSSL legacy module, a SHA256 inventory,
source manifest, toolchain ledger and `CANDIDATE-ONLY.txt`.

## Next qualification steps (not yet PASS)

1. Finish Nettle/Hogweed, libgcrypt, and OpenSSL r2 compilation. Validate the
   **actual** five SONAMEs plus legacy module, source provenance and decoded
   D16 ISA, with no NEON or high-register instructions.
2. Use `scripts/assemble_nurabrowser_alpine_d16_root.py` to copy only into a
   nonexisting root. It independently matches the four WPE producer bytes,
   verifies every crypto replacement SHA256 and preserves existing SONAME
   aliases while replacing their real target files.
3. Rerun the chroot-private musl loader probe for all three WPE processes.
   Test separate GIO/graphics/plugins, `dlopen` and Mesa/LLVM provenance.
4. Independently prove WebProcess and NetworkProcess Seccomp, mount, socket,
   D-Bus negative controls on target; only then supervised pixels/navigation,
   touch, 30-minute soak, rollback and reviewed `runtime.json`.

No production files, Canary launcher, compositor, tablet runtime or GitHub
release have been modified as part of this source/build preparation.
