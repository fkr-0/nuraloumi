# NuraBrowser GitHub ARMv7 producer: private musl loader preflight

Date: 2026-11-25. This is a **read-only build-host preflight**, not a
qualification receipt, a device exercise or deployment authorization.

## Exact producer and independently recovered input

- GitHub native ARMv7 WPE producer run: `45595341644`, source commit
  `844dc30c7885027d06ef3940dcf402607c981720`.
- The *complete* local `nurabrowser-wpe-musl-candidate.tar` has SHA-256
  `8444a91b6d92b5f08589898b9cb8d0f0fd563ad0f225a2bfd87569c43e37fbcd`.
  Its `CANDIDATE-ONLY.txt` correctly declares `runtime_qualification=UNQUALIFIED`
  and `device_deployment=FORBIDDEN`.
- A second download location held only a 5.5-MiB truncated tar and failed
  `tar -tf` with `Unexpected EOF`; it was **not** used as the input.
- The previously assembled Alpine 3.22 `runtime-root-graphics` contains
  WPE WebKit 2.54.2 engine/helpers, the musl interpreter, ICU 76 libraries,
  `bwrap`, `xdg-dbus-proxy` and the selected software-graphics libraries.
  Its assembly is not independently reproducible yet, and it has no
  `runtime.json`. This preflight does **not** validate the root as a complete
  artifact or assert the original producer performed that assembly.

## Reproducible isolated probe

The new `scripts/probe_nurabrowser_alpine_loader.py` requires the producer's
marker and matching tar hash. It launches the three executables' musl loader
with `--list` from *inside* a private rootfs chroot, using a read-only bind
mount and a network-disabled ARMv7 container. The container has no access to
the host filesystem through its loader paths, no network and no writable
rootfs; it gets only `CAP_SYS_CHROOT`. Resource caps are 0.5 CPU, 768 MiB RAM,
80 PIDs and 45 seconds per process. A loader-reported library must resolve
to a regular file inside the provided root, or the probe rejects it.

    python3 scripts/probe_nurabrowser_alpine_loader.py \
        "$ROOTFS" "$PRODUCER_DIR" --output "$EVIDENCE_JSON"

Observed on the x86_64 build host with ARMv7 userspace emulation:

| Process | Private loader paths | Result |
| --- | ---: | --- |
| `nurabrowser-core` | 53 | `--list` succeeded |
| `WPEWebProcess` | 52 | `--list` succeeded |
| `WPENetworkProcess` | 52 | `--list` succeeded |

The core binary also ran `--help` successfully from within the same chroot,
printing the genuine WPE 2.54+ CLI (`--url`, `--platform`, `--no-control`,
`--exit-after-ms`). This proves a bounded private interpreter load and
startup, **not** WebKit rendering, sandbox admission or actual ARMv7 execution
on Nura.

Docker image ID:
`sha256:79f8c66e5f43fb3202701da08f2fcd6e0c28bf9a237da9cddef3fa2cb290fc54`.
Raw JSON evidence SHA-256:
`46cf4869de761068242b7e16909ab916888bb24125a878c316288969837707e7`.
The JSON is held outside the repository; the script can regenerate its fields
using the preserved input artifacts and image. Execution-address lines from
the loader may vary, so byte-for-byte JSON identity is not a release gate.

## Hard failure: D16/NEON

The probe exited **1** with
`WPE_LOADER_PREFLIGHT=BLOCKED processes=3 crypto_issues=6`.

| Private library | Evidence | Decision |
| --- | --- | --- |
| `libcrypto.so.3` | VFPv3, `NEONv1` | Not VFPv3-D16-only |
| `libnettle.so.8` | VFPv3, `NEONv1` | Not VFPv3-D16-only |
| `libgcrypt.so.20` | ARMv8/NEON attributes | Not VFPv3-D16-only |

These findings block deployment to Tegra20, which has no NEON. The older,
separate source-pinned D16 overlay documented in
`NURABROWSER-LOCAL-MUSL-CANDIDATE-20261125.md` is **not** interchangeable
with this producer source commit. Do not rename or relabel either candidate.

The graphics root contains `libLLVM.so.20.1` and a `swrast_dri.so` alias
to `libdril_dri.so`; the target's previously observed LLVM version is 23.1.
Neither LLVM/DRI module was loaded by these three `--list` probes. The
potential `dlopen` selection and Mesa/LLVM compatibility remain unresolved.

## Required next gates

1. Recover or rebuild the exact-source Cortex-A9/VFPv3-D16 crypto overlay,
   including security patches and five paired SONAMEs plus the OpenSSL legacy
   module; verify each source and object digest.
2. Reassemble an immutable private root and repeat loader checks with D16
   replacements. Audit all transitive plugins, GPU/LLVM/DRM and dynamic `dlopen`
   paths; do not infer the load graph from `DT_NEEDED` alone.
3. Resolve mixed ARM/Thumb ISA decoding and independently qualify
   Web-/NetworkProcess namespaces, sockets, D-Bus and Seccomp negative tests.
4. Only then run a supervised Nura Wayland navigation/touch/exit and soak
   qualification with rollback. Build a same-source reviewed `runtime.json`,
   package and install the real `NuraBrowser` entry. A successful GitHub
   producer job alone is not a qualified release.

The current Nura shell, `NuraBrowser (Canary)`, compositor and WayVNC were
left unchanged. No source publishing, release dispatch or installation occurred.
