# NuraLoumi ARMv7 artifact gate — OCP 07 (2026-11-21)

## Identity and safety

- OCP: `NURALOUMI-FIX-CLOSURE-20261121-07`
- Inspected committed HEAD before changes: `fd34f43e6dc27898047c1d95a2cc8748ff8128c7`.
- Workspace Cargo version: `0.0.8-rc.1`, **not** 0.0.8 final.
- `scripts/sl101_userspace_release.py` is still pinned to `0.0.7-dev.1`,
  source `9753fae3895e34735cdb4e3269a86f6590911283`, and three exact
  legacy video library hashes. Do not repoint it to 0.0.8 by changing only
  the version string: its stage and live player contracts are incompatible
  until separately reviewed. This run did not invoke that device-only helper.
- No remote Git, CI, SSH, device, release publication or session mutation.
  No existing dirty file was overwritten.

## Gate matrix

| Gate | Status | Evidence / prerequisite |
| --- | --- | --- |
| Current source identity/version | PASS | HEAD and Cargo workspace version above |
| Offline provenance fixtures | PASS | `python3 -m unittest scripts.test_verify_ci_artifact -v`: 7/7, exit 0 |
| Top-level tar SHA256, manifest file hashes, complete tar inventory | PASS (fixture) | Exact tar checksum, no absent/extra/symlink file, manifest checksum closure; negative partial bundle and corruption tests |
| ELF32 ARM hardfloat identity, Alpine/Debian target | PASS (fixture) | Binary header and CI audit per listed binary, wrong target/SHA/version rejected |
| no-NEON, VFPv3-D16, upper D-registers, hardware divide | PENDING (real artifact) | CI emits per-binary audit, but no exact-SHA artifact was supplied; `inspect-armv7-elf.sh` disassembly and independent extracted ELF audit still required |
| Runtime dependency/loader closure | PENDING (real artifact) | CI workflows whitelist NEEDED and loader. No real ELF or dependency package was available to recheck independently; missing dependency cannot be qualified by synthetic fixtures |
| Signed Alpine APK / Debian DEB integrity | PASS (fixture), PENDING (real artifact) | Offline package checksum tests; ephemeral APK signing key is development-only |
| Official main-branch successful exact-SHA run | PENDING | Need explicit authorized CI run ID, artifact ID and exact reviewed SHA |
| 0.0.8 release gate | PENDING | Current version is 0.0.8-rc.1; final 0.0.8 source/version not identified |
| SL101 device and rollback | NOT RUN | Explicitly prohibited here; host PASS never authorizes deployment |

## Reproduction

Host-only:

    python3 -m unittest scripts.test_verify_ci_artifact -v
    python3 scripts/verify-ci-artifact.py --help
    git diff --check

Additional existing suite, run independently:

    python3 -m unittest scripts.test_sl101_userspace_release -v

The latter currently fails one unrelated baseline assertion:
`test_qualification_does_not_require_ac` rejects a pre-existing
`qualify-nuraloumi-player-video-on-device.sh` AC requirement. This
device qualification script is outside this task; no power gate was changed.

Future *authorized* exact artifact verification (not executed here):

    python3 scripts/verify-ci-artifact.py --lane alpine --run-id RUN_ID --sha EXACT_40_HEX_SHA --expected-version 0.0.8-rc.1 --out EMPTY_DIR

Do the same separately for `debian` when that ABI is intended. The command
downloads only; it never installs. Before any candidate release, independently
inspect real extracted ELF instruction/dependency closure and package manifests,
obtain independent review, then follow the separate human-controlled device
qualification and rollback protocol. There is no release authorization here.

## Independent review handoff

Review the verifier diff and fixtures against both current CI workflows.
Check that manifest line syntax matches `xtask`'s actual package producer;
exercise the verifier on an official exact-SHA artifact when authorized, then
run an independent `readelf`/`objdump`/NEEDED audit. A signed package or
text audit alone must not be considered device ISA qualification.
