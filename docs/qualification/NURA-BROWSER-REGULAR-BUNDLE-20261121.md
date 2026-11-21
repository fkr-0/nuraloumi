# NuraBrowser ordinary release bundle — implementation boundary (2026-11-21)

OCP: `NURALOUMI-20261121-NURABROWSER-REGULAR-BUNDLE-20261121-486BDD`

## Decision: producer BLOCKED; package contract FAIL CLOSED

The Rust `nuraloumi-browser` executable is the UI/launcher, not a WPE engine.
Ordinary `armv7-unknown-linux-musleabihf` and `armv7-unknown-linux-gnueabihf`
xtask packages now require `NURALOUMI_BROWSER_RUNTIME_DIR` and a pinned
40-hex `NURALOUMI_BROWSER_PRODUCER_REVISION`. `package` validates the input
tree against the current NuraLoumi Git SHA and target, copies the *entire*
tree to `libexec/nuraloumi/browser/`, then validates the staged copy. The
normal tar manifest hashes every byte, and `package-check` rechecks the
runtime. CI's downloaded-artifact verifier independently checks provenance,
qualification flags, file inventory, roles and hashes inside the tar. The
normal Alpine APK recipe embeds the same required tree at
`/usr/libexec/nuraloumi/browser/`.

Expected producer directory schema: `runtime.json` with
`schema=nuraloumi-browser-runtime-v1` (JSON value), exact `source_sha`,
`producer_revision`, `target`, `abi`, a complete `files` list of relative
paths, SHA256 and semantic roles. Required roles: `engine` at
`nurabrowser-core`, `webprocess` ending in `/WPEWebProcess`,
`networkprocess` ending in `/WPENetworkProcess`, `bwrap`,
`xdg-dbus-proxy`, `loader`, `library`, `module`. The optional
`gpuprocess` role, when enabled, must end in `/WPEGPUProcess`; all
WPE process executables must share their compiled-in libexec directory.
All five qualification
fields (`sandbox`, `instruction_closure`, `dlopen_closure`, `webprocess`,
`review`) must be `PASS`. The tree may not contain symlinks or unlisted
files. The verifier checks executable permissions for the engine, WebProcess,
sandbox helpers and loader, plus ELF32 ARM hardfloat, VFPv3-D16, NEON attributes,
selected forbidden opcode patterns and loader ABI. The downloaded tar verifier
also rejects missing executable modes. Producer evidence must
add a complete `DT_NEEDED` **and** dynamic `dlopen` closure audit; a textual
`PASS` is not proof of runtime correctness.

The Alpine **ordinary-package** workflow currently has no source-matched,
qualified musl WebKit/WPE runtime input. Its package job therefore fails
explicitly. A separate, experimental GitHub Actions workflow,
`.github/workflows/nurabrowser-wpe-musl-producer.yml`, now defines an Alpine
3.22 native ARMv7/QEMU *candidate build*. It pins the official WPEWebKit 2.54.2
archive by SHA256, compiles WPEPlatform/Wayland and a software-rendered engine
with VFPv3-D16 flags, fixes production WebKit's absolute process install path
to `/usr/libexec/nuraloumi/browser/libexec`, and uploads only a candidate
tree with `runtime_qualification=UNQUALIFIED`. This experimental build script
has passed host syntax checks but **has not yet passed an actual GitHub Actions
build**. A candidate tar is not an installable release and does not contain
`runtime.json`; no workflow promotes it to the ordinary package or device.

The existing Debian ARMHF/glibc smoke artifact is
unqualified, mixes host-architecture sandbox helpers and cannot be substituted
into this release. The isolated glibc approach would require its own pinned
ARMHF loader, full library/process tree and sandbox namespace proof without
polluting musl/GStreamer search paths. A native musl producer is preferred.

The desktop entry runs `nuraloumi-browser`, the native Rust control UI.
Its regular-bundle activation code now checks the qualified manifest and
root-owned, non-writable bundle, then attaches to an existing same-UID,
private, authenticated `$XDG_RUNTIME_DIR/nura-browser.sock`, or attempts one
engine start from `/usr/libexec/nuraloumi/browser/nurabrowser-core`.
The child uses `--platform=wayland` and `WPE_PLATFORM=wayland`; WebKit
sandbox disabling is rejected and software rendering is selected. Bundled
helpers, libraries and WebProcess search paths are scoped to that engine
process, never exported to the NuraLoumi shell, player, GStreamer or VDE.
Startup waits at most 12 seconds for a valid RPC reply; the UI owns and reaps
only an engine process it started, and an existing IPC server is neither
removed nor killed on failure. A missing or unqualified bundle fails closed.

This is **host-tested activation logic only**, not a musl WPE runtime
qualification or a successful device run. The producer still needs to prove
that its ELF interpreter, sandbox helper and WebProcess spawn paths work in
the **installed layout**, including indirect subprocess lookups, dynamic
modules, sandbox namespaces, Wayland and real pixels. In upstream WebKit,
`WEBKIT_EXEC_PATH` is consulted only in developer-mode builds; release builds
normally use compiled-in `PKGLIBEXECDIR`, so the producer must prove the
compiled process paths match the actual regular-bundle installation. An
embedded loader file in the bundle alone does not prove the target kernel's
requested
absolute ELF interpreter is available. The older `,browse` script remains
exclusive to the unrelated glibc/private-root lane. Neither desktop
visibility, synthetic tests, nor the manifest proves that a page renders
on `.106`.

## Remaining gates before an artifact can pass

1. Build reproducible same-revision musl ARMv7 WPEWebProcess, engine, ARM
   sandbox helpers and complete transitive/dynamic modules; inventory all
   files with SHA256. Preserve both Pixman fallback and private GPU-video
   opt-in boundaries.
2. Independently qualify VFPv3-D16/no-NEON/no-hardware-division across every
   executable and DSO, with mapping-symbol/CFG resolution for suspected
   Thumb/data false positives and full transitive `DT_NEEDED` resolution.
3. Run isolated sandboxed WebProcess and record `/proc` maps and actual
   `dlopen`/child-process closure. Test real pixels, navigation and touch on
   the SL101 separately. Do not set `WEBKIT_USE_SANDBOX=0`.
4. Pin producer revision/SHA in normal CI; validate final tar and an
   extracted signed APK for byte identity. Run explicit loader-search-path,
   WebProcess, sandbox and desktop-launcher checks **against the installed
   target layout** before owner release/deploy.

Offline Python contract fixtures are intentionally *synthetic* and use a
mock ELF inspector. They test acceptance/rejection logic only, not WebKit
boot, secure process containment, actual ARM execution, or tablet behavior.
