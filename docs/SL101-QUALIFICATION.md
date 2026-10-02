# SL101 / Nura qualification gate

NuraLoumi 0.1 is not considered target-qualified from host builds alone.

## Hardware facts to respect
- Tegra20 / ARMv7.
- No NEON.
- software-rendered labwc/pixman is the known desktop recovery path.
- compositor-grade EGL/GLES remains independent from NuraLoumi's required path.

## Required target evidence

1. binary starts without SIGILL.
2. panel appears using software Wayland buffers.
3. touch four-corner mapping is correct after output transform.
4. slider keyboard can open/navigate/activate/close menus.
5. menu can be opened/closed 100 times without growth or compositor failure.
6. suspend/resume restores panel or cleanly restarts it.
7. idle and menu-open RSS/CPU are recorded.
8. launcher/system/network/audio menus have stable failure states when services are absent.
9. killing NuraLoumi never kills or wedges labwc.
10. rollback to baseline session requires no filesystem surgery.

## Suspend / resume safety gate

Suspend evidence is kernel/DTB-specific. Record the exact kernel release and
hash/identity of the boot image + DTB before a real sleep test.

The qualification ladder is:

1. `loginctl --dry-run suspend`;
2. inhibitor-aware shallow suspend constrained to `freeze`, with a human present;
3. verify display, touch, slider keyboard, labwc/panel, providers and network;
4. only after shallow recovery passes, separately authorize a human-present
   `mem/deep` experiment.

Never use unattended `rtcwake -m mem`, direct `/sys/power/state` writes, or
ignore-inhibitor flags as a qualification shortcut. An RTC interrupt waking the
SoC is not a resume PASS. If SSH is the only recovery path, the real suspend
test is not allowed.

See `docs/qualification/SL101-BOOT-SUSPEND-20261002.md` for the 2026-10-02
deep-suspend recovery failure and current safety policy.

## Initial budgets
- total NuraLoumi idle RSS target: <= 35 MiB.
- menu-open additional RSS: <= 12 MiB.
- first interactive popup frame: <= 250 ms.
- no continuous redraw when idle.
