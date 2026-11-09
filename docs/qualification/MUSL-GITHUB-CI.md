# GitHub Actions: SL101 Alpine armv7 musl release artifact

`.github/workflows/sl101-musl-armv7.yml` is a CI-only release candidate builder
for the native Alpine/musl SL101. It complements the independent Debian 13
glibc ARMHF job (`.github/workflows/sl101-debian-armhf.yml`).

Both run on pushes to `main`, and both use the **same pushed Git commit**. No
job deploys to either tablet, changes a desktop session, or installs boot
services. Manual workflow dispatch is available.

## Platform and sysroot

The target is `armv7-unknown-linux-musleabihf` (ARMv7 hard-float
VFPv3-D16, **no NEON**, no D16-D31 upper-register bank, no hardware divide).
The glibc target `armv7-unknown-linux-gnueabihf` is incompatible; never
cross-deploy artifacts.

On the GitHub-hosted Ubuntu runner, the job installs Clang/LLD/LLVM and a Rust
stable toolchain with `rust-src` for the repository's existing
`RUSTC_BOOTSTRAP=1 cargo -Z build-std` dynamic musl build path. It enables
QEMU's `linux/arm/v7` architecture and runs the official `alpine:3.22` ARMv7
container to install **signed** Alpine development/runtime packages into an
ephemeral CI sysroot. The packages include musl and GCC CRT/libgcc support,
Cairo, Wayland, XKBCommon, FreeType, HarfBuzz and FriBidi. The existing
`scripts/build-sl101-armv7.sh` and `scripts/armv7-sl101-linker.sh` then
cross-link the full declared runtime set and run the repository's strict ELF
check.

The CI sysroot is **not** the tablet-derived `target/sl101-cross` sysroot,
and the preparation script refuses to overwrite that location. Library versions
from official Alpine 3.22 may differ from a device running a different Alpine
release: a successful ELF audit proves ISA/ABI shape, **not** that all runtime
dependencies are present or compatible on a given tablet. For a tablet-matched
build, continue using the local, prepared tablet-derived sysroot described in
`docs/qualification/ARMV7.md`.

## Artifacts and verification

On `main` pushes, the Actions run uploads a
`nuraloumi-alpine322-armv7-musleabihf-<run_number>` artifact containing
`nuraloumi-ci.tar`, a package `MANIFEST.txt` (Git SHA, target and per-file
SHA256), `SHA256SUMS.txt`, `BUILD-INFO.txt`, `ELF-AUDIT.txt`, the complete
cross-build log and binary inventory. The glibc workflow independently uploads
`nuraloumi-debian13-armv7-gnueabihf-<run_number>`.

The checks reject missing Cargo-declared binaries, stale extra executables,
wrong ELF architecture, missing VFPv3-D16 attributes, NEON attributes,
wrong loader/libc dependencies, and RPATH/RUNPATH. The existing
`inspect-armv7-elf.sh` additionally heuristically checks disassembly for
NEON, upper VFP registers and hardware division.

The default `CI` workflow must independently pass formatting, compilation,
tests and strict Clippy. Host/probe checks:

```sh
python3 -m unittest scripts/test_debian_armhf_ci.py scripts/test_musl_ci.py -v
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
```

For local reproduction **without contacting a tablet**, using an isolated
output directory and a local Docker daemon with ARMv7 QEMU registered:

```sh
export NURALOUMI_SL101_CROSS_ROOT="$PWD/target/sl101-ci-repro"
sh scripts/prepare-ci-musl-sysroot.sh
sh scripts/build-sl101-armv7.sh
```

The preparation stage refuses an existing sysroot. Never point it at the
production tablet-derived sysroot. Only sources committed and pushed to GitHub
are included in CI; downloaded artifacts should be validated against the
manifest and expected `github.sha`.

## Still required on the SL101

On Alpine, stage the musl package in a versioned directory, check its dynamic
dependencies, and run a non-destructive `--help` and no-SIGILL smoke on the
real device before considering activation. Verify the panel renderer status in
the running Sway session (expected `Grate` with the measured GL environment
and mapped Gallium library). Keep the pixman recovery path intact. This CI
pipeline never performs device runtime qualification automatically.
