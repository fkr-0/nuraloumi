# NuraBrowser ARMv7/musl D16 combined closure — host-only result (2026-11-27)

**Decision: HOST LOADER / SELECTED DLOPEN PREFLIGHT PASS; DEVICE / RELEASE NO-GO.** This candidate is explicitly `UNQUALIFIED` and `device_deployment=FORBIDDEN`. No persistent SL101 services, packages, desktop entries or Wayland state were changed; the device only served files over authenticated read-only SCP.

## Recovered inputs and provenance

All recovered bytes are stored below the Git-ignored `local-state/build/nurabrowser-d16/` in this repository, not in `/tmp`.

| Input | Persistent location | Verified identity |
| --- | --- | --- |
| Exact WPE 2.54.2 producer | `producer/` | GitHub run `45595341644`, source `844dc30c7885027d06ef3940dcf402607c981720`, tar SHA-256 `8444a91b6d92b5f08589898b9cb8d0f0fd563ad0f225a2bfd87569c43e37fbcd` |
| Original Alpine root | `alpine-root/` | Docker `nuraloumi-wpe-runtime:45595341644-graphics`; all four producer WPE ELF bytes matched |
| D16 source-pinned Crypto | `overlay/` | Docker exit 0, no OOM, `build_jobs=20`; 6/6 replacement SHA-256 verified, all VFPv3-D16 / hardfloat, no NEON attribute |
| Nura Mesa26/LLVM23/Wayland26 | `graphics-20261127-ssh/` | `ssh sl101` authentication success, read-only `scripts/fetch_nurabrowser_wpe_d16_graphics.sh`: `NURA_MESA26_D16_STAGING=PASS 8`; exact eight object hashes and D16 attributes checked against `GRAPHICS_FROM_NURA` |

The graphics stage comprises `libEGL.so.1.0.0`, `libGLESv2.so.2.0.0`, `libgbm.so.1.0.0`, `libgallium-26.2.4.so`, `libLLVM.so.23.1`, `libdril_dri.so`, `libGLESv1_CM.so.1.1.0`, and `libwayland-client.so.0.26.0`. These are **hash-pinned device binaries**, not a new Mesa/LLVM source rebuild. The earlier SSH authentication failure is resolved for this read-only transfer.

## Assembly and tests

- `assembled-crypto-graphics/` is a **fresh, private root**, not a live device root. Command: `python3 scripts/assemble_nurabrowser_alpine_d16_root.py <alpine-root> <producer> <overlay> <new-output> --graphics-overlay <graphics-20261127-ssh>`, using the exact relative input paths above. Actual receipt: `D16_ALPINE_ASSEMBLY=CANDIDATE_ONLY ... replacements=6 graphics=8`, exit **0**.
- `assembled-crypto-graphics/candidate/PROVENANCE.json` records `blocking_neon_elf_attributes=[]`. The two previously identified NEON-tagged Mesa25 objects no longer block the inventory. Older unselected files may still exist in the root; this attribute check alone does not prove absence of every forbidden ARM opcode in stripped code.
- Full `candidate/ROOTFS-SHA256SUMS.txt` file inventory passed `sha256sum -c ... --quiet`, exit **0**.
- `assembled-crypto-graphics-loader.json` records `WPE_LOADER_PREFLIGHT=LOADER_PREFLIGHT_ONLY processes=3 crypto_issues=0`, exit **0**, against the exact producer tar in a network-disabled read-only ARMv7 chroot. Loader resolution counts: `nurabrowser-core` **53**, `WPEWebProcess` **52**, `WPENetworkProcess` **52**.
- The VFPv3-D16 `dlopen(RTLD_NOW|RTLD_LOCAL)` helper (`scripts/fixtures/nurabrowser_d16_dlopen_probe.c`) was installed **only in a separate cloned test root** `assembled-crypto-graphics-rtldnow-test/`. The immutable candidate was not modified.
- The isolated test container (`alpine:3.22`, ARMv7, no network, read-only root, dropped capabilities except `SYS_CHROOT`, `NoNewPrivileges`, 1 GiB RAM, 2 CPUs, 64 PIDs) completed **16/16 DLOPEN_PASS**, exit **0**, and called `dlclose` after each library:
  - GIO proxy modules: `libgiognomeproxy.so`, `libgiolibproxy.so`.
  - Crypto: `libcrypto.so.3`, `libssl.so.3`, `libnettle.so.8`, `libhogweed.so.6`, `libgcrypt.so.20`, `ossl-modules/legacy.so`.
  - Graphics: `libEGL.so.1`, `libGLESv2.so.2`, `libgbm.so.1`, `libgallium-26.2.4.so`, `libLLVM.so.23.1`, `libdril_dri.so`, `libGLESv1_CM.so.1`, `libwayland-client.so.0`.
- Negative control: `dlopen("/usr/lib/libdoesnotexist.so", RTLD_NOW)` emitted `DLOPEN_FAIL ... No such file or directory` and exited **1** as expected.

## Still mandatory before native activation

1. **Full ISA and source provenance:** the device Mesa26/LLVM23 objects are not independently source-rebuilt; conservative ELF attributes and a bounded subset of `dlopen` paths are not exhaustive full-opcode/plugin/JIT assurance.
2. **WebKit process isolation:** launch actual WPEWebProcess and WPENetworkProcess under the intended namespaces, bubblewrap/seccomp filter, targeted D-Bus proxy and mount/socket policy; prove denial controls and clean exit on device.
3. **End-to-end browser:** TLS/HTTPS, real page switch/navigation, Wayland pixels, touch, memory/temperature and 30-minute soak with crash recovery, supervised rollback and independent review.

There is **no `runtime.json`, no production activation and no release approval**. For prior crypto recovery, original build setup and earlier failure history see `docs/qualification/NURABROWSER-D16-PERSISTENT-RECOVERY-20261127.md`.
