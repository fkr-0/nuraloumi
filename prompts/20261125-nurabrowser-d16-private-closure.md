# Close the private ARMv7/musl WPE 2.54.2 runtime closure

## Goal
Produce a separately staged, reproducible, fail-closed WPE 2.54.2 runtime for the SL101 Nura. Do not replace the working canary or promote any release until its ELF, ISA, plugin and actual loader-map evidence is complete.

## Current evidence
- Baseline NuraLoumi HEAD includes commits `62bb575` (read-only live-runtime auditor and tests) and `9284b5b` (device findings); inspect current HEAD rather than hard-coding an old checkout.
- Read `docs/qualification/NURABROWSER-LIVE-WAYVNC-20261125.md` and `docs/qualification/NURABROWSER-LOCAL-MUSL-CANDIDATE-20261125.md` before touching any runtime tree.
- The live candidate was WPE WebKit 2.54.2, ARMv7/musl; an audit of 92 files/89 ELF/82 statically reachable reported 30 blockers. Private `libLLVM.so.23.1`, `libdrm.so.2`, several XCB/X11 libraries, SONAME aliases and duplicate Wayland providers need resolution. Device process maps showed external-loader fallback. Original bundled crypto had non-D16/NEON attributes, while a separately built source-verified D16 crypto overlay exists.
- The exact pinned source and D16 overlay hashes are in the qualification documents. Treat those documents and observed artifact hashes as authoritative; fail if provenance cannot be verified.

## Scope
1. Reconstruct and document the build inputs for a NEW immutable staging root. Bring the D16 crypto overlay, WebKit helpers, musl loader, ICU 76, GIO/EGL/GStreamer modules and transitive libraries under explicit private dependency control. Verify all ELF class, machine, float ABI and VFPv3-D16 constraints.
2. Resolve missing `DT_NEEDED` entries and collisions without hiding them through host fallback; materialize valid SONAME symlinks/aliases. Produce source IDs, SHA256 manifest, exact staging recipe and a machine-readable loader map.
3. Run `python3 -m unittest scripts.test_inventory_nurabrowser_live_runtime scripts.test_audit_nurabrowser_wpe_candidate scripts.test_assemble_nurabrowser_wpe_d16_candidate -v` and the live inventory checker on the staged tree. Compare static resolution with bounded, supervised real process `/proc/maps` only when the runtime is ready.
4. Report every unresolved dependency, plugin load failure or unsafe ISA attribute explicitly. Do not falsely turn ambiguous Thumb instructions into a pass.

## Boundaries
- Use authenticated projmgrauth, one workspace, AGENTS.md, claims and project notes. Preserve all unrelated edits.
- No recursive subtask dispatch, no automatic successor fan-out and no other repository changes outside this closure.
- Do not mutate the installed canary, `/opt/nuraloumi/current`, Sway, WayVNC, power state or networking. No remote Git push, CI trigger, reboot, deletion, or silent production promotion.
- Host/device commands must be bounded and reversible, and must preserve the original runtime for rollback.

## Acceptance and delivery
- All reachable `DT_NEEDED` and modules resolve uniquely inside the staged root or have an explicit reviewed exception; loader/map provenance proves no accidental host fallback.
- All binaries and dynamically loaded modules satisfy the ARMv7/musl, no-NEON/D16 contract; hashes, source IDs, exact commands and test exits are recorded.
- A stable, reproducible staging artifact and updated qualification report exist. If any assertion fails, mark `UNQUALIFIED` and checkpoint as blocked.
- Deliver focused commit(s) or reviewable diff, exact test outputs and `agent_checkpoint` with the injected OCP identity. No launch/promotion beyond this gate.
