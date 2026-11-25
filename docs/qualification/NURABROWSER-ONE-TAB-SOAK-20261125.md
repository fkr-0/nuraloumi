# NuraBrowser: supervised native single-tab 1,800-second soak

Date: 2026-11-25. Status: **HOST OBSERVER READY; NATIVE SOAK NOT RUN; RELEASE BLOCKED**.

## Why this is not yet a native PASS

The historical private WayVNC WPE canary rendered a first page, navigated to
another page, and exited cleanly. That session was explicitly closed, so there
is no continuously running WebKit tab to observe from that evidence.

`NURABROWSER-NATIVE-E2E-GATE-CD55F4-20261125.md` explicitly blocks **new
native/device exercises**: the pinned D16 private ARMv7/musl runtime closure
and NetworkProcess sandbox remain unqualified. Preserve the deployed
`/opt/nuraloumi/current`, Sway, WayVNC and working canary state; do not launch,
swap, restart, reboot or mutate the target to satisfy this checklist.

The new `scripts/nurabrowser_one_tab_soak.py` is therefore prepared and tested
**only on the host**, with synthetic 1,800-second time progression and a
read-only snapshot of the host test process. Neither is a physical soak.

## Prerequisites for a future approved device session

1. Clear the recorded D16 pinned runtime closure and NetworkProcess sandbox
   gates with evidence; assign an owner and agree on rollback/abort authority.
2. Independently start and confirm one real, permitted native WPE page in one
   browser tab under the intended musl/runtime root. Capture the exact three
   UI/Web/Network PIDs, binary and runtime identities, namespaces, process
   command lines and mount/socket/seccomp provenance **before** observing.
3. Establish a stable exact HTTP(S) fixture URL served by the approved device
   environment. Confirm touch/WayVNC frame capture separately; the observer
   does **not** capture pixels or exercise gestures.
4. Identify real thermal sensor `temp` paths and choose **predeclared** upper
   bounds for aggregate resident KiB and degrees Celsius. Record baseline RSS,
   temperature and the expected target URI; never invent release thresholds.
5. Run the following on the same device/runtime environment as the existing
   WPE processes, without changing any of them. Replace *all* placeholders:

   ```sh
   python3 scripts/nurabrowser_one_tab_soak.py \
     --ui-pid UI_PID --web-pid WEB_PID --network-pid NETWORK_PID \
     --browser-cli /VERIFIED/PATH/nuraloumi-browser \
     --expected-uri https://VERIFIED-HOST/soak \
     --thermal /sys/class/thermal/VERIFIED_ZONE/temp \
     --max-rss-kib AGREED_RSS_KIB --max-temp-c AGREED_TEMP_C \
     --duration-seconds 1800 --interval-seconds 10 \
     > /APPROVED-EVIDENCE-DIR/one-tab-soak.jsonl
   ```

   A `--browser-cli` is required so the read-only `--remote uri` and `--remote
   tabs` queries reach the **already-running** native engine. A shorter host
   probe can use `--duration-seconds 1 --interval-seconds 0.5`, but can never
   claim the 30-minute criterion.

## What the observer actually qualifies

At a nominal 10-second cadence it takes 181 JSONL samples, including time
zero and second 1,800 (and a start/summary record). It checks:

| Evidence | Fail-closed condition |
| --- | --- |
| `/proc` PIDs, start ticks, `comm`, state | Missing/changed/reused PID, unexpected process role, zombie |
| `/proc` RSS and optional PSS, CPU ticks, VmHWM, threads | Aggregate RSS greater than the predeclared limit |
| `/proc` security fields | `NoNewPrivs`, `Seccomp`, `Seccomp_filters` included as observations, **not** interpreted as a sandbox PASS |
| Thermal sensors | Any sensor over the declared temperature limit or unreadable |
| Browser read-only RPC | Lost/erroring IPC, changed URI, loading state not explicitly false, anything other than exactly one active tab |

Linux `/proc/PID/comm` truncates names at 15 bytes. Both `nurabrowser-core`
and `WPENetworkProcess` exceed that bound; matching the truncated expected
names is intentional. For exact executable identity, the **separate preflight**
must capture `/proc/PID/exe`, command line, hashes and runtime manifest.

Exit code 0 with `LIVENESS_PASS` after 1,800 real elapsed seconds demonstrates
**sampled one-tab liveness only**. A shorter successful run yields
`SHORT_PROBE_ONLY`. Any failed check, lost process or interruption yields
`SOAK_FAIL` and nonzero exit. All summaries state
`release_qualification: NOT_ESTABLISHED`: pixel stability, navigation, touch,
crash recovery, p95 interaction latency, strict sandbox and D16 closure still
need independent proof. If WayVNC pixels, health or input are abnormal,
supervision must abort the session regardless of this observer's success.

## Host-only verification

`python3 -m unittest scripts.test_nurabrowser_one_tab_soak scripts.test_measure_process -v`

The synthetic tests exercise role matching with Linux truncation, reused or
zombie PIDs, malformed/second tabs, URL/loading changes, missing thermal
limits, memory and temperature abort, a 1-second probe, and a fake-clock
1,800-second pass. **None is a physical 30-minute stability result.**
