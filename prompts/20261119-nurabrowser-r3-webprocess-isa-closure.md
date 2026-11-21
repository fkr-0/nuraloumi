# Finish the missing NuraBrowser ARMv7 no-NEON WebProcess runtime closure evidence

## Goal
Resolve the specific unfinished Gate 2 proof from NURALOUMI-20261119-NURABROWSER-R2-ARMV7-CLOSURE-20261119-868313. Do not rerun a generic R2 audit and declare success; classify the 128 historical undecided ARM/Thumb ISA suspects and inventory runtime dlopen/WPEWebProcess modules against a pinned ARMHF sysroot.

## Context
- Prior R2 wrote components/nura-browser-engine/scripts/audit-runtime-closure.py and tests/test_runtime_closure.py; 13 focused unit tests passed, but the final gate correctly remained FAIL because 128 suspect disassembly regions were not individually classified, and runtime module inventory including WPEWebProcess and sandbox helpers was missing.
- NuraLoumi Rust app is ARMv7/musl, whereas the current NuraBrowser engine cross-build uses Debian ARMHF/glibc WPE 2.54.0/2.54.2. Do not treat ELF architecture sameness as libc compatibility or silently copy glibc modules into musl root.
- Read docs/qualification/NURA-BROWSER-R2-ARMV7-CLOSURE.md, docs/qualification/NURA-BROWSER-R2-SANDBOX-RUNTIME.md, components/nura-browser-engine/packaging/Dockerfile.ci-armhf and scripts/ci-build-nurabrowser-engine.sh.
- Host x86_64 WPE sandbox headless PASS is not ARMv7 runtime PASS; real Wayland page pixels are owned by a separate Gate 3C follow-up.
- Current runtime must never set WEBKIT_USE_SANDBOX=0 or bypass bwrap/xdg-dbus-proxy fail-closed checks.

## Scope
- In: bounded offline code/tests and diagnostics of full ARMHF sysroot ELF DT_NEEDED, PT_INTERP, processor ISA classification and observed dynamically loaded module/process tree, with safe read-only target interrogation if access is available.
- Out: browser UI/tabstrip/supervisor sources, compositor, any live installation, global EGL/Mesa, device mutation, Git push, and weakening ISA filters.
- Coordinate exact path claims and preserve R2 artifacts. No declaration of PASS if any origin/ISA/module is unknown.

## Acceptance
1. Report 128 suspect instructions with individually reproducible ARM vs Thumb mapping/function/section context, distinguish code from data; never waive a forbidden opcode merely because QEMU executed another path.
2. Record exact WPE engine, WebProcess, bwrap, xdg-dbus-proxy and dlopen module SONAME/file/hash/ABI closure for the target root; explicitly account for process interpreter and transitive dependencies.
3. QEMU no-NEON ISA smoke and full static no-NEON audit have documented pass/fail results; use available pinned sysroot and do not invent target runtime if absent.
4. If private ARMHF glibc root cannot run the WebProcess sandbox, retain FAIL and provide a minimal concrete reproduction/fix path, no sandbox disable.
5. Run focused Python audit tests, native Meson tests and syntax checks, document exact remaining blockers. Flag whether a native musl WPE build or isolated glibc runtime is required.

## Deliver
- Reviewed patch/test/evidence as one narrow lane, commit if coherent; no push.
- Durable final agent_checkpoint with clear PASS/FAIL/UNKNOWN for every closure boundary and self-contained follow-ups.