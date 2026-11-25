# WPE 2.54.2 ARMv7/musl local producer audit — 2026-11-25

This is **private-candidate evidence**, not a production runtime qualification.
The existing live device installation and software-rendering fallback remain
unchanged. No `runtime.json`, qualified release package, tag, or deployment is
authorized by this report alone.

## Artifact and source provenance

- The earlier Docker/Alpine ARMv7 relink completed with exit status 0 at
  2026-11-24 21:30 CET after resolving the WebKit gettext/libseccomp linker
  failures. CMake/Ninja linked WPE WebKit 2.54.2, and Meson/Ninja linked
  `nurabrowser-core`.
- Candidate archive size: 123,043,840 bytes. SHA-256:
  `5c43c2ad85bdb3321723e1c85432d067bb836fbed65c8ac1ccfc3f32137c25ff`.
  Its stored checksum and all staged-file checksums were independently checked.
- Build source SHA (retain unaltered):
  `7e18074746b395f8245c312736063d0bd38324e5`.
- RC.2 integration source SHA:
  `41a901b99cd142009f7e603c8440264d3201d6d5`.
- The complete `components/nura-browser-engine` Git tree is identical in both
  revisions: `cd1eaee884b5fcd068f92bd97fb5d4a7b6451d8c`.
  This proves engine *content equivalence*, not that the 2026-11-24 Docker job
  ran against the later whole-repository commit. A release producer must
  record both identities truthfully or rebuild at the exact release revision.
- The original local `main` checkout remains divergent and dirty. The RC.2
  integration worktree starts from the current upstream release baseline;
  the owner's uncommitted changes have not been reset, stashed, or overwritten.

## Independent private-closure preflight

The private assembled candidate contains 85 regular files, 80 of which have
ARM ELF headers. Its direct ELF `DT_NEEDED` closure has no missing library
filenames within the private root. It contains WPE processes, `bwrap`,
`xdg-dbus-proxy`, the musl interpreter, GIO module seeds, and ICU data.

Run the repository auditor with the assembled private root and exact release
commit, for example:

    python3 scripts/audit_nurabrowser_wpe_candidate.py "$CANDIDATE_ROOT" \
      --release-sha 41a901b99cd142009f7e603c8440264d3201d6d5 \
      --output "$REPORT_PATH"

The run on the actual candidate returned exit status **1**, indicating
`WPE_CANDIDATE_AUDIT=BLOCKED files=85 ELF=80 issues=6`:

| Library | ELF attribute | Strict Tegra20 outcome |
| --- | --- | --- |
| `libcrypto.so.3` | VFPv3 + NEONv1 | FAIL: not D16-only |
| `libnettle.so.8` | VFPv3 + NEONv1 | FAIL: not D16-only |
| `libgcrypt.so.20` | ARMv8 FP + NEON for ARMv8 | FAIL: wrong ISA |

All three come from the Alpine 3.22 ARMv7 dependency set. Decoding these
libraries also finds upper VFP or NEON instructions. The particular disassembly
positions are not proof that those paths execute on Tegra20, but the published
package's strict ISA closure requires a compatible binary inventory; optional
CPU dispatch cannot simply be assumed safe.

The original staged archive holds WPE project outputs and headers, **not** the
complete private dependency closure. The separately assembled candidate is
still explicitly `runtime_qualification=UNQUALIFIED`, with normal device
deployment forbidden. A private page-rendering canary must not be confused
with full regular-bundle validation.

One earlier target GDB trace stopped on `SIGILL` at an anonymous executable
mapping, with a system LLVM shared object present elsewhere in the process
maps. This is consistent with an unqualified generated-code/renderer path,
but the precise generating component has not been proven. A separate headless
attempt also failed surfaceless EGL initialization. Neither trace is a
production pixel/sandbox PASS; keep Mesa/LLVM path qualification independent.

## Remaining release gates and next actions

1. Rebuild or obtain *reproducible, ABI-compatible* non-NEON ARMv7 hard-float
   variants of the exact `libcrypto3`/OpenSSL, `nettle`, and `libgcrypt`
   dependencies; pin and verify their package/source revisions and record any
   upstream assembly-dispatch configuration changes. Do not replace live device
   libraries or merely strip ELF tags.
2. Reassemble the complete private musl runtime with these replacements, then
   re-run the fast ELF, interpreter, `DT_NEEDED`, glibc-dependency, and
   VFPv3-D16 attribute audit. Recheck all hash and SONAME relationships.
3. Run **full decoded** ARM/Thumb/NEON/hardware-division and upper-register
   inspection, including dlopened libraries, Mesa drivers, LLVM and sandbox
   helpers. The current fast auditor does *not* certify this gate.
4. Independently test normal sandbox behavior, WebProcess and NetworkProcess
   lifecycle, real Wayland pixels, navigation, keyboard/touch, and reliable
   restart on the target. Previous private tests do not cover the entire gate.
5. Resolve the build-versus-release commit identity with honest source
   equivalence attestation or a release-revision producer; then generate a
   fully hashed `runtime.json` with verified gate results. Only after this
   succeeds may the official CI artifact, regular tar/APK, and production
   deployment proceed.

Status: **NO-GO** for production. Keep the existing software-rendering
fallback and rollback hashes intact.
