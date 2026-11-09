# Debian SL101 glibc ARMHF release via GitHub Actions

The ASUS SL101 tablet .100 now boots Debian 13.7 (trixie), glibc and
kernel 7.0.1-postmarketos-grate from MicroSD. Its Tegra20 is an ARMv7
dual-core hard-float CPU, using only the VFPv3-D16 register bank, without
NEON or ARM hardware integer divide.

The Nura/Alpine .106 build uses musl and is not ABI-compatible with
Debian. Do not copy its binaries to .100.

## CI-only build

Workflow: .github/workflows/sl101-debian-armhf.yml

- The compilation runs on an ephemeral GitHub-hosted Ubuntu 24.04 runner
  inside a Debian 13 trixie container, so the glibc ABI matches Debian.
- Debian armhf multiarch libraries provide Cairo, Wayland and XKBCommon;
  GCC cross-linker is arm-linux-gnueabihf-gcc.
- Rust cross target is armv7-unknown-linux-gnueabihf (not musl).
- Linker flags: -march=armv7-a -mfpu=vfpv3-d16 -mfloat-abi=hard.
- Rust flags: -C target-feature=-neon,-d32,-hwdiv,-hwdiv-arm.
- Cargo builds all declared workspace release binaries and existing
  cargo xtask package / package-check enforces completeness and SHA256
  per-file integrity.
- ELF validation checks ELF32, ARM, hard-float, VFPv3-D16, the glibc
  loader /lib/ld-linux-armhf.so.3, libc.so.6 and absence of explicit
  NEON ELF attributes. This does not replace physical no-SIGILL tests.
- Build output and dependencies do not consume local target/ disk space.

A push to main or a branch matching ci/sl101-debian-armhf-* triggers the
build. workflow_dispatch provides a manual rebuild. Pull requests touching
supported source or build files also trigger it. The independent
sl101-musl-armv7.yml workflow runs on main pushes as well, so the same pushed
Git SHA produces both glibc and musl artifacts. See MUSL-GITHUB-CI.md.

GitHub Actions uploads a downloadable artifact containing:

- nuraloumi-ci.tar (the complete deterministic NuraLoumi runtime package);
- MANIFEST.txt with binary list, git SHA and file SHA256 hashes;
- SHA256SUMS.txt for the archive;
- BUILD-INFO.txt with compiler and Debian library versions;
- ELF-AUDIT.txt recording inspected binary attributes.

Fetch the artifact through the GitHub Actions run page. Verify SHA256
before extracting. Artifact retention is 14 days.

## Reversible qualification on Debian .100

No CI step accesses the tablet, systemd, a compositor, the bootloader
or eMMC. The build deliberately does not auto-deploy.

1. Verify the target still runs 7.0.1-postmarketos-grate from
   /dev/mmcblk1p1, and has a working Wayland compositor/seat.
2. Download and verify the GitHub Actions artifact and its manifest.
3. Extract into a separate versioned candidate directory on the SD.
4. Run ldd on the staged panel/menu to ensure all of libcairo.so.2,
   libwayland-client.so.0, libxkbcommon.so.0 and libc.so.6 resolve
   without missing dynamic libraries.
5. Run a non-mutating --help smoke before launching panel/menu from
   an active graphical session. Observe for SIGILL, RSS and input.
6. Only then consider service/autostart integration with an explicit
   rollback. No local Rust release build is needed.

If the candidate fails, remove only the private staging directory
and use the previous desktop. Avoid overwriting production binaries.

## Qualification limitations

- Both workflows package one committed and pushed Git source revision.
  Uncommitted changes from other agents are not implicitly included.
- Debian:trixie multiarch repositories and stable Rust are live moving
  inputs; the workflow records versions, but currently does not pin
  digest-addressed container images, an apt snapshot or a specific
  Rust toolchain release. Rebuilds are auditable, not necessarily
  byte-identical over time.
- Static ELF validation is a safety gate, not evidence that every
  Cairo/Wayland/XKB path executes safely on Tegra20. The tablet runtime
  still needs no-SIGILL and compositor integration checks.
