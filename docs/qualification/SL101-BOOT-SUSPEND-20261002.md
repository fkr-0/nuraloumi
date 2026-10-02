# SL101 boot and suspend safety — 2026-10-02

## Scope

This note records what was observed after the failed unattended deep-suspend
qualification and the subsequent physical recovery of the ASUS SL101 ("Nura").
It separates measured facts from follow-up hypotheses.

The incident was caused by a qualification command, not by a normal
NuraLoumi power-menu action:

```sh
rtcwake -m mem -s 8
```

At the time, `/sys/power/mem_sleep` selected `[deep]`. The RTC reported a
wake event, but the device never returned to Wi-Fi/SSH reachability. A physical
power-cycle recovered it.

## Kernel / boot-image boundary

The failed suspend run had been qualified on the newer grate test kernel:

```text
7.0.1-postmarketos-grate
```

After physical power-cycle the persistent boot returned as:

```text
Linux sl101-nura 6.18.45 #5 SMP PREEMPT
```

Both module trees are present:

```text
/lib/modules/6.18.45
/lib/modules/7.0.1-postmarketos-grate
```

The boot partition contains both a normal image and a separate test image:

```text
/boot/vmlinuz
sha256 794a3766aed121654aed5b88f3a8d826f38b47fa4fcfb5bb01d3267518a0d9e1

/boot/zImage-ec-current
sha256 ab170625dbe038965d3c91a684e9153a91ad0e590edcd47c275c1ff3a7495766
```

Likewise, the normal and EC-current DTBs differ:

```text
/boot/tegra20-asus-sl101.dtb
sha256 01fbb5a935e429c42dc5e2793049bbfc91ff3a6f73c15f2063e1fb0af1a289b0

/boot/tegra20-asus-sl101-ec-current.dtb
sha256 7533523688c510723064187167076ec5261ca0248e66860b01596f65706c426b
```

Therefore suspend results must always name the exact kernel + DTB combination.
A failure on the 7.0.1 test combination must not be generalized to the
persistent 6.18.45 baseline without a separate test.

## Current recovered boot timeline

The recovered 6.18.45 boot log shows:

```text
  0.000 s  kernel entry
  0.591 s  TPS6586x RTC sets system clock
  1.627 s  internal eMMC appears
  1.944 s  root SDXC card appears
  1.948 s  /dev/mmcblk1p1 appears
 19.260 s  ext4 journal/orphan recovery completes
 19.299 s  root filesystem mounted
 23.153 s  Tegra DRM initializes
 23.295 s  tegradrmfb available
 23.505 s  BCM4329 Wi-Fi firmware request begins
 23.769 s  BCM4329 firmware is running
 ~30.1 s  first later brcmfmac activity in dmesg
 ~31 s    local boot diagnostics snapshot
```

The diagnostics snapshot still saw Wi-Fi disconnected. NetworkManager and
wpa_supplicant were already started and the recovered system later connected
normally as `sl101-wifi` on `192.168.23.106/24`.

The 17-second interval between block-device discovery and root mount is not yet
a valid steady-state boot benchmark. This boot followed a forced power-cycle;
ext4 reported six orphan inodes and journal recovery. A clean human-present
reboot must be timed before changing initramfs/root-wait policy.

The postmarketOS initramfs does not contain a fixed 17-second root wait. It
checks root discovery immediately and only polls when the partition is absent.

## Graphical-session boot problem

Cold boot currently starts networking/SSH but does not start labwc or
NuraLoumi. There is no compositor autostart service.

The existing OpenRC service:

```text
sl101-wayvnc
```

starts immediately and assumes:

```text
XDG_RUNTIME_DIR=/run/user/0
WAYLAND_DISPLAY=wayland-0
```

Because no compositor creates `wayland-0`, wayvnc respawns every 10 seconds
and repeatedly logs:

```text
Failed to connect to WAYLAND_DISPLAY="wayland-0"
```

Recommended boot ordering:

```text
localmount / seat
        ↓
SL101 graphical-session supervisor
        ↓
labwc (WLR_RENDERER=pixman)
        ↓ wait for /run/user/0/wayland-0
NuraLoumi panel
        ↓
wayvnc
```

Wayvnc should wait on compositor/socket readiness instead of being an
independent infinite respawn loop.

## Other boot-health findings

These are not current boot blockers, but they deserve separate cleanup:

- `postmarketos-zram-swap` is failed and there is no active swap device.
  The machine has ~1 GiB RAM, so this affects robustness under memory pressure.
- `nftables` is failed. The rules file exists, but `nft list ruleset`
  returns `Unable to initialize Netlink socket: Protocol not supported`.
  Firewall policy therefore needs a kernel/configuration fix rather than merely
  enabling the service.
- the touchscreen driver cannot find `maxtouch.cfg`;
- the keyboard path logs an early reset failure;
- the transformer EC/KBC module path logs duplicate-driver/symbol errors;
- BCM4329 Wi-Fi falls back from a device-specific firmware request and has no
  CLM blob. The SL101-specific NVRAM file does exist as a symlink to the TF201
  profile.

The EC/KBC and SDIO/Wi-Fi resume paths are higher priority than cosmetic warning
cleanup because they are plausible contributors to resume reliability.

## Why direct rtcwake was unsafe

The recovered system runs:

```text
elogind-daemon
sleep-inhibitor
NetworkManager
```

NetworkManager holds an elogind sleep **delay inhibitor** so it can quiesce
networking before suspend.

The device's `sleep-inhibitor` service is also designed to inhibit sleep for
conditions such as active SSH sessions.

A direct:

```sh
rtcwake -m mem ...
```

enters the kernel sleep state directly. It bypasses the normal elogind request
path and therefore bypasses the policy/inhibitor coordination we actually want
to test.

The RTC successfully waking the SoC only proves that the wake interrupt fired.
It does **not** prove that SDIO Wi-Fi, the EC, display, input, compositor, and
userspace resumed correctly.

## NuraLoumi suspend policy

NuraLoumi must use `loginctl` for session actions. It must never directly
write `/sys/power/state`, invoke `rtcwake`, or pass
`--ignore-inhibitors`.

Power actions remain dry-run by default.

Explicit reboot/poweroff enablement is separate from suspend enablement.
Suspend requires a second opt-in because it has not passed SL101 deep-resume
qualification.

This means:

```sh
# reboot/poweroff may execute after UI confirmation
nuraloumi-menu --live --family power --enable-power-actions

# suspend still dry-runs

# suspend is possible only with an additional explicit unsafe opt-in
nuraloumi-menu --live --family power   --enable-power-actions --enable-unsafe-suspend
```

## Recommended OS-side safety belt

Until deep suspend is independently qualified, configure elogind on SL101 to
use only the shallow state:

```ini
# /etc/elogind/sleep.conf.d/10-sl101-safe-suspend.conf
[Sleep]
SuspendState=freeze
```

This keeps ordinary `loginctl suspend` on the inhibitor-aware path while
preventing it from choosing `mem/deep`.

Deep suspend should only be re-enabled for a human-present test with:

1. known exact kernel and DTB hashes;
2. a local physical recovery path;
3. network-independent observation of display/input;
4. RTC and power-button wake sources checked;
5. pre-suspend panel/labwc/network state recorded;
6. post-resume display, touchscreen, keyboard, network, provider and menu checks.

No unattended automation may issue `rtcwake -m mem` on this tablet.

## Safe qualification ladder

Use increasingly deep states, stopping on the first failure:

1. no-sleep dry run: `loginctl --dry-run suspend`;
2. inhibitor-aware shallow suspend with elogind constrained to `freeze`;
3. human-observed shallow suspend/resume;
4. only after that, a separately authorized `mem/deep` experiment.

A successful shallow test does not automatically authorize deep sleep.

## Boot improvement priorities

### P0 — session correctness

Create one OpenRC graphical-session service that owns labwc lifecycle, waits for
`wayland-0`, starts the NuraLoumi panel, and only then enables wayvnc.

This removes the current wayvnc retry storm and makes cold boot reach the
intended shell automatically.

### P0 — suspend safety

Keep suspend separately disabled in NuraLoumi and constrain elogind to
`SuspendState=freeze` until a human-present deep-resume qualification passes.

### P1 — clean reboot timing

Capture one clean reboot with monotonic milestones before optimizing initramfs.
The recovered boot's ext4 journal/orphan recovery contaminates the current
~19-second root-mount measurement.

### P1 — resume-critical drivers

Reconcile the EC/KBC duplicate-driver path and inspect BCM4329 SDIO resume
behavior. These are more likely to explain deep-resume failure than NuraLoumi
userspace.

### P1 — memory robustness

Repair zram or establish an intentional swap strategy. Current boot reports the
zram service failed and `/proc/swaps` is empty.

### P2 — firewall/kernel parity

Either supply working nftables kernel support or choose an explicitly supported
firewall path. Do not leave a failed firewall service looking like a configured
one.

### P2 — firmware/config noise

Resolve the missing maXTouch configuration and evaluate device-specific
BCM4329 firmware/CLM availability after the resume-critical work above.
