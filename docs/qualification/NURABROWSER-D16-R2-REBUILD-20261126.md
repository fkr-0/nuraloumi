# WPE ARMv7/musl D16 crypto rebuild, Alpine 3.22 OpenSSL r2

Date: 2026-11-26. Status: **GRAPHICS LOADER/DLOPEN PREFLIGHT PASS; CRYPTO BUILD IN PROGRESS; RUNTIME UNQUALIFIED**.

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
The fresh-root assembler independently writes all remaining
`blocking_neon_elf_attributes` into `candidate/PROVENANCE.json`, with a
synthetic negative-control test. Its report is evidence, **never** a promotion
gate or claim that other decoded/JIT instructions are safe.

## Mesa 26.2.4 / LLVM 23 / Wayland 0.26.0 private closure qualification

A private Mesa 26.2.4 software-rendering D16 build on Nura was the
historically successful **nested** Wayland WPE pixel candidate, not the
production display release. Eight exact ELF objects were transferred read-only
from 192.168.23.106 into **host-private scratch staging**. Their SHA256 pins
are now explicit in `GRAPHICS_FROM_NURA` in the assembler:

- Private `mesa-d16-gnu-tls-candidate-20261124`: `libEGL.so.1.0.0`,
  `libGLESv2.so.2.0.0`, `libgbm.so.1.0.0`,
  `libgallium-26.2.4.so` (Gallium SHA256
  `09e03bea18be4120a6d22dde9887e4a452dcd488df6933c5cf46a4c15ec8e87b`).
- Existing Nura `/usr/lib`: `libLLVM.so.23.1` (SHA256
  `99e1add0a47799197eef0d5a70c714310d2301687d69670525f51c19976ac255`),
  `libGLESv1_CM.so.1.1.0`, `dri/libdril_dri.so`, and
  `libwayland-client.so.0.26.0` (SHA256
  `4b8fa24d5b31b3be7444f3683947f71f8d68823ea509184dd5e4958429f6c3a3`).

**8/8** SHA256 checks and D16/hardfloat `readelf -A` checks PASS;
all report VFPv3-D16, hardfloat, no NEON ELF attribute. The eight-file
graphic-overlay option in `scripts/assemble_nurabrowser_alpine_d16_root.py`
rejects missing, tampered or NEON-attributed sources **before** copying the
root. It preserves four Mesa SONAME aliases; changes the Wayland client alias
from old Alpine `0.23.1` to Nura `0.26.0` under its truthful versioned
filename; installs matching Gallium26, LLVM23 and the DRI shim; and removes
only in a **fresh copied root** the Mesa25 Gallium, incompatible
`gallium-pipe` and old `gbm/dri_gbm.so` plugins. The WPE producer
executables and original Alpine base root remain immutable.

Actual copied-root results (not a mock): graphics overlay **8/8 installed**;
all regular ELF objects rescanned; the original two NEON graphics objects
are gone, leaving precisely four original crypto blockers
(`libcrypto.so.3`, `libnettle.so.8.11`, `libgcrypt.so.20.4.3`,
`ossl-modules/legacy.so`). There is no remaining binary reference to
`libgallium-25.1.9.so` in the copied root.

The first private musl loader test exposed two missing Wayland symbols
(`wl_display_dispatch_queue_timeout` and `wl_fixes_interface`) because
Mesa26 EGL expects Nura's newer Wayland. The Alpine 3.22 client
`libwayland-client.so.0.23.1` exports neither; target
`libwayland-client.so.0.26.0` exports both. After the pinned replacement,
`/lib/ld-musl-armhf.so.1 --list /usr/lib/libEGL.so.1` resolved EGL,
Gallium26, LLVM23 and their transitive closure with **no missing symbol**.

Stricter **actual ARMv7/musl `dlopen(RTLD_NOW|RTLD_LOCAL)` under QEMU,
isolated chroot, network disabled, read-only mount**:
`scripts/fixtures/nurabrowser_d16_dlopen_probe.c` compiled with
`-mcpu=cortex-a9 -mfpu=vfpv3-d16 -mfloat-abi=hard` (probe SHA256
`f6f8fef994e94fd19de9f74639c920c11258b821f00bffd36f2dcbee428d1772`).
EGL, GLESv1, GLESv2, GBM, Gallium26, LLVM23, Wayland-client and DRI
**8/8 DLOPEN_PASS**, exit 0. A/B negative controls:
- same private Mesa26 root **without** new Wayland: EGL `DLOPEN_FAIL`
  for missing `wl_display_dispatch_queue_timeout`, exit 1;
- upgraded root with a nonexistent SONAME: `DLOPEN_FAIL`
  `No such file or directory`, exit 1.

The three original WPE native binaries also all pass private-loader
dependency listing in the Mesa26/Wayland26 root, but the overall probe reports
`BLOCKED`: exactly six old-crypto D16/NEON attribute issues across
libcrypto, libnettle and libgcrypt; the matching D16 r2 crypto compilation is
still ongoing. A standalone `dlopen` of `libWPEWebKit-2.0.so.1.11.5`
hit a musl `initial-exec TLS resolves to dynamic definition` relocation
failure. That test is **not equivalent** to loading WebKit as an executable's
initial `DT_NEEDED` dependency; process startup, TLS and WebKit plugins
remain explicitly unqualified.

These eight binary artifacts have recorded **device SHA provenance**, not
a complete independently rebuilt source recipe. D16 attributes and QEMU
`dlopen` are valuable preflights, not a proof of all ARM/Thumb/JIT opcodes,
GPU/Wayland pixels, seccomp isolation or production correctness. **Do not
install or promote on Nura** based on these results alone.

A separate **read-only live Canary process baseline** on Nura is recorded in
`NURABROWSER-LIVE-CANARY-SANDBOX-BASELINE-20261126.md`. It confirms the
existing WebProcess has one loaded Seccomp filter and isolated namespaces,
while the existing NetworkProcess has **Seccomp=0**, shares the UI namespaces,
and directly sees the session D-Bus address. It also records the present
`/run/sl101-desktop-user` sockets versus the desired conventional
`/run/user/10000` state. These are release blockers, not evidence that the
new combined root is running natively.

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

For the separate **binary-provenance** graphics overlay, with an SSH alias
`sl101` pointing to the authorized Nura device, the new guarded fetch helper
uses SCP read-only and refuses an existing destination:

    sh scripts/fetch_nurabrowser_wpe_d16_graphics.sh "$SCRATCH/graphics"

This transfers eight exact pinned ELF files and reruns the per-object SHA256
and D16/hardfloat preflight. The initial manual read-only transfers and
preflight succeeded; the new helper's syntax, existing-destination refusal,
and verifier were checked, but a second complete 139-MiB remote transfer was
not repeated. An independently reproducible **source build** of Mesa26/LLVM23
is still a separate qualification requirement. Once the canonical crypto
build succeeds, assemble the **combined** candidate only into a fresh output:

    python3 scripts/assemble_nurabrowser_alpine_d16_root.py \
      "$SCRATCH/runtime-root-graphics" "$SCRATCH/producer" \
      "$SCRATCH/overlay" "$SCRATCH/combined-root" \
      --graphics-overlay "$SCRATCH/graphics"

Replace the example producer/root paths with the independently verified WPE
artifact roots. The assembler refuses missing/tampered inputs, always marks
its output UNQUALIFIED, and never writes into any source tree.

## Direct WebKit executable startup under the original crypto root (2026-11-26)

The previous strict `RTLD_NOW` test of `libWPEWebKit` by late `dlopen`
reported an `initial-exec TLS resolves to dynamic definition` error.
That fixture is not equivalent to a normal executable launch. To distinguish
them, the exact pinned ARMv7/musl producer's `nurabrowser-core` was
executed with its normal dynamic dependencies in the private graphics-overlay
root, without any host network or Nura device process:

```sh
docker run --rm --pull never --platform linux/arm/v7 --network none \
  --read-only --cap-drop ALL --cap-add SYS_CHROOT \
  --security-opt no-new-privileges \
  --mount type=bind,src=/tmp/nura-mesa26-d16-qualification-YmOnBgzl/root-wayland26,dst=/private-root,readonly \
  nuraloumi-crypto-d16:alpine322-r2 sh -c \
  'chroot /private-root /usr/libexec/nuraloumi/browser/nurabrowser-core --help'
```

**PASS (exit 0):** WebKit's original native executable reached its own
GOption help output and identified `NuraBrowser * native WPE WebKit 2.54+`
with `--url`, `--platform`, `--no-control`, and `--exit-after-ms`
options. The full initial `DT_NEEDED` loader/TLS startup needed to reach
that code path succeeded, notwithstanding the deliberately different
late-`dlopen` TLS test. This is **not** a successful page render, network
TLS handshake, GIO plugin resolution or Web/Network auxiliary process start.

A separate network-disabled, tmpfs-state `--url=about:blank`,
headless bounded attempt did **not** complete in the intended
five-second headless window under ARMv7 linux-user QEMU; its transient
diagnostic container was stopped explicitly and no page-load PASS was
recorded. Do not infer native Nura behavior from this slow QEMU outcome.
Repeat navigation, plugin/TLS tests and bounded exit in the **combined**
D16 crypto+graphics candidate, then on a separately supervised device
canary.

## Direct plugin RTLD_NOW closure preflight (2026-11-26)

The existing isolated ARMv7/musl chroot contains a strict
`scripts/fixtures/nurabrowser_d16_dlopen_probe.c` binary that calls
`dlopen(RTLD_NOW | RTLD_LOCAL)` and `dlclose`, failing the command on
any error. A network-disabled, read-only ARMv7 QEMU Docker invocation
against the **original, not-yet-crypto-corrected** root
`/tmp/nura-mesa26-d16-qualification-YmOnBgzl/root-wayland26`
returned exit **0** for four separately loaded shared objects:

```text
DLOPEN_PASS /usr/lib/gio/modules/libgiognomeproxy.so
DLOPEN_PASS /usr/lib/gio/modules/libgiolibproxy.so
DLOPEN_PASS /usr/lib/libssl.so.3
DLOPEN_PASS /usr/lib/ossl-modules/legacy.so
```

These GIO module names were independently identified in the live
NetworkProcess maps. This checks their immediate **symbol-resolution
closure**, not GIO module activation, certificate validation, a
network TLS handshake, WebKit auxiliary-process IPC, or compatibility
of the old NEON-attributed libcrypto on Tegra20. Repeat all four tests
in the final combined D16 root after OpenSSL r2 completes.

## Read-only loaded plugin/TLS inventory from existing Nura Canary (2026-11-26)

The **already-running, unmodified** Nura canary was observed through
`/proc/PID/maps` and host `readelf -A`, not installed or restarted.
UI PID `14184` maps `libWPEWebKit-2.0.so.1`, ICU 76's three libraries,
`libsoup-3.0.so.0`, `libgio-2.0.so.0`, Mesa `libEGL.so.1` and
`libgallium-26.2.4.so`. WebProcess PID `14209` additionally maps
`libGLESv2.so.2` and the Gallium 26.2.4 driver. NetworkProcess
PID `14188` maps WebKit, Soup/GIO, ICU 76, **`libssl.so.3` and
`libcrypto.so.3`**, and both GIO modules
`modules/gio/libgiognomeproxy.so` and `libgiolibproxy.so`.

`readelf -A` on these **currently loaded** files gave:

| Object | FP attribute | NEON attribute | Disposition |
| --- | --- | --- | --- |
| Canary `libcrypto.so.3` | VFPv3 | **NEONv1** | **BLOCKED** for D16 compatibility; this is the old root, not the r2 candidate |
| Canary GIO `libgiognomeproxy.so` | VFPv3-D16 | none | Plugin attribute preflight only |
| Canary GIO `libgiolibproxy.so` | VFPv3-D16 | none | Plugin attribute preflight only |

The same live process-status check showed UI and NetworkProcess each
`NoNewPrivs=1, Seccomp=0, Seccomp_filters=0`, while WebProcess had
`NoNewPrivs=1, Seccomp=2, Seccomp_filters=1`. These observations prove
which runtime objects are mapped, **not** that a TLS handshake, HTTPS
navigation, D-Bus proxy policy, or the new complete D16 root passed.
The separate native ARMv7 sandbox fixture results are recorded in
`NURABROWSER-NETWORK-SANDBOX-NEGATIVE-20261125.md`.

## Next qualification steps (not yet PASS)

1. Finish Nettle/Hogweed, libgcrypt, and OpenSSL r2 compilation. Validate the
   **actual** five SONAMEs plus legacy module, source provenance and decoded
   D16 ISA, with no NEON or high-register instructions.
2. Use `scripts/assemble_nurabrowser_alpine_d16_root.py` with the optional
   `--graphics-overlay` containing the **eight pinned** Mesa/LLVM/Wayland
   files, into a nonexisting root. It matches all four WPE producer bytes,
   verifies crypto and graphics SHA256, and preserves/updates SONAME aliases.
3. Rerun the chroot-private musl loader probe for all three WPE processes,
   then the 8/8 strict graphics RTLD_NOW test in the **combined** root.
   Test GIO/graphics/dlopen plugins and executable-initial WebKit TLS;
   independently reproduce the Mesa26/LLVM23 source builds.
4. Independently prove WebProcess and NetworkProcess Seccomp, mount, socket,
   D-Bus negative controls on target; only then supervised pixels/navigation,
   touch, 30-minute soak, rollback and reviewed `runtime.json`.

No production files, Canary launcher, compositor, tablet runtime or GitHub
release have been modified as part of this source/build preparation.
