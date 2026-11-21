# SL101 Nura .106: functional NuraLoumi release gate — 2026-11-21

**Decision: NO-GO for activation.** The owner confirmed the familiar Nura SL101 SSH alias for .106, and explicitly reports that the previously deployed NuraPlayer and NuraBrowser did not work. This report does not claim an actual physical repair or touch the device.

## Source integration

The working tree originally had unrelated shell/Sway functionality, archived OCP prompt policy updates, Cairo/Fontconfig diagnostic fixtures, qualification notes and a release verifier under an independent agent claim. Changes were reviewed and grouped into local, path-specific commits, preserving the ongoing claim. Source and Rust tests were checked locally; **no Git push, remote CI trigger, CI-source promotion or device deployment** was performed.

- `45317c5`: Super solo App Overview / workspace-chord handling, Sway binding config and browser UI qualification note (host unit tests PASS, not keyboard/device proof).
- `4c77b4a`, `7ac05e7`, `9f4dc1c`, `5544883`, `7fec73a`: legacy OCP prompts and finite fix-closure campaign provenance; these do not constitute execution or dispatch.
- `d53f119`: historical browser/CI and 339-item triage ledger plus Cairo terminal-lifetime diagnostic (not a product memory leak fix).
- `abc985b`: make the stale private-video power-policy test assert the existing AC/battery/root-lock guards, while passive userspace release remains threshold-free. No device safety guard was removed.
- `7f04a0c`: fix-closure lane 07's exact-version and complete-tar/ELF CI artifact verifier, regression tests, packaging README and ARM gate note, integrated after its independent claim expired. All 22 scoped package tests passed; the already-fetched real `fb47cfc` artifact passed the updated offline validator at its correct version, and an intentionally wrong expected version failed closed.

**Last code/verifier integration commit:** `7f04a0c3bb3375ae5f66af5c683f8a80614ddce8`; subsequent report-only commits change Git HEAD but not binaries. The actual final HEAD must be frozen anew before any approved production CI run.

## Verified host checks

| Gate | Result |
| --- | --- |
| Rust workspace tests, locked | PASS |
| Rust workspace Clippy, all targets, `-D warnings` | PASS |
| Launcher + workspace switcher tests | 24/24 PASS |
| Browser offline Python contracts | 76 tests, 2 fixture skips; no live pixels |
| Native media audio/geometry/EOS bus regression | PASS; fixture/host only |
| Package verifier, integrity and binary unit tests | 22/22 PASS |
| Composite/userspace/browser/video release tests after stale AC assertion fix | 32 tests PASS, 1 skip |
| Browser nested Wayland `--probe` | SKIP/exit 77: no sway/labwc, grim, wtype or engine binary in local test environment |
| ARMv7 real browser/player E2E on the current tablet | NOT RUN |

## Critical binary evidence — last successful official package

- Official Alpine/musl workflow `sl101-musl-armv7.yml`, successful run `44542054386`, source `fb47cfc73c614440dcbf6aecb12642085fef796a`, version `0.0.8-rc.1`. The previously downloaded original CI archive and native APK passed the updated **offline** exact-SHA/version verifier.
- Direct inventory of its verified `nuraloumi-ci.tar` found `bin/nuraloumi-player`, `bin/nuraloumi-playerd`, `bin/nuraloumi-browser` but **no** `lib/nuraloumi/video/` private producer bundle and **no** `libexec/nuraloumi/browser/nurabrowser-core`. The successful shell-binary package therefore cannot by itself supply qualified GPU playback or a complete native WPE browser.
- The separate `nurabrowser-engine-armhf` CI source marks `web_process_sandbox=unqualified`, `dependency_instruction_closure=unqualified`, and `device_deployment=forbidden`; its ARMHF/glibc engine is not a drop-in musl component.
- `NURA-BROWSER-R3-WEBPROCESS-ISA-CLOSURE.md` records the actual WebProcess ISA, glibc/musl, sandbox-helper, dynamic module and ARMHF closure gaps. A host test PASS cannot clear them. Earlier private 0.0.7 Grate player 414-frame physical evidence does not qualify a later incomplete 0.0.8 package.
- The latest locally integrated source is newer than `fb47cfc` and has **no successful official same-SHA package CI**. Even a correctly re-staged older artifact would omit the newly committed changes.

This evidence establishes missing package/runtime prerequisites, not the complete live-device root-cause stack or a repaired player/browser.

## Mandatory next release path

1. **Freeze exact source:** complete independent agent claim reconciliation, test the final clean commit and decide the next explicit version. Do not silently conflate local `main` with official CI source.
2. **Prove complete NuraPlayer artifact:** pin producer source + exact SHA of GStreamer private `libgstv4l2codecs`, Grate/Mesa, libdrm, `libnuraloumi-video.so`, ARMv7 musl ELF/loader/SONAME/plugin closure and native playerd; use `verify_sl101_video_bundle.py`, plus actual 854x480/24fps device proof. Generic software-only CI must remain valid as such, but it is never a complete-video release.
3. **Prove complete NuraBrowser runtime:** independently build and qualify the exact ARMv7 no-NEON WPEWebProcess + sandbox helpers + engine, with an explicit musl or properly isolated glibc loader/runtime policy. Gate 2/3C/5, real page pixels, touch and navigation must PASS; do not use a nondeployable ABI-smoke tar.
4. **Official artifact:** the owner's Nura `.106` deploy request already authorizes the ordinary reviewed `main` push and official ARMv7/musl CI needed for this release; no repeated confirmation. Obtain a successful artifact for the **same** reviewed source SHA/version, verify exact native package bytes/provenance; never ship local host build outputs.
5. **Identity/recovery:** use the owner's established `.106` SSH target with strict known-host-key checking for a read-only preflight; no physical serial sticker exists or is required. Compare current SD/eMMC CIDs, model, boot ID, kernel/DTB, live Sway/Pixman baseline, free space, battery/AC, service lock and rollback to the expected Nura inventory before writes. The owner has already confirmed routine recovery SD and physical recovery availability; inspect actual rollback hashes without repeatedly requesting that confirmation.
6. **Stage then canary:** copy verified artifact to a fresh inactive unprivileged versioned directory; verify hashes and dynamic closure. Launch isolated browser/player without switching defaults, killing Sway, global pkill, reboot, kernel/Mesa replacement, or flash. Preserve the old binary/Pixman recovery.
7. **Functional accept/reject:** confirm real browser pixel/navigation/touch and WebProcess survival; then player/video pixel checks, 414-frame/AAC/EOS where applicable, pause/seek/volume, no SIGILL and zero outstanding DMA-BUF/GBM lifetimes. Promote atomically only after explicit owner-reviewed GO. If any part fails, retain/restore prior release and record raw logs.

**No deploy was executed on .106 in this integration review.**

## Subsequent source-only GPU package closure

The next local follow-up found a producer/consumer mismatch: the GPU consumer
accepted a hash-enumerated `gst/` dependency closure but the packager only
copied `gst/libgstv4l2codecs.so`, potentially dropping required private
GStreamer modules. The packager now stages the complete verified `gst/`
directory. A dedicated offline verifier checks that the **final GPU tar**,
not merely its source bundle, retains exactly all producer-hashed files and
the reviewed NuraLoumi SHA/version/ARM ABI. This does not create the missing
independent CI producer artifact, establish an official successful same-SHA
release run, or qualify the tablet. This addendum describes subsequent code;
the earlier 12-commit integration count above is historical, not current HEAD.

## Owner release workflow clarification — 2026-11-21

The owner explicitly corrected three unsuitable procedural assumptions:

- Nura SL101 `.106` has **no physical serial sticker**. Its identity is
  established by the trusted SSH host key, read-only current model/SD/eMMC
  CIDs, boot/kernel/DTB and the established device inventory. Read-only SSH
  is permitted before obtaining these live facts.
- The recovery SD and physical recovery route are standing owner context;
  do not keep requesting a new affirmation. Still verify a usable rollback
  snapshot and preserve the Pixman/Sway baseline before changing anything.
- A request to deploy a reviewed NuraLoumi release includes the ordinary
  upstream push and official CI workflow for its exact revision. Those steps
  require no separate repeated owner question, but release artifacts must
  still pass exact-SHA/ABI/native-runtime checks before staging.

These corrections remove **procedural** blockers only. They do not claim a
working player, WPE browser, complete GPU producer closure, verified CI
artifact, or tested hardware rendering where none has been demonstrated.

## First successful read-only SSH preflight

After the owner corrected the unavailable-sticker requirement, the known
`.106` SSH host key was found in the host's trusted inventory and the
connection succeeded without disabling host-key checks. The live device
identified as **ASUS Eee Pad Slider SL101 / Nura edge**, with
`7.0.1-postmarketos-grate-dmavde1`; the observed SD and eMMC CIDs match
the historical Nura inventory. Current boot identity was collected privately;
the root filesystem reports ~89.6 GiB and 7.66 million inodes free.

The active Sway session and `nuraloumi-panel --live` remain running.
`/opt/nuraloumi/current` resolves to the `fb47cfc` release's `bin/` path.
Unlike the original CI archive, this *installed device release* has an
additional `lib/nuraloumi/video/` directory with private Grate/Mesa and VDE
GStreamer modules plus `libnuraloumi-video.so`. It has **no embedded producer
`provenance.json`**; therefore do not claim this sidecar meets the new
source-verified runtime contract. The required historical system libdrm
directory and serialization shim were present. The native browser engine
under the release's `libexec/nuraloumi/browser/` path was **absent**.

This corrects the earlier assumption that missing modules in the downloaded
CI archive necessarily imply missing player libraries *on the tablet*.
The precise player runtime failure is not yet diagnosed; no playback,
compositor, default symlink or device files were modified by this preflight.
