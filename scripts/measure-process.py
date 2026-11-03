#!/usr/bin/env python3
"""Read-only, bounded /proc sampler for an already running NuraLoumi process set.

Evidence is emitted as JSON lines on stdout; no process is launched, signalled or
configured. Use shell redirection to save evidence. It does not prove first frame.
"""
from __future__ import annotations

import argparse
import json
import os
import time
from pathlib import Path


def parse_stat(data: str) -> tuple[int, int, int]:
    # comm is parenthesised and may contain whitespace or ")".
    closing = data.rfind(")")
    if closing < 0 or not data.startswith("(") and "(" not in data[:closing]:
        raise ValueError("invalid proc stat comm")
    fields = data[closing + 1 :].split()  # state is field 3, index 0
    if len(fields) < 22:
        raise ValueError("short /proc stat")
    return int(fields[11]) + int(fields[12]), int(fields[19]), int(fields[21])


def snapshot(pid: int, proc: Path = Path("/proc")) -> dict:
    stat = (proc / str(pid) / "stat").read_text(encoding="ascii")
    ticks, start_ticks, rss_pages = parse_stat(stat)
    return {"pid": pid, "start_ticks": start_ticks,
            "cpu_ticks": ticks, "rss_kib": (rss_pages * os.sysconf("SC_PAGE_SIZE")) // 1024}


def bounded(samples: int, interval: float, pids: list[int]) -> None:
    if not 1 <= samples <= 60 or not 0.1 <= interval <= 2.0 or (samples - 1) * interval > 120:
        raise ValueError("samples must be 1..60, interval 0.1..2s, duration <=120s")
    if not pids or len(pids) > 16 or len(set(pids)) != len(pids) or any(pid <= 0 for pid in pids):
        raise ValueError("supply 1..16 distinct positive --pid arguments")


def sample(pids: list[int], samples: int, interval: float) -> None:
    bounded(samples, interval, pids)
    hz = os.sysconf("SC_CLK_TCK")
    previous: dict[int, dict] = {}
    previous_time = None
    for i in range(samples):
        start = time.monotonic()
        rows = [snapshot(pid) for pid in pids]
        for row in rows:
            old = previous.get(row["pid"])
            if old is not None and old["start_ticks"] != row["start_ticks"]:
                raise RuntimeError(f"PID reused during sampling: {row['pid']}")
            row["cpu_percent_one_core"] = (
                round(100 * (row["cpu_ticks"] - old["cpu_ticks"]) / hz /
                      (start - previous_time), 2)
                if old is not None and previous_time is not None and start > previous_time else None
            )
        payload = {
            "kind": "sample", "sample": i + 1, "monotonic_seconds": round(start, 6),
            "processes": rows, "aggregate_rss_kib": sum(row["rss_kib"] for row in rows),
        }
        print(json.dumps(payload, sort_keys=True), flush=True)
        previous = {row["pid"]: row for row in rows}
        previous_time = start
        if i + 1 < samples:
            time.sleep(max(0.0, interval - (time.monotonic() - start)))
    print(json.dumps({"kind": "summary", "samples": samples,
                      "pids": pids, "scope": "existing-processes-read-only",
                      "device_runtime": "not-established-by-host-sampling"},
                     sort_keys=True), flush=True)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--pid", type=int, action="append", required=True,
                        help="existing process ID; repeat to sum multiple binaries (maximum 16)")
    parser.add_argument("--samples", type=int, default=5)
    parser.add_argument("--interval", type=float, default=1.0,
                        help="seconds between samples (0.1..2.0)")
    args = parser.parse_args()
    try:
        sample(args.pid, args.samples, args.interval)
    except (ValueError, OSError, RuntimeError) as error:
        parser.exit(2, f"MEASURE=FAIL reason={error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
