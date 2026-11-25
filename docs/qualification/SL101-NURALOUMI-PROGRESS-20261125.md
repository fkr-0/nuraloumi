# SL101 / NuraLoumi progress and qualification ledger — 25 November 2026

## Executive decision

**Production RC2 qualification: NO-GO.** Source-side UI repairs and a private 414-frame VDE/GPU reference exist, and official musl push CI for a previous source revision is green. None proves a complete source-matched musl browser/GPU package installed and working on the SL101. This report is not a deployment authorization.

The primary target is the owner-designated ASUS SL101, postmarketOS Nura ARMv7/musl with Sway; the device's private SSH address remains local to the owner's inventory. The separate Debian ARMHF/glibc target and its dynamic loader must not be conflated with the musl package. See docs/architecture/SL101-NURALOUMI-SYSTEM-MAP-20261125.md for the five rendered Mermaid architecture/release diagrams.

## 1. Consolidation and worker status

Repository: NuraLoumi project root, working on main; earlier dirty/preservation worktrees were reconciled. As checked this morning, git worktree list contains only the main project directory, not an extra .ws-bridge/worktrees checkout. The local branch has advanced beyond the official pushed SHA; its worktree state and commit should be rechecked immediately before any release packaging.

Nine bounded ChatGPT ABC OCP tasks were accepted and dispatched; all nine now have terminal complete worker checkpoints. These checkpoints reflect scoped host/source work, not physical SL101 qualification. All workers were explicitly told not to deploy/reboot, publish, or touch the parent-owned WPE/VDE engine.

| OCP short receipt | Task | Local evidence | Physical tablet |
| --- | --- | --- | --- |
| 255b0 | Launcher geometry | c415f68: clip menu to available viewport; host qualification | Await screenshot |
| bbaa7 | Wheel/touch menu scrolling | 1edd14a: multi-touch cancel/scroll gesture tests; worker 207/207 PASS | Await swipe/wheel |
| f1385 | Font-free Control Center category icons | aa52019: Cairo raster/bounds tests, 448/280px, dark/light, 1×/2× | Await pixel proof |
| 9510d | Notification delivery and usage | 8d39576: CLI/D-Bus documentation, worker 24 focused host tests + private bus smoke | No device E2E |
| 269bf | Exact-source RC2 package/CI/SSH gate | 1dc9c99: independent NO-GO audit and rollback prerequisites | SSH denied |
| 0728c | Slider open/closed notifications | 65691eb: opt-in debounced observer with initial-state suppression | Hardware input and polarity unknown |
| f5476 | Nerd Font selection/coverage | 34a6152: Terminess 3.5.1 local cmap, 35/35 listed PUA glyphs and fallback API | Default render path + package asset not integrated |
| cacab | History receives notifications, no floating banner | a0f4287: allow toast when center state is unknown; added child lifecycle diagnostics; worker 22/22 tests | Actual toast pixels unknown |
| ad734 | Super+Shift+R kills panel, fails restart | NuraLoumi 041b5fc and sl101_linux 4be1c4f, 3fff958, 280be10: direct restart binding, readiness-checked hook, tests; sibling make test 2641 total / 11 skipped, exit 0 | No live hotkey/restart |

The original invalid icon prompt was rejected by the OCP placeholder validator before delivery; the corrected f1385 task was dispatched and completed. That invalid queued row is not evidence of an additional worker.

## 2. Parent-owned browser and media-player work

| Area | Source/host result | Limitation |
| --- | --- | --- |
| Native VDE renderer errors | 9b9284b: player now carries specific GStreamer, /dev/dri/renderD128, GBM, EGL, Grate shader/vendor and Wayland stream setup errors through existing status; null GL vendor guarded | New binary not installed on .106; exact failing device stage unknown |
| Native media host build | components/nura-media-engine/build.sh produced libnuraloumi-video.so for host x86-64; media/audio regression script passed | Not ARMv7/musl compilation, VDE pixels or sound |
| Browser supervisor/scene | cargo test targeted browser_scene_layout 5/5 and browser_supervisor_lifecycle 6/6 PASS | No real WPE WebProcess or Wayland pixels |
| WPE source tooling | Four Python suites (decoded ISA, candidate ELF audit, D16 assembly, runtime verifier) 38/38 PASS | Synthetic/host contract tests, no full private closure PASS |
| Historical hardware reference | 854×480 H.264/AAC, 414 frames, 414 compositor callbacks/releases and 23.59fps from private no-install NuraLoumi player integration | Does not qualify active release package, current system Mesa, or a different clip |

The owner reports that the installed player has never worked from the normal application. Do not describe the issue as a regression in a previously working production player.

## 3. Official CI and release artifact truth

Observed via authenticated GitHub CLI on 25 November:

- Last listed successful musl push run on release/0.0.8-rc.2: **45545537613**, source **623609cbe4413af3313682b9478bb505a883510d**. The companion main run **45545530071** used the same source.
- The published source precedes UI fixes beginning at 95bb56bc and the subsequent worker commits; today's local changes have **not yet inherited an official CI PASS**.
- At that GitHub revision, the musl job artifact contains three text files: BUILD-LOG.txt, BINARY-LIST.txt, ELF-AUDIT.txt. The worker independently found 28 expected names and matching PASS rows, but actual ELF bytes were absent; it is not a deployable tar/APK.
- The WPE 2.54.2 Alpine ARMv7/musl producer has a Docker/Ninja candidate build path. Its six-hour QEMU run previously exceeded the job window and the existing successful local candidate remains marked UNQUALIFIED. A same-commit WPE runtime.json with independently proven loader, sandbox, dlopen and WebProcess closure is missing.
- The GPU private video-bundle producer must supply exact source/run/archive hash and all Grate/EGL/GBM/DRM/GStreamer/VDE modules. Contract references to Gallium 25.0.7 and observed device system Gallium/Mesa 26.2.4 must be resolved by source and SONAME inspection; no version substitution is automatic.
- Manual package promotion requires the exact-source qualified WPE producer and optionally the separately verified GPU producer. A normal green push does not activate this gate.

### Local CI changes prepared during this continuation

Paths: .github/workflows/sl101-musl-armv7.yml, scripts/test_musl_ci.py, docs/qualification/MUSL-GITHUB-CI.md.

1. Repaired four erroneous doubled shell continuations in the GPU APK Docker invocation; previously the combined GPU APK step would parse as broken commands.
2. Added a push-only staging step for **actual ARMv7/musl Rust ELF bytes**, their SHA256 inventory and a source/target marker declaring UNQUALIFIED, no WPE, no GPU bundle, deployment FORBIDDEN. This is explicitly an inspectable binary audit artifact, **not** a device release.
3. Preserved release workflow_dispatch source/manifest/hash requirements, dedicated WPE and GPU checks, and safety/fail-closed semantics.
4. Added regression tests for the real-ELF audit artifact and GPU APK line continuations, including executing the actual CI staging shell block against isolated fixture binaries. Focused scripts.test_musl_ci **9/9 PASS** and actionlint for the workflow **PASS** on this local checkout.
5. Updated the previously inaccurate docs that claimed every main push uploaded a full tar/APK.
6. In the separate WPE candidate workflow, bounded Docker/QEMU at 5h30 within the six-hour job and retained BUILD-LOG.txt plus BUILD-STATUS.txt on failure or timeout. This preserves the actual Ninja failure stage without mislabeling any partial tree as qualified. New producer CI contract tests **4/4 PASS**; actionlint **PASS**.
7. Combined musl/WPE/runtime/GPU/native-package regression suites: **64/64 PASS**; cargo fmt --check and both GitHub workflow actionlint checks PASS. All five Mermaid diagrams in the architecture document rendered successfully with Chromium.

**Next CI checkpoint:** push a reviewed exact main commit, wait for a new official run, download that new artifact, recompute the new SHA files, independently inspect all 28-ish ELF objects. This is still short of a complete musl WPE/GPU release. The producer build-time and runtime qualification blockers must then be tackled independently.

## 4. Device login and hardware gate

A previously denied SSH attempt is superseded by a successful, read-only login on 25 November:

    ssh -o BatchMode=yes -o ConnectTimeout=7 -o NumberOfPasswordPrompts=0 SL101_ALIAS 'uname -m; id -un'

returned **exit 0**, `armv7l` and the expected unprivileged account. The kernel identifies as `7.0.1-postmarketos-grate-dmavde1`; Sway is running. The installed NuraLoumi symlink still targets the prior `623609c` software-only release. No new binary has been installed or activated by this qualification.

The panel is not running after the reported Super+Shift+R restart failure. Its local log shows repeated `cannot bind panel IPC` / `Address in use` errors; diagnosing the stale or active socket owner is separate from the source-matched CI gate. A live socket path by itself is not evidence of an active listener. No blind socket deletion or Sway restart has been attempted.

Pending remaining read-only/physical device evidence:

1. Verify exact boot, kernel/DTB, active /opt/nuraloumi/current symlink, binaries and private library hashes, Sway PID, relevant render/Wayland sockets and loader environment.
2. Confirm that the privately staged GPU video module/Grate/GStreamer version is the one the normal player actually dlopens; use the new status error to identify the failing stage.
3. Run source-matched WPE private sandbox/page canary under bounded resources, capture WebProcess exit/signal, actual dlopen/Mesa/LLVM mappings and a real Wayland screenshot.
4. Re-run menu y-coordinate/clipping, scrollbar wheel/touch gestures, five category icons, notification toast versus history, physical slider switch polarity and Super+Shift+R supervised restart.
5. Only after matching release manifest, rollback hashes, ISA/ABI checks, device memory/thermal measures and owner sign-off consider a versioned canary/activation.

## 5. Ordered blockers and next actions

| Priority | Owner/task | Gate to close |
| --- | --- | --- |
| P0 | Read-only SSH restored; complete runtime inventory | Current boot and active software-only release identified; inspect rollback hashes, WPE/Grate/GStreamer and session consistency before mutation |
| P0 | WPE runtime producer | Finish a CI-capable ARMv7/musl 2.54.2 build, full library/dlopen/sandbox/ISA coverage, real WebProcess/Wayland pixels; create independently reviewed runtime.json only after PASS |
| P0 | Private GPU producer | Exact-source Grate/Mesa/EGL/GBM/DRM and GStreamer/VDE archive, verified SONAME/ELF closure and correct version contract |
| P1 | Review/push local CI+UI source | New official exact-SHA musl CI, download actual ELF audit bytes, independent digest and ARM instruction scan |
| P1 | Qualified tar/APK | Manually dispatch only after producer SHA inputs are available; verify package-check and signed development APK |
| P1 | SL101 canary | Preserve rollback, stage inactive release, run 414-frame player and full browser/UI/notification tests, repeat no-SIGILL/restart/thermal checks |
| P2 | Nerd Font/slider | Integrate licensed font asset/explicit renderer and identify physical slider input after a safe read-only sensor inventory |

As of this ledger update, no new production deployment, reboot, release tag or public GitHub push has been performed.
