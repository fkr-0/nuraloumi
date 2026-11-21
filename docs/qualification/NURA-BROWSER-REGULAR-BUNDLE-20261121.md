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
`nurabrowser-core`, `webprocess` ending in `/WPEWebProcess`, `bwrap`,
`xdg-dbus-proxy`, `loader`, `library`, `module`. All five qualification
fields (`sandbox`, `instruction_closure`, `dlopen_closure`, `webprocess`,
`review`) must be `PASS`. The tree may not contain symlinks or unlisted
files. The verifier checks executable permissions for the engine, WebProcess,
sandbox helpers and loader, plus ELF32 ARM hardfloat, VFPv3-D16, NEON attributes,
selected forbidden opcode patterns and loader ABI. The downloaded tar verifier
also rejects missing executable modes. Producer evidence must
add a complete `DT_NEEDED` **and** dynamic `dlopen` closure audit; a textual
`PASS` is not proof of runtime correctness.

The Alpine workflow currently has **no** source-matched musl WebKit/WPE
producer stage or verified runtime input. Its ordinary package job therefore
fails explicitly. The existing Debian ARMHF/glibc smoke artifact is
unqualified, mixes host-architecture sandbox helpers and cannot be substituted
into this release. The isolated glibc approach would require its own pinned
ARMHF loader, full library/process tree and sandbox namespace proof without
polluting musl/GStreamer search paths. A native musl producer is preferred.

The new desktop entry starts `nuraloumi-browser`, the Rust control UI. Its
native RPC client expects an already-running same-UID engine socket at
`$XDG_RUNTIME_DIR/nura-browser.sock`; this package change does **not** install
or activate the engine service. The older `,browse` script is exclusively for
the separately prepared glibc/private-root lane and points to
`/usr/libexec/nura-browser/nurabrowser-core`, not the ordinary release path
`/usr/libexec/nuraloumi/browser/nurabrowser-core`. A native musl/regular-bundle
engine activation, sandbox path mapping and end-to-end launcher test are still
required. Neither desktop visibility nor the manifest is evidence that a page
can render on the tablet.

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
   extracted signed APK for byte identity. Add explicit loader-search-path and
   desktop launcher tests before owner release/deploy.

Offline Python contract fixtures are intentionally *synthetic* and use a
mock ELF inspector. They test acceptance/rejection logic only, not WebKit
boot, secure process containment, actual ARM execution, or tablet behavior.
