#!/usr/bin/env python3
"""Fail-closed, read-only /proc snapshot oracle for NuraBrowser process roles.

This is an observation gate, not an installer or a seccomp policy generator.
It never treats a filter's existence on disk as evidence of enforcement.
"""
import argparse
import json
from pathlib import Path


ROLES = ("ui", "web", "network")


def inspect(root: Path, pids: dict[str, int]) -> dict:
    observations = {}
    failures = []
    for role in ROLES:
        pid = pids[role]
        proc = root / str(pid)
        try:
            status = (proc / "status").read_text()
            fields = dict(line.split(":", 1) for line in status.splitlines() if ":" in line)
            nnp = int(fields["NoNewPrivs"].strip())
            seccomp = int(fields["Seccomp"].strip())
            filters = int(fields["Seccomp_filters"].strip())
            ns = {name: (proc / "ns" / name).readlink().as_posix()
                  for name in ("mnt", "net", "pid", "user", "uts", "ipc")}
            mounts = (proc / "mountinfo").read_text().splitlines()
        except (OSError, ValueError, KeyError) as exc:
            failures.append(f"{role}: missing or malformed live evidence: {exc}")
            continue
        # mountinfo field 5 is the mountpoint; escaped paths remain fail-closed.
        targets = [line.split()[4] for line in mounts if len(line.split()) >= 5]
        observations[role] = {"pid": pid, "no_new_privs": nnp,
                              "seccomp": seccomp, "filters": filters,
                              "namespaces": ns, "mountpoints": targets}
        if nnp != 1 or seccomp != 2 or filters < 1:
            failures.append(f"{role}: effective NoNewPrivs=1/Seccomp=2/filter>=1 not observed")
        if role in ("web", "network"):
            # Every mount namespace has a / mount. Its source and content
            # require a separate inode/visibility probe; do not flag it alone.
            if "/run/user/10000" in targets:
                failures.append(f"{role}: entire desktop runtime directory exposed")
    if len(observations) == len(ROLES):
        ui, web, net = (observations[r]["namespaces"] for r in ROLES)
        for role, other in (("web", web), ("network", net)):
            for namespace in ("mnt", "user"):
                if ui[namespace] == other[namespace]:
                    failures.append(f"{role}: shared {namespace} namespace with UI")
        if web["net"] == net["net"]:
            failures.append("web: shares network namespace with NetworkProcess")
    # This oracle cannot prove syscall, socket, D-Bus, or filesystem DENIAL.
    # A release gate additionally needs real attempted accesses and exit logs.
    return {"snapshot_gate": "BLOCKED" if failures else "SNAPSHOT_PASS_ONLY",
            "release_gate": "BLOCKED", "failures": failures,
            "missing_behavioral_evidence": ["filesystem_denial", "socket_denial",
                                            "dbus_name_denial", "syscall_denial",
                                            "allowed_navigation", "helper_cleanup"],
            "observations": observations}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--proc-root", type=Path, default=Path("/proc"))
    for role in ROLES:
        parser.add_argument(f"--{role}-pid", type=int, required=True)
    args = parser.parse_args()
    result = inspect(args.proc_root, {role: getattr(args, f"{role}_pid") for role in ROLES})
    print(json.dumps(result, sort_keys=True, indent=2))
    return 0 if result["snapshot_gate"] == "SNAPSHOT_PASS_ONLY" else 1


if __name__ == "__main__":
    raise SystemExit(main())
