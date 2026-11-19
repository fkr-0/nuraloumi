# NuraBrowser R2 — native WPE sandbox runtime gate

OCP: `NURALOUMI-20261119-NURABROWSER-R2-SANDBOX-RUNTIME-20261119-0D5721`
Date: 2026-11-19. Scope: local x86_64 host only; no SL101, private Debian armhf root, compositor or deployment claim.

## Outcome

**HOST PASS, DEVICE/PRIVATE-ROOT PENDING.** Installed WPE WebKit and WPEPlatform both 2.54.2. Native Meson engine built and a real headless navigation to `about:blank` loaded successfully. Runtime inspection while this engine was alive observed a Bubblewrap-launched `xdg-dbus-proxy` and a Bubblewrap-launched `WPEWebProcess`. The WebProcess had different user, mount and network namespaces from the engine parent.

The earlier private-glibc Debian armhf bus-proxy failure was **not reproduced** by this Arch x86_64 host test. It has not been resolved for that distinct environment. The private-root gate remains open pending the original ARMHF runtime/loader reproduction, within a separately authorized deployment/fixture environment.

## Evidence (actual host observations)

- `pkg-config --modversion wpe-webkit-2.0 wpe-platform-2.0`: 2.54.2 / 2.54.2 (exit 0).
- `meson setup /tmp/nurabrowser-r2-sandbox-build components/nura-browser-engine`: exit 0.
- `meson compile -C /tmp/nurabrowser-r2-sandbox-build`: exit 0 before and after the engine guard change.
- `meson test -C /tmp/nurabrowser-r2-sandbox-build --print-errorlogs`: 7/7 PASS, exit 0.
- `NURABROWSER_TEST_CORE=/tmp/nurabrowser-r2-sandbox-build/nurabrowser-core python3 -m unittest discover -s components/nura-browser-engine/tests -p test_sandbox_runtime.py -v`: 3/3 PASS, exit 0. Tested `WEBKIT_USE_SANDBOX=0/false/empty`, dangerous override even when empty, and missing helper PATH. Every prohibited launch exits nonzero before WebKit startup.
- `WEBKIT_USE_SANDBOX=1 .../nurabrowser-core --platform=headless --no-control --url=about:blank --exit-after-ms=12000`: observed `WebKit load-finished (failed=no)` and `headless CI PASS`.
- During this exact run, `ps` showed engine PID 2790008; proxy chain 2790043 → 2790044 → 2790045 (`/usr/bin/xdg-dbus-proxy`); WebProcess chain 2790048 → 2790052 → 2790055 (`/usr/lib/wpe-webkit-2.0/WPEWebProcess`), each chain launched by `/usr/bin/bwrap --args ...`.
- Same live observation: engine namespace identities user `4026531837`, mount `4026531832`, network `4026531833`; WebProcess user `4026534981`, mount `4026534522`, network `4026534525`. These three namespace pairs differ. PIDs are ephemeral and useful only as historical observation evidence.

## Fail-closed boundary

The direct native engine now rejects the dangerous WebKit sandbox-disable variable even when empty, rejects `WEBKIT_USE_SANDBOX` values other than `1` (unset is accepted), and refuses startup when either `bwrap` or `xdg-dbus-proxy` cannot be resolved on PATH. This supplements the historical private-root launcher checks; it does not weaken them. WebKit is still responsible for actually creating and enforcing its process sandbox. No unsafe fallback path has been introduced. IPC/navigation/control sources and protocol contract were unchanged.

## Additional legacy fixture result

- Initial run: `python3 -m unittest discover -s components/nura-browser-engine/tests -p test_offline_qualification.py -v`: **21/22 PASS, 1 FAIL, exit 1**. The failing `test_explicit_pkg_config_linkage` expected the retired `rename: ',browse'` Meson alias.
- Follow-up correction (2026-11-19): that test now asserts the installed internal `nurabrowser-core` libexec location and rejects the old alias. Repeat: **22/22 PASS, exit 0**; `test_migration_contract.py`: **3/3 PASS, exit 0**. No public launcher was reintroduced.

## ARMHF container/QEMU attempt (2026-11-19 follow-up)

- The already-local Docker image `sl101-nurabrowser-wpe-dev:2.54.0-2` contains genuine Debian `libwpewebkit-2.0-1` **2.54.0-2 armhf**, but `bwrap` and `xdg-dbus-proxy` are **amd64**. It is an x86_64-hosted ARMHF cross-build environment, not the target private runtime root.
- Offline `docker run --rm --network none` against the read-only project source compiled current ARMHF `nurabrowser-core` with the existing `build-armhf.sh`, deposited in a separate host `/tmp/nurabrowser-r2-armhf-follow-build`, and passed its ELF audit: **exit 0, qualified-elf**. No device, network, or published artifact was touched.
- The same ELF's `--help` under the host's `qemu-arm-static -L /` in that container: **exit 0**, confirming the user-mode launcher reaches option parsing (not a WebProcess proof).
- A bounded headless QEMU runtime attempt with sandbox **enabled**, Docker network disabled and non-root UID, started WPEPlatform but then printed `bwrap: No permissions to create a new namespace, likely because the kernel does not allow non-privileged user namespaces`; subsequently WebKit's credential IPC aborted (`Connection: failed to receive credentials`), **exit 134**. This is **FAIL**, not a renderer or WebProcess PASS. No unsandboxed fallback was attempted.
- Exact unavailable prerequisite: permission for nested Bubblewrap user namespaces in the isolated Docker/QEMU host arrangement, plus a truly target-matched private ARMHF runtime root with ARMHF sandbox helpers. The x86_64 host's successful bwrap isolation does not prove the container's nested namespace capability. Do not change Docker security settings, kernel userns policy, or start privileged containers to force a result under this lane.

## Remaining qualification

1. Reproduce the historic `xdg-dbus-proxy` failure on the exact pinned private-glibc ARMHF root, with the archived loader, dependencies, PATH and WebKit debug logs. Keep the run isolated; do not infer parity from x86_64 packages.
2. Confirm runtime WebProcess namespace identities and proxy exit status there, plus page load and expected sandbox error behavior. If the launcher fails, retain the failure as FAIL/PENDING, never relaunch with sandbox disabled.
3. A separate SL101 qualification requires explicit device authorization, artifact provenance and rollback; this host proof is not that qualification.
