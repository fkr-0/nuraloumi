# NuraBrowser R2 — release packaging gate (2026-11-19)

## Verdict: PENDING — no installable release qualified

Current checkout: `387c5e61ca2a338f205a70c7b9f458e7135b65df` at start
of review, with concurrent uncommitted browser and CI work. No Git push,
CI trigger, device install or desktop/session mutation was performed.

The canonical application is `nuraloumi-browser` (Rust/Cairo/Wayland) and
its private WPE engine is installed at
`/usr/libexec/nuraloumi/browser/nurabrowser-core`. The historical
`,browse` launcher in `components/nura-browser-engine/scripts/launch.sh`
and older CI tarball provenance are **not** the canonical release.
`components/nura-browser-engine/meson.build` installs the engine privately,
not a public launcher. The currently tracked application desktop inventory
has no `nuraloumi-browser.desktop`; package installability is therefore
not proven. The older standalone engine manifest
(`components/nura-browser-engine/packaging/create-manifest.py`) still
describes the legacy `,browse` path; it must not be accepted as a current
NuraLoumi package manifest.

## New fail-closed preflight

`scripts/qualify-nurabrowser-release.py` expects an independently produced
archive, exact approved archive SHA-256, separately reviewed source SHA,
a release-manifest JSON and an extracted read-only staging root. It verifies:

- root-owned regular-only deterministic member inventory, no duplicate paths,
  path traversal, symlinks, special nodes or privileged mode bits;
- SHA-256 equality of every regular member against the explicit manifest and
  the approved archive digest;
- exact source provenance and explicit Gate 2 `PASS`;
- native public `nuraloumi-browser` ELF, native private `nurabrowser-core`
  ELF, and a desktop entry launching the public Rust UI;
- rejection of the historical public `,browse` alias and historical internal
  engine install location;
- extracted binary hash equality, ARMv7 hard-float/VFPv3-D16/no-NEON ELF
  attributes, and target-specific musl/glibc ELF interpreter selection;
- explicitly declared atomic-retention rollback and public launch contract.

The checker returns `STATIC-PACKAGE-PASS-DEVICE-PENDING` only for a complete
fixture satisfying these static preconditions. It cannot itself prove a
runtime WPE library closure, the authenticity of an external Gate 2 report,
installation script semantics, signatures, byte-for-byte build reproducibility,
or an SL101 launch. A declared Gate 2 PASS must be independently checked and
bound to the exact archive/source SHA before release approval.

Example (paths are placeholders, not existing output):

    python3 scripts/qualify-nurabrowser-release.py \
      --archive /staging/nuraloumi-browser-armhf.tar \
      --archive-sha256 <reviewed-64-hex-digest> \
      --manifest /staging/browser-release.json \
      --source-sha <reviewed-40-hex-commit> \
      --extracted-root /staging/root

Manifest schema: `nuraloumi.browser.release.v1` with fields
`source_sha`, `target`, `gate2: PASS`, `files` (complete relative-path
to SHA-256 mapping), `launch: nuraloumi-browser`, and
`rollback: retain-previous-atomically`. This is a **qualification input
contract**, not a shipping manifest yet.

## Blocking gates

1. **Gate 2:** `docs/qualification/NURA-BROWSER-R2-ARMV7-CLOSURE.md`
   explicitly records FAIL/incomplete dynamic WebProcess and `dlopen`
   closure. No static package preflight may override it.
2. **Missing real current-source artifact:** native engine + Rust browser,
   desktop file, pinned target dependency closure, package archive, exact
   provenance and rollback implementation have not been presented together.
3. **Packaging ownership:** shared APK/DEB builders and workflow files are
   owned by another CI lane. They were neither modified nor assumed suitable
   for a browser-bearing package. A separate coordinated builder integration
   must supply the contract above.
4. **Deployment:** ABI validation, package installation, real Wayland launch,
   sandbox/WebProcess and rollback on SL101 are not proved here.

## Test ledger

| Check | Exit | Result |
| --- | ---: | --- |
| `python3 -m unittest discover -s scripts -p test_qualify_nurabrowser_release.py -v` | 0 | PASS 9/9 |
| `python3 -m py_compile scripts/qualify-nurabrowser-release.py scripts/test_qualify_nurabrowser_release.py` | 0 | PASS |
| `meson test -C /tmp/nurabrowser-r2-sandbox-build --print-errorlogs` | 0 | PASS 7/7 (existing host build directory) |
| `cargo test -p nuraloumi-shell --bin nuraloumi-browser` | 0 | PASS 4/4 |
| `cargo clippy -p nuraloumi-shell --bin nuraloumi-browser -- -D warnings` | 0 | PASS |
| `git diff --check` | 0 | PASS (tracked changes; new untracked files separately checked) |
| ARMHF complete library runtime closure | — | PENDING (Gate 2) |
| Actual native APK/DEB install and rollback | — | PENDING |
| Device launch and browser pixels/touch | — | PENDING |

No release or deployment claim follows from host tests alone.
