# SL101 hardware Fn keys and VT switching

This is a **compositor/seat** operation, not a NuraLoumi menu action. The
persistent NuraLoumi panel is not keyboard-interactive, and a transient
Wayland client cannot install global `Ctrl+Alt+Fn` shortcuts.

## Requested mapping

- Physical `Fn+Ctrl+Alt+1..9` -> Linux VT1..VT9, respectively.
- `Fn+Ctrl+Alt+0` -> VT10 **only if VT10 exists**.
- This treats Fn as the hardware layer that generates F1..F10, **not** as
  an XKB/Wayland modifier. Verify the actual EC mapping before deployment.
- VT numbers are independent of Wayland workspaces. Existing graphical
  sessions may occupy one of the VTs, so verify recovery first.

## Read-only keyboard preflight (required)

1. Record the kernel/DTB, current VT, and session seat:
   `uname -a; cat /sys/class/tty/tty0/active; cat /proc/bus/input/devices`.
2. Run `python3 scripts/sl101-vt-keys.py` on the tablet, then pass the
   AT keyboard's `/dev/input/by-path/*-event-kbd` or `/dev/input/eventN`
   to `python3 scripts/sl101-vt-keys.py DEVICE --seconds 30`.
   Root/group permissions may be needed. The script is read-only, uses
   no key injection and changes neither the compositor nor kernel keymap.
3. Press the desired chord. Interpret observations:
   - F1..F9/F10 with Ctrl+Alt: the Fn layer is working. The compositor
     should receive VT function-key chords; test seat/VT switching next.
   - DIGIT_1..DIGIT_0: Fn is *not* converting digits into F keys. Determine
     whether Fn emits KEY_FN or is absorbed by the EC. Do not install an
     invented mapping; first record the relevant scan codes and kernel.
   - No events: wrong event device, seat/keyboard focus issue, or EC
     driver problem; diagnose input first.
4. Have an SSH/serial recovery path available before any actual VT switch.
   `cat /sys/class/tty/tty0/active` can confirm the active VT.

## Prefer the compositor/seat VT path

labwc/wlroots receives the physical F keys, not NuraLoumi. First try
`Ctrl+Alt+F1` to switch only after confirming that VT1 is a usable target.
With the SL101 slider this *may* mean `Fn+Ctrl+Alt+1`; evidence from the
key-event test decides. The seat backend must be able to perform a VT switch
and the target VT must have a getty or graphical session.

labwc's normal `rc.xml` keybinding syntax uses `C` for Ctrl and `A`
for Alt, e.g. `key="C-A-F2"`. However the standard VT chord can be
consumed in the compositor/seat before normal shortcut handling, so a
user-defined bind is **not** evidence that VT switching works.

If the Fn layer emits digits instead of F keys, a guarded fallback could
bind `C-A-1` to `chvt 1`, but *only* after confirming labwc receives that
chord and the compositor session has permission to issue VT_ACTIVATE.
Do not add a privileged setuid wrapper or `sudo NOPASSWD chvt` as a
workaround. The same principle applies to `C-A-0` -> VT10. On a system
where labwc does not own a VT/session or where Fn is not exposed, address
the actual keyboard/seat problem instead of adding NuraLoumi keybindings.

## Acceptance on real SL101

- Exact input key-event traces for Fn/Ctrl/Alt and all ten digits.
- Same kernel/DTB and seat/logind/seatd backend recorded before/after.
- Switch to an **existing** text VT, type a command, return to the
  graphical VT; keyboard, touch, panel and labwc still work.
- No new system-wide privileges or persistent renderer/boot changes.
- Repeat several transitions and verify menu/terminal launch still works.
- Without this evidence, the VT feature is **unqualified**; the diagnostic
  script and this guide are not a claim that it is working on hardware.

Upstream references: labwc `docs/rc.xml.all` (keyboard notation), labwc
getting-started documentation (reconfigure), Linux input event ABI.
