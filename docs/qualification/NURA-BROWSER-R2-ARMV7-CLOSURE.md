# NuraBrowser R2 — ARMv7 no-NEON closure (2026-11-19)

## Verdict: FAIL / incomplete evidence

This is a host-side static qualification, **not** SL101 runtime proof. Never
promote an `attributes-only` scan, a QEMU arithmetic probe, or zero missing
`DT_NEEDED` entries into a complete WPE WebProcess closure.

## Evidence in this run

| Check | Exit | Result |
| --- | ---: | --- |
| `python3 -m unittest discover -s components/nura-browser-engine/tests -p 'test_runtime_closure.py' -v` | 0 | PASS, 13 tests |
| `python3 -m unittest discover -s components/nura-browser-engine/tests -p 'test_*.py' -q` | 1 | FAIL, 72 tests, one pre-existing migration mismatch: `test_explicit_pkg_config_linkage` expects historical `rename: ',browse'` Meson stanza; 2 skipped |
| `python3 -m py_compile components/nura-browser-engine/scripts/audit-runtime-closure.py components/nura-browser-engine/tests/test_runtime_closure.py` | 0 | PASS |
| `git diff --check` | 0 | PASS |
| `sh components/nura-browser-engine/tests/isa/run_no_neon_qemu.sh` | 1 | FAIL to execute: hardcoded `/src/probe_tegra2_no_neon.c` absent on host |
| ARMHF cross-build against pinned sysroot | — | PENDING in this run; prior R1 evidence is not requalification |
| Full sysroot ARM/Thumb mapping classification | — | PENDING; prior 128 conservative suspects cannot be waived |
| WPE WebProcess and `dlopen` module runtime inventory | — | PENDING; no traced target process tree |

## New audit entry point

The existing offline scanner now accepts repeated `--extra-elf` arguments:

    python3 components/nura-browser-engine/scripts/audit-runtime-closure.py \
      --root /prepared/armhf/root \
      --binary /prepared/armhf/root/usr/libexec/nura-browser/nurabrowser-core \
      --extra-elf /prepared/armhf/root/usr/lib/arm-linux-gnueabihf/gstreamer-1.0/libgstexample.so \
      --full-disassembly

These paths are **illustrative**, not an assertion that the module exists.
Every explicit seed must be a regular file within the root; absence and
symlink escapes cause audit errors. The scanner recursively resolves each
seed's `DT_NEEDED` closure and includes the seed paths in JSON evidence.

## Remaining no-NEON proof obligations

1. Obtain reproducible full ARMHF sysroot and exact source/artifact digests.
2. Enumerate executable paths and loaded shared objects for real WPE UI,
   Network/WebProcess and GPU processes using an isolated runtime with
   representative navigation, media and subprocess startup. Include plugin
   discovery paths (GStreamer, WPEPlatform, GIO, graphics), loader search
   roots and the runtime environment in the provenance record.
3. Seed **every** observed executable and loadable ELF; compare static and
   runtime inventories; fail if unknown/unsampled modules remain.
4. For all executable sections, classify ARM vs Thumb using verified mapping
   symbols and actual entry/branch targets. Missing/stripped mapping is an
   unresolved suspect, not permission to discard a forbidden mnemonic.
   Disambiguate data-in-code and unreachable bytes only with reproducible
   decoding/control-flow evidence. Reject true NEON, VFPv4 and D16-D31 uses.
5. Audit hard-float calling conventions, ARM attributes, loader/interpreter,
   PLT and all module boundaries. QEMU Cortex-A9 with `neon=off,vfp-d32=off`
   is a supplementary execution check, not a proof of unexercised code.
6. Repeat under CI's pinned container layout and, separately, on the SL101
   without replacing the user's live session. Preserve gate as FAIL until all
   requirements have recorded PASS evidence.

### Caveats

The current `objdump -d` heuristic conservatively flags plausible NEON
instructions but does not prove that disassembler mode and control-flow
reachability are correct. An `--extra-elf` list supplied by a caller is not
proof of complete dynamic inventory. No security check is waived by this work.
