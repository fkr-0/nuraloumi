# SL101 hardware buttons and slider: event-to-notification qualification (2026-11-25)

## Status and scope

**Host-implemented, not yet physically mapped, not deployed or autostarted.**
The opt-in `nuraloumi-hardware-observer` extends the slider notification
prototype to volume-up, volume-down, display-on, display-off, and an
alternative display-toggle **key role**. It records **raw numeric Linux evdev
events** until a human explicitly supplies a physical mapping. Notification
delivery requires an independent `--notify` opt-in; notifications are not
volume or display-power actions.

Read-only inventory on a reachable SL101 (2026-11-25) identified these
*candidate* interfaces:

| Input device at inventory | Kernel name | Capability | Qualified role? |
| --- | --- | --- | --- |
| `/dev/input/event3` | `gpio-keys` | `EV_KEY` | **No** individual button mapping |
| `/dev/input/event4` | `extcon-keys` | `EV_SW`, switch code `1` (`SW_TABLET_MODE`) | **No** physical open/closed polarity |
| `/dev/input/event5` | `AT Raw Set 2 keyboard` | Keyboard input | Not a proven hardware-control source |

`eventN` numbers can change on reboot; recheck `/proc/bus/input/devices`
before use. Neither the code `1` switch nor any key has been physically
correlated with SL101 slider or button state. No device ACL/permissions,
autostart unit, shell hotkey, display state, or system mixer was changed.
Do not scan the normal keyboard event node: raw codes can disclose typing
patterns. Keep any capture logs local and avoid publishing user input traces.

## Controlled discovery, with a person at the device

For a prebuilt ARMv7/musl binary after independent runtime qualification, or
for a host compiled binary attached to an appropriate input test fixture:

```sh
# First verify the name, capabilities and permissions for the current boot.
cat /proc/bus/input/devices
ls -l /dev/input/event3 /dev/input/event4

# Each invocation only opens ONE explicitly supplied source for reading.
nuraloumi-hardware-observer --device /dev/input/event3 --seconds 45 --max-events 200
# While the scan runs, separately press/release Volume +, Volume -, and the
# display button several times; record code, value, whether press is emitted,
# repeat patterns and whether the display button is one toggle or two roles.
nuraloumi-hardware-observer --device /dev/input/event4 --seconds 45 --max-events 200
# Physically open/close/open/close the slider; record both switch levels.
```

Scan output looks like `EV_KEY code=NNN action=press|release|repeat` and
`EV_SW code=NNN level=0|1`. Release (`value=0`) and auto-repeat (`value=2`)
are visible for discovery but **never trigger a notification**. A key-down
must be rearmed by release; presses within 80 ms are de-bounced. The raw scan
makes **no claim about which physical control caused an event**.
`EV_SW` state is initially unknown; first event establishes an initial
300 ms stable baseline, not an opening/closing notification. The observer
uses a 50 ms tick so event-only switch sources receive the required stable
samples; a later stable change emits the matching slider summary. If an
event source closes or fails to read, the capture terminates; it does not
replay old events or emit a synthetic transition.

The process stops after 60 seconds or 1000 accepted events by default,
configurable up to 600 seconds and 10,000 events. It uses a nonblocking,
read-only character-device handle, without `EVIOCGRAB`, writing sysfs,
or global keyboard hooks. Only direct `/dev/input/eventN` paths are allowed;
unreadable or symlinked sources fail closed. A permission error is **not**
permission to widen `udev` rules, input groups, or ACLs: arrange an approved
short-lived observer context separately. In the 2026-11-25 inventory,
the ordinary `user` session lacked permission for `/dev/input/event4`.

## Only after physical keycode and polarity qualification

Use the exact values **recorded on the specific machine** in place of the
placeholder numbers. The examples below are templates, not SL101 mappings:

```sh
# Read-only labels, never a replacement for normal mixer/power-key handling:
nuraloumi-hardware-observer --device /dev/input/eventN \
    --key volume-up=KEYCODE_UP --key volume-down=KEYCODE_DOWN \
    --key display-toggle=KEYCODE_DISPLAY --seconds 60

# Separate switch source. The polarity below must be measured, not guessed:
nuraloumi-hardware-observer --device /dev/input/eventM \
    --switch SWITCH_CODE:OPEN_LEVEL --seconds 60

# Repeat ONLY with --notify after the read-only map and panel IPC are verified:
nuraloumi-hardware-observer --device /dev/input/eventN \
    --key volume-up=KEYCODE_UP --key volume-down=KEYCODE_DOWN \
    --key display-toggle=KEYCODE_DISPLAY --notify --seconds 60
```

On devices with **separate** display-on and display-off physical controls,
use `--key display-on=...` and `--key display-off=...` instead of
`display-toggle`. Do not assign the same code to multiple roles; the CLI
rejects duplicate codes and roles. Flags require decimal `u16` event codes.
`--notify` without an explicit key or switch mapping is rejected. A
`display-toggle` assignment cannot coexist with `display-on` or
`display-off` assignments in one capture.

The five possible mapped key summaries are `Volume + button pressed`,
`Volume - button pressed`, `Display on button pressed`,
`Display off button pressed`, and `Display toggle button pressed`.
The switch reuses `Slider opened` and `Slider closed`. Each notified event
calls the existing `nuraloumi-ctl notify --summary ... --app NuraLoumi`
through a two-second bounded transport. The notifier makes at most 32
delivery attempts per capture and disables delivery after the first failure;
it does not spawn a retry storm when the panel is unavailable. Both the
floating banner and notification-history
rendering remain separate UI qualification items. A stale panel IPC socket
or failed `Super+Shift+R` panel restart can make this route unavailable;
do not interpret that failure as an input mapping failure.

## Host acceptance and ARMv7/device gate

1. `cargo test -p nuraloumi-providers --locked hardware_input` tests native
   32-bit and 64-bit record formats, fragmented records, invalid switches,
   release/repeat suppression, key debounce and bounded memory.
2. `cargo test -p nuraloumi-shell --bin nuraloumi-hardware-observer --locked`
   tests CLI bounds, notification opt-in, duplicate mappings, path screening,
   and no implicit role mapping.
3. Physically trace three **separate** button sessions on the current kernel,
   proving volume directions, display semantics and false-positive behavior
   (suspend/resume, long press, autorepeat). Discover the switch polarity
   with closed/opened/closed/opened cycles.
4. Qualify a source-matched ARMv7/musl binary on the SL101, including
   time/event ABI, process memory and permissions; then test one to one
   notification histories, banners and no duplicated presses. Confirm the
   panel is running and `nuraloumi-ctl` has the correct session environment.
5. Only with explicit release/deployment approval, decide whether an
   installed foreground utility/service is desirable. No autostart,
   privileged rule, display-power control or volume-mixer action follows
   from this prototype.

This feature does **not** qualify the separate hardware VDE/GPU, WPE/musl,
media player or browser runtime gates.
