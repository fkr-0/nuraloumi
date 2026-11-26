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

## 2026-11-26 native ARMv7 Bubblewrap policy fixture (PASS, not a NetworkProcess launch)

A new bounded negative-control program, `scripts/fixtures/nurabrowser_network_policy_probe.c`,
was built **on the ARMv7/musl toolchain** with
`-mcpu=cortex-a9 -mfpu=vfpv3-d16 -mfloat-abi=hard -mno-unaligned-access`
and `-Wall -Wextra -Werror`. The fixture's single-byte allowed file is
`scripts/fixtures/nurabrowser_policy_allowed.txt` (`Q\n`).
The final executable SHA-256 was
`3797bc409fb408b052a5cdaa0cacd23e69905365340de381a2e1af85df880080`;
the staged binary on native `sl101` had the identical hash. The allowed
file SHA-256 was
`282f82a2d55dbfe663906eecf403ead5ecad4a750d37f8a09d16d99c2283cd61`.
The earlier QEMU linux-user trial gave `seccomp-install: ENOSYS`, so it
was **not** counted as a Seccomp success.

The probe was copied to a fresh, unprivileged
`/tmp/nura-network-policy-XXOFLNga` on `sl101` (ARMv7, Bubblewrap
0.13.0, UID 10000). The following is the effective native policy used;
the named probe and fixture files were staged first:

```sh
bwrap --die-with-parent --new-session \
  --unshare-user --unshare-pid --unshare-uts --unshare-ipc \
  --dir /usr --ro-bind /usr /usr \
  --symlink usr/bin /bin --symlink usr/lib /lib \
  --dev /dev --proc /proc --tmpfs /tmp \
  --dir /fixture \
  --ro-bind /tmp/nura-network-policy-XXOFLNga/nurabrowser_policy_allowed.txt /fixture/allowed.txt \
  --ro-bind /tmp/nura-network-policy-XXOFLNga/network-policy-probe /probe \
  --dir /run --dir /run/user --dir /run/user/10000 \
  --setenv XDG_RUNTIME_DIR /run/user/10000 \
  --unsetenv DBUS_SESSION_BUS_ADDRESS --unsetenv WAYLAND_DISPLAY \
  /probe /fixture/allowed.txt /etc/passwd /run/sl101-desktop-user/wayland-1
```

Native exit **0** with:

```text
MOUNT_POLICY_PASS allowed-file=read forbidden-host-file=ENOENT
SOCKET_POLICY_PASS local-unix=connect inet=create forbidden-host=ENOENT
SECCOMP_POLICY_PASS nnp=1 mode=2 filters=0->1 getppid=EACCES getpid=allowed
NETWORK_POLICY_FIXTURE=PASS not-a-WebKit-launch
```

The host-side control independently confirmed `/etc/passwd` exists
and `/run/sl101-desktop-user/wayland-1` is a live Unix socket. They
were not mounted inside the private root. The fixture read only its
explicitly mounted `Q` file, created and connected its own AF_UNIX
socket under private `/tmp`, and created an AF_INET socket without
initiating outbound traffic. The hidden host file and Wayland socket
were both absent (`ENOENT`). The conventional
`/run/user/10000` was created **empty** inside the namespace; no
ambient Wayland or session-bus socket was bound. The seccomp BPF filter
uses an ARM architecture guard; after `PR_SET_NO_NEW_PRIVS` its
`SECCOMP_SET_MODE_FILTER` operation incremented the live
`/proc/self/status` filter count from zero to one. A deliberate
`getppid` syscall then returned `EACCES`, while `getpid` succeeded;
the separate `ptrace` rule returns `EPERM`.

**Negative control:** repeating the same native invocation with
`--dir /etc --ro-bind /etc/passwd /etc/passwd` added before
`--dir /run` deliberately exposed the forbidden host file.
The probe detected it as
`POLICY_PROBE_FAIL forbidden-host-file-visible` and exited **1**.
This establishes that the mount test fails on a real leak rather than
always reporting success.

**Scope boundary:** This test demonstrates real kernel-enforced
Seccomp, a narrow mount set, and an effective local Unix socket
denial on Nura, but is **not** an independently sandboxed
`WPENetworkProcess`. The BPF deny-two-syscalls program is a test
fixture, **not** a production allowlist. The `/usr` read-only bind is
also deliberately broad for fixture execution. No WebKit IPC
descriptor propagation, DNS/TLS connection, `xdg-dbus-proxy` policy,
or actual auxiliary-process re-exec has yet passed. Network namespace
is intentionally **not** unshared for this network-role fixture.
The existing live NetworkProcess remains `Seccomp=0`, shares the
UI's namespaces, and stays **NO-GO**. No installed service, runtime
tree, compositor, or production browser was changed.

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
