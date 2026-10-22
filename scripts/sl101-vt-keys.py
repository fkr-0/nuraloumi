#!/usr/bin/env python3
"""Read-only EV_KEY decoder for diagnosing SL101 Fn/VT chords.

Run on the tablet's AT keyboard event node. No keys are remapped and no VT
is switched. Linux input_event uses native C long timeval members; the
format intentionally follows the interpreter's ABI (ARMv7/armhf or x86_64).
"""
import argparse
import glob
import os
import select
import struct
import sys
import time

EVENT = struct.Struct("@llHHi")
EV_KEY = 1
KEY_NAMES = {
    29: "LEFTCTRL", 97: "RIGHTCTRL",
    56: "LEFTALT", 100: "RIGHTALT",
    464: "FN",
    **{number: f"F{number - 58}" for number in range(59, 69)},
    **{number: f"DIGIT_{number - 1}" for number in range(2, 11)},
    11: "DIGIT_0",
}
STATES = {0: "up", 1: "down", 2: "repeat"}


def list_keyboards() -> None:
    paths = sorted(glob.glob("/dev/input/by-path/*-event-kbd"))
    if paths:
        for path in paths:
            print(f"{path} -> {os.path.realpath(path)}")
    else:
        print("No by-path keyboard symlinks; inspect /proc/bus/input/devices", file=sys.stderr)


def capture(path: str, seconds: float) -> int:
    try:
        fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK)
    except OSError as error:
        print(f"cannot open {path}: {error}", file=sys.stderr)
        return 2
    print(f"Reading {path} for {seconds:g}s. Press Fn+Ctrl+Alt+1..9 and 0; Ctrl-C exits.", flush=True)
    deadline = time.monotonic() + seconds
    pending = bytearray()
    try:
        while (remaining := deadline - time.monotonic()) > 0:
            ready, _, _ = select.select([fd], [], [], remaining)
            if not ready:
                break
            try:
                chunk = os.read(fd, EVENT.size * 64)
            except BlockingIOError:
                continue
            if not chunk:
                print("input event stream closed", file=sys.stderr)
                return 1
            pending.extend(chunk)
            while len(pending) >= EVENT.size:
                chunk = bytes(pending[:EVENT.size])
                del pending[:EVENT.size]
                _, _, event_type, code, value = EVENT.unpack(chunk)
                if event_type == EV_KEY and code in KEY_NAMES:
                    print(f"{KEY_NAMES[code]:>12} {STATES.get(value, value)} ({code})", flush=True)
    except KeyboardInterrupt:
        pass
    finally:
        os.close(fd)
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("device", nargs="?", help="/dev/input/eventN or /dev/input/by-path/*-event-kbd")
    parser.add_argument("--seconds", type=float, default=20.0)
    args = parser.parse_args()
    if args.seconds <= 0 or args.seconds > 120:
        parser.error("--seconds must be in (0,120]")
    if not args.device:
        list_keyboards()
        return 0
    return capture(args.device, args.seconds)


if __name__ == "__main__":
    sys.exit(main())
