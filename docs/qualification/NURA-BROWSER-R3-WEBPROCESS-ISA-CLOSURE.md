# NuraBrowser R3 — WebProcess ARMHF/no-NEON closure

OCP: NURALOUMI-20261119-NURABROWSER-R3-WEBPROCESS-ISA-CLOSURE-20261119-D8F5C1

## Release decision: FAIL (not an ARMv7 runtime PASS)

The local pinned image `sl101-nurabrowser-wpe-dev:2.54.0-2` has image
identity `sha256:5d7f46c14a1667aa77a7bc17fcbef9e0910e15fa6dd360540dfebdcb33553a17`.
This identifies a *cross-build environment*, not a release-pinned standalone
ARMHF runtime/sysroot. Debian ARMHF/glibc WebKit must not be installed into the
NuraLoumi ARMv7/musl runtime by ELF-machine equivalence.

### Verified local boundaries

| Boundary | Verdict | Evidence |
| --- | --- | --- |
| QEMU Cortex-A9, NEON disabled, VFP D32 disabled | PASS (smoke only) | `sh components/nura-browser-engine/tests/isa/run_no_neon_qemu.sh`: exit 0, HWCAP NEON=0, `TEGRA2_NO_NEON_QEMU_PROBE=PASS`; fixture source path repaired |
| Focused audit tests | PASS | `python3 -m unittest discover -s components/nura-browser-engine/tests -p test_runtime_closure.py -v`: 14/14 |
| Native Meson | PASS (x86_64 only) | `meson test -C /tmp/nurabrowser-r2-sandbox-build --print-errorlogs`: 7/7 |
| Python syntax and diff whitespace | PASS | `python3 -m py_compile` for scanner/test; `git diff --check` |
| ARMHF WebProcess runtime with sandbox | FAIL / not proven | Image contains ARMHF `/usr/lib/arm-linux-gnueabihf/wpe-webkit-2.0/WPEWebProcess`, but its `/usr/bin/bwrap` and `/usr/bin/xdg-dbus-proxy` are x86-64, not ARMHF. R2 isolated QEMU launch failed at nested namespace creation (exit 134). |
| ARM/Thumb mapping, code/data and 128 historical suspects | UNKNOWN / blocking | No independently reviewed mapping symbols and control-flow classification of all historical suspects; decode-only findings are *not* waivers |
| Full `WPEWebProcess` seeded static DT_NEEDED + heuristic scan | FAIL / incomplete | Pinned Docker scan: 147 ELF files, 0 unresolved DT_NEEDED/interpreter paths, 139 forbidden-file records, 60,498 heuristic decode candidates; 0 dynamic module coverage claims. See diagnostic below |
| Runtime `dlopen` and child-process inventory | UNKNOWN / blocking | No observed ARMHF WebProcess process tree and mapped-ELF inventory |
| Standalone musl or isolated glibc target closure | UNKNOWN / blocking | Current NuraLoumi application uses musl; WPE cross-build uses glibc |
| Real SL101 page pixels/input | OUT OF SCOPE | Separate Gate 3C |

### Static diagnostic

The offline scanner now records *every* heuristic ISA suspect with file SHA256,
ELF path, section, nearest disassembly symbol, address and disassembler decode.
Each entry explicitly remains `arm_thumb=unverified`,
`code_or_data=unverified`, `verdict=unresolved`. This does not claim to
classify the historical 128 findings or to prove instruction reachability.

The static seed can be reproduced with this pinned image and source-mounted
read-only scanner (no network or device access):

    docker run --rm --network none \
      -v /home/user/code/nuraloumi/components/nura-browser-engine/scripts:/audit:ro \
      --entrypoint python3 sl101-nurabrowser-wpe-dev:2.54.0-2 \
      /audit/audit-runtime-closure.py --root / \
      --binary /usr/lib/arm-linux-gnueabihf/wpe-webkit-2.0/WPEWebProcess \
      --full-disassembly

Run result: exit 1 (`status=blocked`, `pass=false`), 147 transitive ELF
files; JSON SHA256 `41aea83c3684d2990fecac6ca465c3ab0306e3dbb8aaa8d8fdac32724ab69545`
(local host diagnostic `/tmp/nurabrowser-d8f5c1-webprocess.json`, 18 MiB).
Among 60,498 *heuristic* candidates are 27,247 in
`libWPEWebKit-2.0.so.1.11.3` and 3,254 in `libwebp.so.7.2.0`.
These counts are **not 60,498 confirmed forbidden instructions**: a naive
objdump mode can misdecode Thumb bytes, and this new full sysroot scan is not
the historical 128-suspect sample. A larger list does not resolve the original
128; exact cross-run identity cannot be asserted without the earlier manifest.

This does *not* seed every `dlopen` module or helper. An engine binary built
outside the image must first be copied into an isolated pinned root, then added
as an explicit seed with exact SHA256. Seed lists and transitive `DT_NEEDED`
alone are insufficient to certify runtime closure.

### Minimal fix/qualification sequence

1. Freeze a reproducible ARMHF/glibc runtime root with package lock/digests,
   ARMHF `bwrap`, ARMHF `xdg-dbus-proxy`, `WPEWebProcess`, loader
   `/lib/ld-linux-armhf.so.3`, engine executable and its transitive libraries.
   Do not reuse amd64 helpers as target evidence.
2. For *each* historical suspect, record ELF SHA256, section, address, mapping
   symbol (`$a`, `$t`, `$d`) and verified branch/function boundary;
   identify data-in-code and genuine forbidden instructions. A stripped mapping
   remains UNKNOWN until independent disassembly/CFG evidence exists.
3. Run the engine in a separately authorized isolated ARMHF glibc environment,
   keeping sandbox enabled. If namespace creation is denied, preserve the
   failure; reproduce with ARMHF helper ELF and kernel/userns permissions, not
   an override of `WEBKIT_USE_SANDBOX` or privilege escalation.
4. Record `/proc/<pid>/maps`, executable hashes, `PT_INTERP`, all
   `DT_NEEDED` edges and observed `dlopen` modules across engine,
   WebProcess, network/GPU processes and sandbox helpers after representative
   navigation/media. Compare observed modules against static seeds and resolve
   every discrepancy.
5. Choose an ARMHF/glibc *isolated* runtime boundary with explicit loader,
   paths and sandbox proof, or produce a genuinely native musl WPE stack.
   Requalify ISA and sandbox closure on that exact target root.

Do not promote this result to Gate 2 PASS while any mapping, module or ABI
origin remains unknown.
