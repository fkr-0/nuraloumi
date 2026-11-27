#!/usr/bin/env python3
"""Independent read-only checker for the supervised native Nura Wayland soak.

This verifies *observed* process continuity, one-tab IPC, health limits and
compositor frame headers. It never grants browser/sandbox release approval.
"""
from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path
import re
import struct

SAMPLE = re.compile(
    r"^(?P<time>\S+) ui=(?P<ui>\d+) web=(?P<web>\d+) "
    r"network=(?P<network>\d+) rss_kib=(?P<rss>\d+) "
    r"available_kib=(?P<available>\d+) cpu_mC=(?P<cpu>\d+) "
    r"skin_mC=(?P<skin>\d+)(?P<roles>.*)$"
)
ROLE = re.compile(r"^(ui|web|network):(\d+):(\d+)$")
PNG_MAGIC = b"\x89PNG\r\n\x1a\n"


def _image_dimensions(path: Path) -> tuple[int, int]:
    with path.open("rb") as source:
        header = source.read(24)
    if len(header) != 24 or not header.startswith(PNG_MAGIC) or header[12:16] != b"IHDR":
        raise ValueError(f"{path.name} is not a PNG")
    return struct.unpack(">II", header[16:24])


def verify(
    run: Path, expected_uri: str, *, minimum_seconds: int = 1800,
    minimum_samples: int = 150, minimum_pairs: int = 150,
) -> dict:
    failures: list[str] = []
    samples = []
    identities: dict[str, int] = {}
    max_rss, max_cpu, max_skin = 0, 0, 0
    try:
        raw_samples = (run / "soak-samples.log").read_text().splitlines()
    except OSError as exc:
        raw_samples = []
        failures.append(f"missing process samples: {exc}")
    for index, line in enumerate(raw_samples):
        match = SAMPLE.fullmatch(line)
        if not match:
            failures.append(f"sample {index}: invalid shape")
            continue
        group = match.groupdict()
        try:
            timestamp = datetime.fromisoformat(group["time"].replace("Z", "+00:00"))
            if timestamp.tzinfo is None or timestamp.utcoffset().total_seconds() != 0:
                raise ValueError("expected explicit UTC")
            roles: dict[str, list[tuple[int, int]]] = {"ui": [], "web": [], "network": []}
            for value in group["roles"].split():
                role_match = ROLE.fullmatch(value)
                if not role_match:
                    raise ValueError(f"bad role entry {value!r}")
                role, pid, rss = role_match.groups()
                roles[role].append((int(pid), int(rss)))
            counts = {role: int(group[role]) for role in roles}
            if counts != {role: len(items) for role, items in roles.items()}:
                raise ValueError("role count does not match sampled PIDs")
            if counts["ui"] != 1 or counts["network"] != 1 or counts["web"] < 1:
                raise ValueError(f"missing/ambiguous role: {counts}")
            if sum(memory for items in roles.values() for _, memory in items) != int(group["rss"]):
                raise ValueError("RSS total differs from per-process observations")
            for role in roles:
                present = {pid for pid, _ in roles[role]}
                if role not in identities:
                    identities[role] = roles[role][0][0]
                if identities[role] not in present:
                    raise ValueError(f"{role} original process disappeared/restarted")
            if (int(group["rss"]) >= 716800 or int(group["available"]) <= 100000
                    or int(group["cpu"]) >= 70000 or int(group["skin"]) >= 60000):
                raise ValueError("memory/RSS/thermal abort threshold exceeded")
            max_rss = max(max_rss, int(group["rss"]))
            max_cpu = max(max_cpu, int(group["cpu"]))
            max_skin = max(max_skin, int(group["skin"]))
            samples.append(timestamp)
        except (ValueError, OverflowError) as exc:
            failures.append(f"sample {index}: {exc}")
    elapsed = 0.0
    if samples:
        elapsed = (samples[-1] - samples[0]).total_seconds()
        for previous, current in zip(samples, samples[1:]):
            delta = (current - previous).total_seconds()
            if not 0 < delta <= 30:
                failures.append(f"invalid sampling gap: {delta} seconds")
    if len(samples) < minimum_samples or elapsed < minimum_seconds:
        failures.append(
            f"insufficient soak: {len(samples)} samples, {elapsed} seconds "
            f"(require {minimum_samples} / {minimum_seconds})"
        )

    pairs = 0
    try:
        lines = (run / "state" / "browser-state.jsonl").read_text().splitlines()
        if len(lines) % 2:
            failures.append("incomplete browser-state URI/tabs pair")
        for index in range(0, len(lines) - 1, 2):
            uri, tabs = (json.loads(lines[index]), json.loads(lines[index + 1]))
            pairs += 1
            if uri.get("ok") is not True or tabs.get("ok") is not True:
                failures.append(f"browser state pair {pairs}: failed IPC")
                continue
            u, t = uri.get("result"), tabs.get("result")
            entries = t.get("tabs") if isinstance(t, dict) else None
            if not isinstance(u, dict) or not isinstance(entries, list):
                failures.append(f"browser state pair {pairs}: invalid result")
                continue
            if (len(entries) != 1 or t.get("active") != 0
                    or entries[0].get("uri") != expected_uri
                    or u.get("uri") != expected_uri):
                failures.append(f"browser state pair {pairs}: wrong tab/URI")
            # Allow the first ~30 seconds for TLS/page load to settle.
            if pairs > 3 and (u.get("is_loading") is not False
                              or entries[0].get("loading") is not False):
                failures.append(f"browser state pair {pairs}: page still loading")
    except (OSError, ValueError, AttributeError, TypeError) as exc:
        failures.append(f"invalid browser state log: {exc}")
    if pairs < minimum_pairs:
        failures.append(f"insufficient browser observations: {pairs}/{minimum_pairs}")

    try:
        engine = (run / "engine.log").read_text()
        if "WebKit load-finished (failed=no)" not in engine:
            failures.append("successful initial WebKit load not found")
        if "WebKit load-finished (failed=yes)" in engine or "WebProcess crashed" in engine:
            failures.append("WebKit load/crash failure in engine log")
    except OSError as exc:
        failures.append(f"missing engine log: {exc}")
    for image in ("wayland-first.png", "wayland-last.png"):
        try:
            if _image_dimensions(run / image) != (1280, 800):
                failures.append(f"{image}: unexpected output dimensions")
        except (OSError, ValueError) as exc:
            failures.append(f"{image}: {exc}")
    return {
        "stability_gate": "SOAK_PASS_ONLY" if not failures else "BLOCKED",
        "release_gate": "BLOCKED",
        "reason": "UI and NetworkProcess isolation and physical touch still unqualified",
        "failures": failures,
        "samples": len(samples),
        "sample_elapsed_seconds": elapsed,
        "browser_state_pairs": pairs,
        "original_pids": identities,
        "max_aggregate_rss_kib": max_rss,
        "max_cpu_millic": max_cpu,
        "max_skin_millic": max_skin,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--run-dir", type=Path, required=True)
    parser.add_argument("--expected-uri", required=True)
    parser.add_argument("--minimum-seconds", type=int, default=1800)
    parser.add_argument("--minimum-samples", type=int, default=150)
    parser.add_argument("--minimum-pairs", type=int, default=150)
    args = parser.parse_args()
    result = verify(args.run_dir, args.expected_uri,
                    minimum_seconds=args.minimum_seconds,
                    minimum_samples=args.minimum_samples,
                    minimum_pairs=args.minimum_pairs)
    print(json.dumps(result, indent=2, sort_keys=True))
    return 0 if result["stability_gate"] == "SOAK_PASS_ONLY" else 1


if __name__ == "__main__":
    raise SystemExit(main())
