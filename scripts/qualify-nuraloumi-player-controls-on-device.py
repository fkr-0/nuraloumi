#!/usr/bin/env python3
"""Exercise ONE NuraLoumi playerd socket during a real VDE/GPU video stream.

This is an unprivileged hardware qualification fixture, not another player.
"""
import json
import os
from pathlib import Path
import socket
import sys
import time


def command(op: str, **kwargs):
    path = Path(os.environ["XDG_RUNTIME_DIR"]) / "nuraloumi-player.sock"
    data = json.dumps({"op": op, **kwargs}).encode() + b"\n"
    with socket.socket(socket.AF_UNIX) as conn:
        conn.settimeout(7)
        conn.connect(str(path))
        conn.sendall(data)
        result = bytearray()
        while not result.endswith(b"\n"):
            part = conn.recv(4096)
            if not part:
                raise AssertionError(f"{op}: truncated reply")
            result.extend(part)
            if len(result) > 262144:
                raise AssertionError(f"{op}: response too large")
    reply = json.loads(result)
    if not reply.get("ok"):
        raise AssertionError(f"{op} failed: {reply.get('error')}")
    return reply["state"]


def eventually(label, predicate, deadline=6.0):
    start = time.monotonic()
    while time.monotonic() - start < deadline:
        state = command("status")
        if predicate(state):
            return state
        time.sleep(0.12)
    raise AssertionError(f"{label} timeout: {state}")


def main():
    started = eventually(
        "initial video", lambda s: s["status"] == "Playing"
        and s["position_ms"] >= 450, 9.0
    )
    print(f"VIDEO_CONTROL initial_pos_ms={started['position_ms']}")

    command("pause")
    paused = eventually("pause", lambda s: s["status"] == "Paused")
    time.sleep(0.75)
    still = command("status")
    assert still["status"] == "Paused", still
    delta = abs(still["position_ms"] - paused["position_ms"])
    assert delta <= 100, (paused, still)
    print(f"VIDEO_CONTROL pause_stable_delta_ms={delta}")

    command("seek", position_ms=5000)
    sought = eventually(
        "paused seek", lambda s: s["status"] == "Paused"
        and abs(s["position_ms"] - 5000) <= 100
    )
    print(f"VIDEO_CONTROL paused_seek_ms={sought['position_ms']}")

    volume = command("volume", level=0.35)
    assert abs(volume["playlist"]["volume"] - 0.35) < 0.001
    muted = command("mute", muted=True)
    assert muted["playlist"]["muted"] is True
    unmuted = command("mute", muted=False)
    assert unmuted["playlist"]["muted"] is False

    command("play")
    resumed = eventually(
        "resume", lambda s: s["status"] == "Playing"
        and s["position_ms"] >= 5100
    )
    print(f"VIDEO_CONTROL resumed_ms={resumed['position_ms']}")

    before_stop = time.monotonic()
    stopped = command("stop")
    elapsed_ms = (time.monotonic() - before_stop) * 1000.0
    assert stopped["status"] == "Stopped", stopped
    print(f"VIDEO_CONTROL stop_reply_ms={elapsed_ms:.1f}")
    # The normal NuraLoumi client uses a 600-ms response deadline. A
    # synchronous stop must meet it, not merely finish eventually.
    assert elapsed_ms < 600, "Stop exceeds the regular UI IPC deadline"
    print("PLAYERD_CONTROL_E2E_PASS pause=1 seek=1 volume=1 mute=1 resume=1 stop=1")


if __name__ == "__main__":
    try:
        main()
    except Exception as exc:
        print(f"PLAYERD_CONTROL_E2E_FAIL {exc}", file=sys.stderr)
        sys.exit(7)
