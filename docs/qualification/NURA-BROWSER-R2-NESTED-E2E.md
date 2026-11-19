# NuraBrowser R2 Gate 3C — nested compositor E2E

Date: 2026-11-19 UTC. Follow-up to OCP
NURALOUMI-20261119-NURABROWSER-R2-COMPOSITOR-E2E-20261119-8C1B5B.

## Verdict: harness restored; real Gate 3C remains PENDING

The predecessor reported created R1-03C scripts but they were absent from the
checkout and reachable Git history. Its claim e14b3007 expired at 01:25:46
UTC; a new non-overlapping claim was acquired at 01:26:15 before restoring:

- scripts/qualify-browser-nested-wayland.py — private compositor, real engine
  and browser startup, deterministic HTTP pixel oracle, PNG sampling, monotonic
  first frame and process-tree RSS;
- scripts/qualify-browser-nested-wayland.sh — shell entrypoint;
- scripts/test_qualify_browser_nested_wayland.py — negative safety contract
  and positive/negative PNG oracle fixtures.

## Actual host preflight and checks

| Command or dependency | Observed |
| --- | --- |
| sway or labwc | MISSING |
| grim | MISSING |
| wtype | MISSING |
| native WPE engine | MISSING from PATH |
| target/debug/nuraloumi-browser | PRESENT |
| Python 3, Pillow, ShellCheck | PRESENT |
| Python compile for harness + test | PASS, exit 0 |
| unittest scripts/test_qualify_browser_nested_wayland.py | PASS, 5 tests, exit 0 |
| shellcheck and bash -n for shell wrapper | PASS, exit 0 |
| sh scripts/qualify-browser-nested-wayland.sh --probe | SKIP, exit 77 |

The probe produced structured JSON with the exact four missing prerequisites
and did not create an output directory or invoke a compositor. This is a new
observed exit 77; it does not depend on the predecessor's historical test.
No package installation, remote Git/CI, active Sway mutation, device command
or global process kill occurred.

### Follow-up native WPE build (same host, separate temporary build dir)

The pkg-config name is wpe-platform-2.0 (not wpe-platform-1.0); both
wpe-platform-2.0 and wpe-webkit-2.0 report 2.54.2. The build was performed
without installing dependencies or writing into other agents' source lanes.

| Command | Observed |
| --- | --- |
| meson setup /tmp/nurabrowser-gate3c-fu1-20261119 components/nura-browser-engine | PASS, exit 0 |
| meson compile -C /tmp/nurabrowser-gate3c-fu1-20261119 | PASS, exit 0 |
| meson test -C /tmp/nurabrowser-gate3c-fu1-20261119 --print-errorlogs | PASS, 7/7, exit 0 |
| ./scripts/qualify-browser-nested-wayland.sh --probe --engine-bin /tmp/nurabrowser-gate3c-fu1-20261119/nurabrowser-core | SKIP, exit 77: only sway/labwc, grim and wtype missing |

The native engine executable is now available for this ephemeral qualification
run, but Meson success is neither WebProcess sandbox proof nor real page pixels.
No ordinary system package installation was attempted.

## Reproduce safely

    python3 -m unittest -v scripts/test_qualify_browser_nested_wayland.py
    sh scripts/qualify-browser-nested-wayland.sh --probe

Once the missing packages/binaries have been authorized and provided, choose
a *new* output directory and current-source native WPE engine executable:

    sh scripts/qualify-browser-nested-wayland.sh --engine-bin /path/to/native/engine --browser-bin target/debug/nuraloumi-browser --output-dir /tmp/nura-browser-gate3c-unique --timeout 12

Run isolation is enforced with a dedicated 0700 XDG_RUNTIME_DIR and private
Wayland socket, separate XDG config/data/cache/state directories,
WLR_BACKENDS=headless, WLR_RENDERER=pixman and no inherited live compositor
socket or DBus session connection. Only child process groups created by this
run are terminated. Localhost HTML paints a bright green oracle patch;
captured PNG pixel samples, bounds, timestamp and RSS descendants are written
to evidence.json and log files in the new directory.

## Qualification matrix

| Gate | Status |
| --- | --- |
| Dedicated compositor/private socket | PENDING, prerequisites absent |
| Actual WPE page screenshot and painted pixel oracle | PENDING |
| Browser chrome tabstrip/address geometry and 48 logical px targets | PENDING |
| Keyboard semantic focus and navigation parity | PENDING |
| Real virtual touch down/up and semantic parity | PENDING |
| First frame and shell/WebProcess/compositor RSS budget | PENDING |
| SL101 no-NEON real device acceptance | PENDING, independent gate |

The harness intentionally does not turn a keyboard injection exit code into
focus proof or a pointer tap into a touch event. Even when page pixels are
confirmed, full Gate 3C remains open until real chrome geometry, input
semantics and touch are attributable and independently verified. Incomplete
runtime qualification returns nonzero, not a fake success.
