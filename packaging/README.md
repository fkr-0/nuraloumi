# NuraLoumi package staging

NuraLoumi deployment must consume **only GitHub Actions build artifacts** from
a successful, reviewed run for the exact intended Git SHA and ARMv7 ABI.
Never deploy the build host's local target/ directory to the SL101.

The original xtask tar is still a copy-only test bundle. It does not install
into /usr or register services:

Layout:

    bin/
      nuraloumi-panel
      nuraloumi-menu
      nuraloumi-probe
      nuraloumi-thumbnail-helper
      ... all declared nuraloumi-shell/wayland/providers binary targets
    config/
      nuraloumi.toml.example
    share/nuraloumi/
      qualification/
    LICENSE
    README.md
    MANIFEST.txt

The manifest records the target triple, Git commit, exact release build command,
and SHA-256 of every staged input. The tar writer fixes ordering, mtime, owner
and group metadata so identical staged inputs can be hash-compared.

Build and stage:

    cargo xtask build-release
    cargo xtask package
    cargo xtask package-check --dir target/nuraloumi-package/<host-triple>/nuraloumi-<version>-<host-triple>

For SL101 cross artifacts, use armv7-unknown-linux-musleabihf only after
qualify-armv7 reports the Rust target and linker prerequisites. A successful
compiler check or ELF audit is not on-device qualification.

## CI native packages

Two existing cross-build jobs now package the **same** already audited binary
staging directory after `cargo xtask package-check` and ELF/no-NEON audit:

* Alpine 3.22 / ARMv7 / musl: a native `.apk` from `abuild` running in a
  QEMU-backed Alpine arm/v7 container. The direct ELF NEEDED libraries are
  checked against a strict readelf allowlist mapped to explicit package
  dependencies; `!tracedeps` prevents the ephemeral build container from
  changing runtime dependencies. The included CI public key is ephemeral
  per run; this is a development package, **not** a trusted production signing
  identity. Installations must never silently import CI keys or pass
  `apk add --allow-untrusted`.
* Debian 13 / armhf / glibc: a normal `.deb` created with
  `dpkg-deb --root-owner-group`. All binaries, a config example and the
  original SHA256 manifest are included. There are no maintainer scripts or
  system service enablement.

Both bundles contain their original `MANIFEST.txt`, `BUILD-INFO.txt`,
`ELF-AUDIT.txt`, full Git SHA and standalone SHA256 checksums. The workflow
artifact name also includes the exact source SHA. CI does not connect to the
tablets and cannot deploy.

Before qualifying a candidate, record the Actions run ID, verify its
`conclusion=success`, compare the artifact's SHA with the reviewed commit,
validate `SHA256SUMS.txt` and extracted `MANIFEST.txt`, and confirm that the
OS/ABI are the intended Alpine-musl or Debian-glibc pairing. Extract the
existing tar to disposable user-owned staging for the first live test; only
after device qualification should an operator explicitly authorize system
package installation.

Use `python3 scripts/verify-ci-artifact.py --lane alpine|debian --run-id N
--sha FULL_SHA --out EMPTY_DIRECTORY` to download and validate the exact
successful main-branch CI run **without deploying**. The script refuses
wrong-ABI, mismatched Git provenance or package/tar checksums.

Flatpak is **not** part of the ARMv7 product lane: the supported current
Freedesktop/Flathub runtimes are x86_64 and aarch64, not SL101 armv7.
A separate desktop Flatpak would need its own source/SDK build,
layer-shell/sandbox boundary review and integration testing. It must not wrap
the ARMv7 ELF into a misleading Flatpak artifact.

See [CI-only packages and release gates](../docs/qualification/CI-ONLY-NATIVE-PACKAGES.md).
