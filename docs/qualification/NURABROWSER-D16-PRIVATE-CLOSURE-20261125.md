# NuraBrowser D16 private runtime closure — 2026-11-25

OCP task: `NURALOUMI-20261125-NURABROWSER-D16-PRIVATE-CLOSURE-20261125-68938E`

## Decision

**UNQUALIFIED / BLOCKED.** No new runtime staging root was assembled, no
`runtime.json` was emitted, and no live canary, compositor, device power,
network configuration or production installation was changed.

The authoritative source-pinned producer described in
`NURABROWSER-LOCAL-MUSL-CANDIDATE-20261125.md` is
`7e18074746b395f8245c312736063d0bd38324e5`, with original candidate
tar SHA256
`5c43c2ad85bdb3321723e1c85432d067bb836fbed65c8ac1ccfc3f32137c25ff`.
The source-verified D16 overlay's six replacement hashes and source revisions
are recorded there. The documented D16 candidate tar SHA256 is
`80f11c94ce065b72cdb07ead3441db35cc7fd743de61b18d8ed9515d722e39df`.
These are provenance pins, not evidence that the corresponding local inputs
were recovered in this run.

A locally visible, **different** producer directory
`/tmp/nuraloumi-wpe-45595341644` contained a
`CANDIDATE-ONLY.txt` with source SHA
`844dc30c7885027d06ef3940dcf402607c981720`; its tar SHA256 was
`8444a91b6d92b5f08589898b9cb8d0f0fd563ad0f225a2bfd87569c43e37fbcd`.
Neither identity matches the documented required producer. Do not relabel
this tree as the pinned candidate or silently substitute its runtime closure.

No `REPLACEMENTS-SHA256SUMS.txt` was found under the inspected local
`/tmp` scope. A limited scan under `/home/user` did not locate the required
original or overlay artifacts; access-denied directories were not traversed.
This is **not proof of global absence**. The local temporary tree also lacks
the marker at its `runtime-root` directory, so the live inventory checker
properly rejected it.

## Reproducible commands and observed exits

| Command | Exit | Observation |
| --- | ---: | --- |
| `python3 -m unittest scripts.test_inventory_nurabrowser_live_runtime scripts.test_audit_nurabrowser_wpe_candidate scripts.test_assemble_nurabrowser_wpe_d16_candidate -v` | 0 | 17 tests, all OK |
| `python3 scripts/inventory_nurabrowser_live_runtime.py /tmp/nuraloumi-wpe-45595341644/runtime-root` | 2 | `WPE_LIVE_CLOSURE=ERROR: missing regular CANDIDATE-ONLY.txt` |
| `sha256sum /tmp/nuraloumi-wpe-45595341644/nurabrowser-wpe-musl-candidate.tar` | 0 | `8444a91b6d92b5f08589898b9cb8d0f0fd563ad0f225a2bfd87569c43e37fbcd` |

The 17 passing tests prove the synthetic auditor/assembly guardrails, **not**
actual ARMv7 process admission or complete private loader closure.

## Mandatory gates still open

1. Recover the exact pinned 123,043,840-byte original tar, independently verify
   its hash and `source_sha`, plus the original unqualified marker.
2. Recover the source-verified D16 overlay and independently recheck each entry
   in `REPLACEMENTS-SHA256SUMS.txt`, including all five paired SONAME libraries
   and the OpenSSL legacy module. No hash mismatch may be waived.
3. Assemble a **new, nonexisting** output root through
   `scripts/assemble_nurabrowser_wpe_d16_candidate.py`; never write into the
   active canary or `/opt/nuraloumi/current`.
4. Materialize and pin the private musl interpreter, ICU 76 data and libraries,
   GIO/EGL/GStreamer plugin closure, and `libLLVM.so.23.1`,
   `libdrm.so.2`, X11/XCB and Wayland providers with deterministic private
   precedence and SONAME aliases. Do not silently resolve host libraries.
5. Re-run the static ELF/attribute/needed audits and the live checker against
   the correctly marked new root. Emit a hash manifest, exact source IDs, recipe
   and machine-readable loader map. Resolve every conflict or record an
   independently reviewed exception.
6. The whole-runtime decoder remains **BLOCKED** on ambiguous mixed ARM/Thumb
   modes: the earlier 809,154 unknown decodes and 39,959 suspected incompatible
   uses are **not proven illegal instructions** and cannot be converted to a
   pass. Qualify all plugins and generated-code paths independently.
7. Only when the private closure is ready, run bounded/supervised target helper
   processes and correlate `/proc/<pid>/maps` with `mountinfo` and inode
   provenance. Prior observed external `libLLVM` / `libdrm` loads remain
   unresolved. Do not touch the existing running canary for this experiment.

No release, package promotion, reboot, remote push or production deployment
is authorized by these observations.
