# Qualify the WPE NetworkProcess and WebProcess sandbox restrictions

## Goal
Prove that the WPE 2.54.2 UI, Web and Network processes on Nura use deliberately separate mount, namespace, socket, D-Bus and Seccomp privileges, with reproducible negative tests and no compositor regression.

## Current evidence
- Start at current NuraLoumi HEAD and read `docs/qualification/NURABROWSER-LIVE-WAYVNC-20261125.md` plus the current runtime closure report.
- In the bounded device inspection, WPEWebProcess had `NoNewPrivs=1`, `Seccomp=2`, a filter and distinct namespaces; WPENetworkProcess had `NoNewPrivs=1` but `Seccomp=0`. Existing broad mount exposure and D-Bus/socket exceptions remain unqualified. The running canary's loader closure is not yet a release gate pass.

## Scope
1. Audit process launch wiring and Bubblewrap policy for UI/Web/Network roles. Document intended `/run/user/10000` semantics and bind only necessary Wayland, proxy D-Bus, DNS/network and runtime endpoints.
2. Design and implement least-privilege NetworkProcess Seccomp filter, any needed xdg-dbus-proxy policy, and deterministic fixture/negative tests for denied filesystem paths, sockets, D-Bus names and syscalls. A filter file existing on disk does not prove it was loaded.
3. Verify effective `NoNewPrivs`, `Seccomp`, namespace inodes, process mappings, denied accesses and allowed navigation with supervised bounded processes; collect exact exit codes and logs.
4. Ensure a blocked WebProcess/NetworkProcess terminates cleanly without orphan helpers or breaking Sway and WayVNC. If physical device verification depends on the new D16 closure, implement/test the policy host-side first and explicitly checkpoint device verification as blocked until the runtime gate passes.

## Boundaries
- Use authenticated projmgrauth, root instructions, workspace claims and bounded commands. No recursive/fan-out prompts.
- Preserve the working persistent canary and live release; no overwrite of current runtime, global namespace settings, system services, Sway/WayVNC, reboot, or remote push.
- A blocked permission or user namespace case is a test result, not permission to bypass policy.

## Acceptance and delivery
- Positive and negative controls demonstrate both WPE helpers' effective least-privilege policies, or mark the gate `BLOCKED` with exact missing evidence.
- Test output and a table of allowed/denied mounts, sockets, D-Bus names, syscalls and role boundaries are committed or otherwise reviewable.
- Final checkpoint includes source commit, test hashes/exits and outstanding dependencies. Do not approve or promote release.
