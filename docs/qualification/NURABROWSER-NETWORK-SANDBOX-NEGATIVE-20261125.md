# NuraBrowser NetworkProcess sandbox — negative qualification gate

OCP: `NURALOUMI-20261125-NURABROWSER-NETWORK-SANDBOX-NEGATIVE-20261125-F1EB68`.
Source baseline: `9284b5b1a55b5a9e7ec9aba4161f518c137cfbf0`.

## Verdict: BLOCKED (no release approval)

The 2026-11-25 live canary recorded UI NoNewPrivs=1/Seccomp=0,
NetworkProcess NoNewPrivs=1/Seccomp=0, and WebProcess
NoNewPrivs=1/Seccomp=2/one filter. NetworkProcess shares its outer mount,
user, and network namespaces with UI. This is **not** a role-separated
NetworkProcess enforcement proof. The exact 2026-11-25 live process/mapping
evidence is in `NURABROWSER-LIVE-WAYVNC-20261125.md`; the private runtime
closure there has 30 unresolved findings, including D16/NEON provenance.

The repository's `launch.sh` checks that bwrap and xdg-dbus-proxy exist, and
`engine.c` forbids disabling WebKit's sandbox. Neither launches the WebKit
NetworkProcess through an independently controlled role wrapper. The shipped
WebKit 2.54.2 launcher treats NetworkProcess as trusted: narrowing the outer
UI namespace does **not** produce independent NetworkProcess policy, while
adding a filter to UI before fork may prevent legitimate UI/compositor calls.
No speculative production seccomp allowlist was installed.

## Intended role boundary and required negative controls

| Resource | UI | WebProcess | NetworkProcess | Required proof |
| --- | --- | --- | --- | --- |
| Mounts | own readonly closure, private writable state | reduced readonly web closure, no host root | reduced TLS/DNS closure, no host root | blocked private file open yields EACCES/ENOENT; permitted fixture read succeeds |
| `/run/user/10000` | **only** named Wayland socket and needed state endpoints | no complete runtime directory | no Wayland socket; only verified proxy socket if required | enumerate mountinfo and attempt opening forbidden sibling socket |
| AF_UNIX | selected Wayland/control paths | WebKit IPC only | WebKit IPC and approved proxy | attempt forbidden desktop/session sockets and permitted IPC |
| AF_INET/AF_INET6 | denied unless separately justified | denied | permitted DNS/TLS destination via policy | WebProcess outbound attempt denied; NetworkProcess local test HTTP succeeds |
| D-Bus | explicit proxy names only if justified | no ambient session bus | approved portal/proxy names only if justified | denied name ownership and method call; permitted call recorded |
| Seccomp | own measured filter, minimal UI syscalls | measured WebKit filter | **distinct measured** NetworkProcess filter | actual forbidden syscall returns EPERM or kills fixture; permitted syscall succeeds |
| Lifecycle | independent Sway/WayVNC survive | supervised cleanup | supervised cleanup | exit codes, process tree absence, compositor PIDs and WayVNC frame |

The `/run/user/10000` directory is not itself a capability: binding the whole
directory exposes unrelated sockets and the bus. The compositor endpoint is
needed only for the UI role; NetworkProcess should not inherit it. Exact
socket dependencies and D-Bus allow-names must be measured before choosing
an allowlist. A D-Bus proxy binary on PATH does not prove a policy was applied.

## Host-only observation oracle

`scripts/verify_nurabrowser_sandbox_snapshot.py --ui-pid PID --web-pid PID
--network-pid PID` reads process `/proc` evidence without changing processes,
services, kernel permissions, mounts or network settings. An alternative
`--proc-root` permits deterministic synthetic fixtures. Nonzero exit means
missing effective confinement; exit zero means **SNAPSHOT_PASS_ONLY**, never
behavioral/release PASS. It explicitly flags `Seccomp=0`, missing filters,
shared UI/helper mount/user namespaces, or complete runtime mount binds,
and absent proc entries. Inodes, maps and mountinfo must be re-collected from
the same bounded live run, not inferred from stale PIDs or binary filenames.
The root mount is universal, so its presence alone cannot distinguish a
private filesystem from a broad host bind; check mount sources and inode
identity plus actual forbidden-file opens separately.

The synthetic suite covers a positive snapshot, missing NetworkProcess
filter, shared namespace, broad runtime bind and absent process evidence.
It does not impersonate a real syscall denial, D-Bus request, network denial,
mount-isolation test or successful page navigation.

## Outstanding dependency before device qualification

1. Source-matched D16 private runtime closure, interpreter, dlopen plugins,
   real maps/inodes and release manifest must pass independently.
2. Introduce an audited WebKit NetworkProcess launch interception or a
   deliberately source-matched launcher integration and define **separate**
   UI, Web and Network policy files. Test their filters in host-side bounded
   processes with positive/negative syscall fixtures; inspect actual
   `Seccomp` and filter count after exec.
3. Implement explicit xdg-dbus-proxy name policy, isolated DNS/TLS and
   socket fixtures and forbidden-path/socket tests. Record exact exit codes,
   errno, logs and host hashes.
4. On the unchanged target, run only a separately approved isolated canary;
   capture three live namespace/mountinfo/maps snapshots, navigation and
   blocked accesses, confirm no orphan helpers and surviving Sway/WayVNC.

No changes to `/opt/nuraloumi/current`, the working private canary,
system services, namespace settings or compositor are authorized by this gate.
