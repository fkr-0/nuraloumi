#!/usr/bin/env python3
"""Read-only 30-minute WPE one-tab liveness observer; never launches a browser.

Requires an independently qualified, already-running browser and explicit
process identities. Output is JSONL; a successful soak is not a release gate.
"""
from __future__ import annotations

import argparse
from functools import lru_cache
import importlib.util
import json
import math
import os
from pathlib import Path
import subprocess
import sys
import time
from typing import Callable


ROLES = {
    "ui": "nurabrowser-core",
    "web": "WPEWebProcess",
    "network": "WPENetworkProcess",
}
FULL_SOAK_SECONDS = 1800.0


@lru_cache(maxsize=1)
def _process_sampler():
    path = Path(__file__).with_name("measure-process.py")
    spec = importlib.util.spec_from_file_location("nura_process_sampler", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("Existing read-only process sampler is unavailable")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _status_value(status: str, key: str) -> str:
    for line in status.splitlines():
        if line.startswith(key + ":"):
            return line.split(":", 1)[1].strip()
    raise RuntimeError(f"Process status missing {key}")


def read_process(pid: int, proc_root: Path = Path("/proc")) -> dict:
    record = _process_sampler().snapshot(pid, proc_root)
    root = proc_root / str(pid)
    record["comm"] = (root / "comm").read_text(encoding="utf-8").strip()
    status = (root / "status").read_text(encoding="utf-8")
    record["state"] = _status_value(status, "State").split()[0]
    for key in ("VmHWM", "Threads", "NoNewPrivs", "Seccomp", "Seccomp_filters"):
        try:
            raw = _status_value(status, key).split()[0]
            record[key.lower()] = int(raw)
        except (RuntimeError, ValueError):
            record[key.lower()] = None
    try:
        rollup = (root / "smaps_rollup").read_text(encoding="utf-8")
        record["pss_kib"] = int(_status_value(rollup, "Pss").split()[0])
    except (OSError, RuntimeError, ValueError):
        record["pss_kib"] = None
    return record


def read_thermal_millic(path: Path) -> int:
    value = int(path.read_text(encoding="ascii").strip())
    if not -50000 <= value <= 200000:
        raise ValueError(f"Invalid thermal reading at {path}")
    return value


def probe_browser_cli(cli: Path, command: str) -> dict:
    result = subprocess.run(
        [str(cli), "--remote", command],
        capture_output=True,
        text=True,
        timeout=4,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(f"Browser {command} probe exited {result.returncode}")
    payload = json.loads(result.stdout)
    if not isinstance(payload, dict) or payload.get("ok") is not True:
        raise RuntimeError(f"Browser {command} probe did not report success")
    data = payload.get("result")
    if not isinstance(data, dict):
        raise RuntimeError(f"Browser {command} result is not an object")
    return data


def validate_config(pids: dict[str, int], duration: float, interval: float,
                    max_rss_kib: int, max_temp_c: float, expected_uri: str,
                    thermal_paths: list[Path]) -> None:
    if set(pids) != set(ROLES) or any(type(pid) is not int or pid <= 0 for pid in pids.values()):
        raise ValueError("Require positive exact UI, Web and Network PIDs")
    if len(set(pids.values())) != 3:
        raise ValueError("UI, Web and Network PIDs must be distinct")
    if not math.isfinite(duration) or not 1 <= duration <= FULL_SOAK_SECONDS:
        raise ValueError("Duration must be 1..1800 seconds")
    if not math.isfinite(interval) or not 0.1 <= interval <= 30:
        raise ValueError("Interval must be 0.1..30 seconds")
    if max_rss_kib <= 0 or not math.isfinite(max_temp_c) or not 0 <= max_temp_c <= 125:
        raise ValueError("Set explicit positive RSS and 0..125 C thermal abort limits")
    if not expected_uri.startswith(("http://", "https://")) or len(expected_uri) > 4096:
        raise ValueError("Expected page must be an exact HTTP(S) URI")
    if not thermal_paths or len(thermal_paths) > 8 or len(set(thermal_paths)) != len(thermal_paths):
        raise ValueError("Specify 1..8 distinct thermal sensor paths")


class OneTabObserver:
    def __init__(self, pids: dict[str, int], expected_uri: str,
                 max_rss_kib: int, max_temp_c: float, thermal_paths: list[Path],
                 process_reader: Callable[[int], dict],
                 browser_probe: Callable[[str], dict],
                 thermal_reader: Callable[[Path], int]):
        self.pids = pids
        self.expected_uri = expected_uri
        self.max_rss_kib = max_rss_kib
        self.max_temp_millic = max_temp_c * 1000
        self.thermal_paths = thermal_paths
        self.process_reader = process_reader
        self.browser_probe = browser_probe
        self.thermal_reader = thermal_reader
        self.starts: dict[str, int] = {}

    def sample(self) -> dict:
        processes = {}
        for role, pid in self.pids.items():
            record = self.process_reader(pid)
            # Linux TASK_COMM_LEN is 16 bytes including NUL; /proc/<pid>/comm
            # truncates names longer than 15 bytes (UI and NetworkProcess).
            if record["pid"] != pid or record["comm"] != ROLES[role][:15]:
                raise RuntimeError(f"{role} process identity mismatch at PID {pid}")
            if record["state"] in ("Z", "X", "x"):
                raise RuntimeError(f"{role} is not alive (state {record['state']})")
            first = self.starts.setdefault(role, record["start_ticks"])
            if first != record["start_ticks"]:
                raise RuntimeError(f"{role} PID {pid} was reused or restarted")
            processes[role] = record
        rss = sum(row["rss_kib"] for row in processes.values())
        if rss > self.max_rss_kib:
            raise RuntimeError(f"Aggregate RSS exceeds limit: {rss} KiB")

        temperatures = {str(path): self.thermal_reader(path) for path in self.thermal_paths}
        if max(temperatures.values()) > self.max_temp_millic:
            raise RuntimeError("Thermal abort threshold exceeded")

        page = self.browser_probe("uri")
        tabs = self.browser_probe("tabs")
        entries = tabs.get("tabs")
        if (not isinstance(entries, list) or len(entries) != 1 or
                not isinstance(entries[0], dict) or
                type(tabs.get("active")) is not int or tabs["active"] != 0):
            raise RuntimeError("Not exactly one active WebKit tab")
        if (page.get("uri") != self.expected_uri or
                entries[0].get("uri") != self.expected_uri or
                page.get("is_loading") is not False):
            raise RuntimeError("One-tab URI/loading state changed during soak")
        return {"processes": processes, "aggregate_rss_kib": rss,
                "thermal_millic": temperatures, "page_uri": self.expected_uri,
                "tab_count": 1}


def run_soak(observer: OneTabObserver, duration: float, interval: float,
             emit: Callable[[dict], None], clock: Callable[[], float] = time.monotonic,
             sleeper: Callable[[float], None] = time.sleep) -> dict:
    start = clock()
    deadline = start + duration
    next_due = start
    samples = 0
    while True:
        now = clock()
        if now < next_due:
            sleeper(next_due - now)
            now = clock()
        row = observer.sample()
        samples += 1
        emit({"kind": "sample", "index": samples,
              "elapsed_seconds": round(now - start, 3), **row})
        if now >= deadline:
            break
        next_due = min(deadline, now + interval)
    return {"kind": "summary", "samples": samples,
            "elapsed_seconds": round(clock() - start, 3),
            "result": "LIVENESS_PASS" if duration == FULL_SOAK_SECONDS else "SHORT_PROBE_ONLY",
            "release_qualification": "NOT_ESTABLISHED"}


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    for role in ROLES:
        parser.add_argument(f"--{role}-pid", type=int, required=True)
    parser.add_argument("--browser-cli", type=Path, required=True)
    parser.add_argument("--expected-uri", required=True)
    parser.add_argument("--thermal", type=Path, action="append", required=True)
    parser.add_argument("--max-rss-kib", type=int, required=True)
    parser.add_argument("--max-temp-c", type=float, required=True)
    parser.add_argument("--duration-seconds", type=float, default=FULL_SOAK_SECONDS)
    parser.add_argument("--interval-seconds", type=float, default=10.0)
    args = parser.parse_args(argv)
    pids = {role: getattr(args, f"{role}_pid") for role in ROLES}
    emit = lambda row: print(json.dumps(row, sort_keys=True), flush=True)
    try:
        validate_config(pids, args.duration_seconds, args.interval_seconds,
                        args.max_rss_kib, args.max_temp_c, args.expected_uri, args.thermal)
        if not args.browser_cli.is_file() or not os.access(args.browser_cli, os.X_OK):
            raise ValueError("Browser CLI must exist and be executable")
        observer = OneTabObserver(
            pids, args.expected_uri, args.max_rss_kib, args.max_temp_c, args.thermal,
            read_process, lambda command: probe_browser_cli(args.browser_cli, command),
            read_thermal_millic,
        )
        emit({"kind": "start", "duration_seconds": args.duration_seconds,
              "interval_seconds": args.interval_seconds, "pids": pids,
              "scope": "read-only-existing-native-one-tab"})
        emit(run_soak(observer, args.duration_seconds, args.interval_seconds, emit))
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError,
            subprocess.TimeoutExpired, KeyboardInterrupt) as error:
        emit({"kind": "summary", "result": "SOAK_FAIL", "reason": str(error),
              "release_qualification": "NOT_ESTABLISHED"})
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
