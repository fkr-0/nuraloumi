# NuraBrowser 2.54.2 D16 — native Nura Bubblewrap, HTTPS, pixels and soak

Date: 2026-11-27 UTC. Target: owner-authorized `ssh sl101`, UID/GID 10000,
ARMv7 VFPv3-D16/musl. **Candidate-only qualification; NO-GO for release.**
No installed NuraLoumi service, compositor, WayVNC or `/opt/nuraloumi/current`
activation was modified. The active test device is also the productive tablet.

## Exact isolated runtime

- Host source: `local-state/build/nurabrowser-d16/assembled-crypto-graphics/`.
  WPE WebKit 2.54.2 producer run `45595341644`, git SHA
  `844dc30c7885027d06ef3940dcf402607c981720`; combined pinned
  OpenSSL 3.5.9-r2 / ICU 76 / Mesa and LLVM D16 closure, plus musl loader.
- Device staging only:
  `/home/user/.local/state/nura-sandbox-qualification-20261127/runtime-root`.
  Exactly 1,234 regular files, 770 symlinks, 248 directories. All regular
  file hashes pass the source `candidate/ROOTFS-SHA256SUMS.txt` via native
  BusyBox `sha256sum -cs` (exit 0). 1,481 source file/directory modes restored
  after extraction. `candidate/CANDIDATE-ONLY.txt` remains present;
  **no qualifying runtime.json was written**.
- Live launcher: `scripts/qualify_nurabrowser_nura_native.sh`.
  Bounded user-owned Bubblewrap; readonly private root and `/sys`,
  targeted readonly Nura `/etc/resolv.conf`, tmpfs `/run` and `/tmp`,
  private state/cache/config, native `/run/user/10000` (mode 0700),
  one named bind of the host compositor's `wayland-1` socket as
  `/run/user/10000/wayland-0`. No ambient desktop session bus,
  host `/home` or complete host `/run/sl101-desktop-user` bind.
  The helper WebKit subprocess uses its own nested Bubblewrap launcher.

## Real negative controls

Native ARMv7 build of `scripts/fixtures/nurabrowser_network_policy_probe.c`
launched inside a fresh Bubblewrap user/mount/PID/net/IPC/UTS namespace:

- `MOUNT_POLICY_PASS`: allowlisted file read; forbidden host file absent
  (`ENOENT`).
- `SOCKET_POLICY_PASS`: local Unix IPC and AF_INET socket creation work;
  forbidden desktop Wayland socket connect receives `ENOENT`.
- `SECCOMP_POLICY_PASS`: `NoNewPrivs=1`, `Seccomp=2`, filter count
  0→1; `getppid` is actively denied (`EACCES`), `getpid` still succeeds.
- The intentionally leaky mount control exposes `/etc/passwd`; the probe
  prints `POLICY_PROBE_FAIL forbidden-host-file-visible` and exits **1**.

The isolated fixture is **not a production NetworkProcess filter** and does
not prove the WebKit helper's specific forbidden-syscall behavior.

## Native live WPE and browser

Evidence directories on device:

- `runs/native-HHLGFC`: first 90-second live triple-process/Wayland
  probe. The internal container lacked DNS; the real Wayland error page
  read “Error resolving example.org”. Clean exit; production/Sway/WayVNC
  unchanged.
- `runs/native-CAgoiM`: second 90-second live probe with readonly
  `/etc/resolv.conf`. `https://example.org/` and
  `https://www.tagesschau.de/` loaded without WebKit `load-failed`.
  In-container `--remote uri/open/back/forward` returned `ok:true`.
  Compositor `grim` captured distinct 1280×800 page pixels; the
  `wayland-tagesschau-loaded.png` image visibly includes Tagesschau
  navigation and article content (SHA256
  `54da5e644f01df87782316d4c7d86842850b0cb6104beae5613fd541e1429505`).
  Early frames remained the initial page during loading; later frames
  repainted. Exit clean, no WPE processes left afterward; production,
  Sway and WayVNC PIDs unchanged.
- `runs/native-IAfDlF`: 40-second read-only sampling smoke:
  5/5 10-second process/RSS/thermal samples, one Example Domain tab,
  all three roles alive, initial aggregate RSS about 403 MiB and CPU 40°C.
  Clean exit and compositor unchanged.

Representative 2026-11-27 live `/proc` snapshot, second 90-second run:

| Role / PID | NNP | Seccomp/filters | User/mount/net namespace |
| --- | ---: | --- | --- |
| UI `nurabrowser-core` / 22463 | 1 | **0 / 0** | outer Bubblewrap |
| `WPENetworkProcess` / 22468 | 1 | **0 / 0** | **same as UI** |
| `WPEWebProcess` / 22474 | 1 | **2 / 1** | independent nested Bubblewrap |

Read-only live root visibility on the 30-minute run confirms:

- UI and NetworkProcess can see both
  `/run/user/10000/wayland-0` **and** `nura-browser.sock`;
  WebProcess cannot see either.
- None can see the outer host session D-Bus socket or host user SSH files.
- This is a **NetworkProcess socket leak relative to the desired role
  policy**, even though the host's complete desktop runtime directory
  was never mounted.
- `scripts/verify_nurabrowser_sandbox_snapshot.py` correctly returns
  `release_gate=BLOCKED`: UI and Network lack effective Seccomp;
  Network shares UI user/mount namespaces. Its broad
  `/run/user/10000` mountpoint heuristic also fires even though that
  specific path is a newly bound private directory, not the host's full
  desktop runtime.

The panel/compositor reports the real Atmel maXTouch touchscreen assigned
to Sway `seat0`; **no captured physical `wl_touch` gesture proof** yet.

## Full 30-minute soak

In progress during initial report. Device evidence:
`runs/native-goJdjP`. Command:

    ssh sl101 'sh -s -- https://www.tagesschau.de/ 1800 soak' < scripts/qualify_nurabrowser_nura_native.sh

The supervised harness checks live UI/Web/Network process identities,
RSS, available memory and CPU/skin temperatures every 10 seconds,
and queries the single tab's `uri` and `tabs` state from **inside the
same user namespace**. Aborts on missing helpers, aggregate RSS ≥700 MiB,
available memory ≤100 MiB, CPU ≥70°C or skin ≥60°C. It captures initial
and final Wayland pixels, terminates only tagged WPE processes and verifies
the production release, Sway and WayVNC did not change.

**Do not mark PASS before all 1,800 seconds, process samples, browser state,
exit status and cleanup are independently verified.**

## Unresolved release blockers

1. **NetworkProcess:** source-level WebKit launch interception or audited
   NetworkProcess wrapper preserving the real IPC FDs, with distinct
   user/mount namespaces, its own Seccomp filter and TLS/DNS socket policy.
   WPE's current trusted NetworkProcess has zero filters and inherits the
   UI's Wayland/control sockets. Merely running an independent BPF fixture
   is insufficient.
2. **UI policy:** effective measured Seccomp filter compatible with nested
   WebKit Bubblewrap. A blanket deny of user namespaces/unshare would break
   the WebProcess launcher.
3. **D-Bus:** no ambient bus is exposed, but a positive, name-restricted
   `xdg-dbus-proxy` policy and its denied-name behavioral controls are
   unqualified.
4. **WebProcess behavior:** installed WebKit filter is observed, but its
   actual forbidden-syscall negative control remains unqualified.
5. **Touch and browser shell:** physical touch event tracing, actual
   keyboard/touch navigation, error/recovery controls and broader UI chrome
   remain pending. None may be inferred from VNC pointer input or Sway's
   device inventory alone.
6. A successful stability soak cannot override the above isolation
   failures; keep `runtime_qualification=UNQUALIFIED` and do not promote.
