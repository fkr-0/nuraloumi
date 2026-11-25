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
- Source/builder consistency and synthetic failure tests: **13/13 pass**.
- Independent host-side application of the eight Alpine OpenSSL patches to a
  fresh OpenSSL 3.5.9 source tree: **8/8 pass** with no fuzz, offsets or rejects.

Independent build environment: Linux ARMv7/musl Alpine 3.22 Docker image
`sha256:cd4d0a952d38d42d376bcbf178818ed567b4640f60e6049b8dcffbc406ae433f`
with GCC 14.2, GMP and libgpg-error headers; this is ARMv7 userspace under
emulation on an x86_64 host, not an ARMv7-native GitHub runner. Host-side
`BUILD-TOOLCHAIN.txt` SHA256 at build start:
`363586a17dcddeab66a090cc3d84df839622541bf9ddf2c12fabc5774ea973f9`.

Observed first offline build (diagnostic, not a release): Nettle and Hogweed
linked and installed correctly with `Tag_FP_arch: VFPv3-D16` and
`Tag_ABI_VFP_args: VFP registers`, without a NEON ELF attribute. The older
libgcrypt `config.guess` in this first run reported
`armv7l-unknown-linux-gnueabihf` and warned that Alpine's musl
`libgpg-error` was built for a different triplet, even though the actual GCC
compilation used the proper ARMv7/musl target. The committed build script now
passes the GCC target as both `--build` and `--host` for libgcrypt. **A new
canonical build with this correction is required**; do not promote the
first-run output from a diagnostically mismatched configuration.
The independent, corrected canonical run has since rebuilt and staged
Nettle/Hogweed and reached libgcrypt configure with both Build and Host
reported as `armv7-alpine-linux-musleabihf`, `cross compiling... no`, and
`GPG Error >= 1.27... yes (1.55)` **without the prior mismatch warning**.
At this writing, it is still compiling; neither container has produced a
qualified final overlay.

Static decoded-ISA diagnostic: three linked first-run crypto libraries
(`libnettle.so.8`, `libhogweed.so.6`, `libgcrypt.so.20`) were copied into an
independent flat inspection directory and passed
`scripts/audit_nurabrowser_decoded_isa.py`: **238,947 decoded instructions,
0 forbidden uses, 0 unknown decodes and 0 decoder errors**. `libgcrypt` also
had SONAME `libgcrypt.so.20` and `DT_NEEDED` on
`libc.musl-armv7.so.1`/`libgpg-error.so.0`. This does **not** supersede the
required repeat against the corrected canonical build and entire WebKit root.

Additional graphics-plugin preflight (original root, not replacements):
`libEGL.so.1`, `libgbm.so.1` and `dri/libdril_dri.so` report
VFPv3-D16/hardfloat ELF attributes. **`libgallium-25.1.9.so` instead reports
`Tag_FP_arch: VFPv3` and `Tag_Advanced_SIMD_arch: NEONv1`**, a definite
incompatibility with Tegra20 if loaded. The instruction decoder yielded
**1,082,779 unknown decodes** and
**89,990 flagged uses across the four libraries** (3,948,507 decoded
instructions). The stripped ARM/Thumb mapping can produce false
disassemblies and false NEON positives, so these instruction counts cannot
classify `libEGL`, `libgbm` and the DRI shim. The `libgallium` NEON **ELF
attribute** is an independent hard failure, not based on the decoder.
Prove that the runtime does not load Gallium, or replace that graphics stack
with a D16-qualified build; `dlopen`/Mesa/LLVM and correct-mode ISA evidence
remain mandatory.

Whole-root `readelf -A` scan of all regular files found exactly six
NEON-attributed objects in the original Alpine root: `usr/lib/libcrypto.so.3`,
`usr/lib/libnettle.so.8.11`, `usr/lib/libgcrypt.so.20.4.3`,
`usr/lib/ossl-modules/legacy.so`, `usr/lib/libgallium-25.1.9.so` and
`usr/lib/gallium-pipe/pipe_kmsro.so`. The source-pinned crypto overlay replaces
the first four as part of the five-library-plus-module ABI set; **both
graphics objects remain NEON-attributed** until they are proved unreachable
by runtime `dlopen` or rebuilt for D16. The audit detects attributes, not
every possible machine instruction, and is not a sandbox/pixel proof.

The graphics blocker is not merely a speculative module lookup:
`readelf -d` shows `libEGL.so.1.0.0` has a direct `DT_NEEDED` edge to
`libgallium-25.1.9.so`. The GLESv1/GLESv2 libraries also embed that
dependency name. Therefore **any runtime loading this packaged EGL stack
necessarily loads the NEON Gallium object**. A D16-qualified Mesa EGL/Gallium
replacement or a proved EGL-free rendering backend is required; removing
only `pipe_kmsro.so` cannot qualify the current EGL closure.
The fresh-root assembler now independently writes all remaining
`blocking_neon_elf_attributes` into `candidate/PROVENANCE.json`, with a
synthetic negative-control test. Its report is evidence, **never** a promotion
gate or claim that other decoded/JIT instructions are safe.

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
