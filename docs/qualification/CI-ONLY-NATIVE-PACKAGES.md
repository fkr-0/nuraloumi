# NuraLoumi CI-only release artifacts — ARMv7

## Provenance and intended hosts

| Host userspace | Target / runtime | Actions build | Package |
| --- | --- | --- | --- |
| postmarketOS Alpine 3.22 | armv7-unknown-linux-musleabihf, musl | sl101-musl-armv7.yml | .apk plus original .tar |
| Debian 13 | armv7-unknown-linux-gnueabihf, glibc | sl101-debian-armhf.yml | .deb plus original .tar |

These files belong to **different runtime ABIs** and are not interchangeable.
The build chain pins VFPv3-D16 and excludes NEON attributes. No local host
build may be substituted for a successful workflow artifact.

The `.deb` is installable after device qualification; it owns only the
NuraLoumi executables, a config example, and packaged documentation.
No post-install hooks, compositor/service changes, boot changes or login
session modifications run during package installation.

The `.apk` is a development-signed Alpine package. Each CI run generates a
new ephemeral signing key and publishes its **public** key alongside the
package, then destroys its private key. The package can be verified against
that key, but the resulting identity is **not** a persistent production trust
root. Production APK distribution needs a long-term private signing key held
in protected CI secrets and a public key intentionally enrolled on target
devices. Never silently add CI ephemeral public keys to /etc/apk/keys.

## Required gates before any package-based deployment

1. Pin Git SHA and exact Actions run IDs (do not use `latest`). Confirm
   both build runs actually targeted that SHA and concluded successfully.
2. Download artifacts by run ID, verify ZIP contents and the SHA256 checksums
   attached to the tar and native package. Compare original staging
   `MANIFEST.txt` `git_commit`, `git_dirty=false`, target triple, version
   and binary list. Preserve the CI `ELF-AUDIT.txt` for qualification.
3. Validate the ABI against the destination: no glibc binaries on Alpine,
   no musl binaries on Debian. Preserve the existing recovery desktop.
4. Stage and smoke the binaries under an isolated, unprivileged directory
   first. Record first-frame latency, touchscreen and keyboard response,
   memory footprint, absent libraries, and real no-SIGILL evidence.
5. Only after operator-reviewed hardware qualification, choose a deliberate
   package installation strategy and rollback. APK production installation
   remains blocked until a persistent signing key and explicit device trust
   enrollment have been implemented and reviewed.

CI **does not deploy** and must never contain hardcoded device IPs, SSH,
service restarts or writes to the boot partition.

## Flatpak decision

Flatpak is a future, independent desktop target, not an SL101 armv7 delivery
mechanism. Freedesktop/Flathub runtimes ship x86_64 and aarch64. A real desktop
Flatpak should rebuild the app inside a matching SDK from source, declare
Wayland/layer-shell and filesystem/IPC permissions explicitly, and validate
runtime subprocess behavior; copying the musl or glibc armv7 tar into a Flatpak
would produce a non-working file with a plausible extension.

## CI artifacts are not device qualification

ELF ABI audits are heuristic. Running on the actual SL101 (Sway/labwc,
postmarketOS or Debian userspace, known kernel and Mesa build) is a separate,
human-supervised qualification step. Never label CI success alone as a
production deployment approval.
