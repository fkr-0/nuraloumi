# NuraLoumi CI suite and browser deployment preparation — 2026-11-19

## Intended CI matrix

| Workflow | Build output | Qualification boundary |
| --- | --- | --- |
| ci.yml | Host Rust workspace, Python browser contracts | Host only |
| sl101-musl-armv7.yml | ARMv7 musl suite, including Rust browser; tar and signed development APK | Development key is not trusted for device release |
| sl101-debian-armhf.yml | ARMHF glibc suite, including Rust browser; tar and DEB | Target ABI still needs hardware verification |
| nurabrowser-engine-armhf.yml | Separately built native C/WPEPlatform engine in private libexec path; archive, checksums, ELF proof | Not runtime qualified |

The Cargo release inventory currently lists 28 runtime binaries, including
nuraloumi-browser. The Rust frontend is therefore already packaged in the
existing musl and Debian Cargo workflows.

The native browser engine is in components/nura-browser-engine and remains
separate from the NuraLoumi Cairo frontend. The new CI job builds with the
existing pinned ARMHF WPE 2.54.0-2 Docker toolchain (two SHA-256-verified Debian
packages), Meson, and the ARMHF executable audit. No historical public
,browse launcher, GTK, XWayland, NEON requirement or sandbox bypass is added.

The artifact explicitly marks dependency ISA closure, WebProcess sandbox
and device installation unqualified. CI does not deploy.

## Verification evidence

- cargo test --workspace --locked: exit 0 on live integrated workspace.
- Python package CI contract suite before modifications: 25/25 passed.
- Python package/verifier/engine CI contracts after modifications: 15/15 passed.
- C engine Python tests: 72 passed, 2 real sandbox runtime checks skipped.
- actionlint .github/workflows/nurabrowser-engine-armhf.yml: exit 0.
- Existing pinned container built native ARMHF nurabrowser-core and emitted
  NURABROWSER_ENGINE_CI_BUILD=PASS with private libexec stage.
- Initial committed clean HEAD had a Rust merge-marker in player_engine.rs
  and format drift in notifications/panel. The separately owned source-fix lane
  committed the repairs as aada576; a new clean git archive passed cargo fmt
  --all -- --check and cargo clippy --workspace --all-targets --locked --
  -D warnings (exit 0).
- Fresh rebuild of the actual pinned Dockerfile.ci-armhf from downloaded,
  SHA256-verified WPE packages: exit 0. Then rebuilt the private C engine
  against that fresh image: exit 0 and ARMHF audit PASS.
- Fresh local archive SHA-256:
  390ef36a2ca0590acbabd7cd34e04e3808914bbeb6c906a8af4fee28a22e87c4.
  This represents source SHA aada57678001b37c714f76197c75c079ea3ee673;
  local proof, not a GitHub Actions receipt.

### Packaging corrections

The CI artifact verifier now permits SemVer -rc.N and maps release candidate
package names to Alpine _rcN and Debian ~rc.N. The Debian package constructor
uses ~rc.N. The same invalid/unrelated release cases remain rejected.

## Deployment preparation for SL101 .106

The name "23106" is tentatively interpreted as the established SL101 .106.
Confirm the exact target identity before any device action.

1. Record the exact pushed main Git SHA; wait for green CI, Alpine, Debian
   and private WPE engine workflows for that same SHA.
2. Download the exact-SHA Alpine artifact and validate it with
   scripts/verify-ci-artifact.py (run-id, sha, lane alpine, empty out path).
   Confirm manifest/git commit, tar SHA, APK digest and signing identity.
3. Download the matching engine artifact and verify SHA256SUMS, ELF attributes,
   WPE NEEDED dependencies, private libexec install path and provenance.
4. Do not install or activate the engine until independent proof of ARMHF
   dependency/no-NEON closure, sandboxed WebProcess, live compositor pixels
   and touch/focus/tab handoff, supervisor recovery and rollback readiness.
5. An ephemeral CI APK key is not a device trust root. No auto-enrolment
   or --allow-untrusted workaround is allowed.
6. Only after review, sign with an approved trusted key, preserve previous
   installation and record revert steps. Installation is a separate action.

PROVEN: local Rust suite; ARMHF C engine compilation; checksums; offline tests.
UNPROVEN: remote CI for future SHA; complete WPE runtime/ISA; compositor,
sandbox, physical device and deployment. CI compilation is not release approval.
