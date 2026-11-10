# Generic viewer — host review and qualification ledger

- **Date:** 2026-11-10
- **Project:** NuraLoumi; canonical OCP
  `NURALOUMI-20261110-NURA-GENERIC-VIEWER-20261110-8AB122`.
- **Scope:** standalone Viewer and its Cairo/Wayland/file(1) fallback.
  No SL101 deployment or suspend was requested or performed for this review.
- **Original implementation:** `301eb015ac060a6d62618dfde073aa231fe15a00`.
- **Repair commits:** `bd6d16b9bcc1165d16377eada2d6ac02da3f7c97`,
  `c2f7faa6f38003960861e7ebcd5334c1393652d7`.
- **Follow-up:** bounded image pixel conversion optimized for Tegra20
  ARMv7 without hardware integer division, and explicit operator documentation.

## Review matrix

| Prior finding | Fix / evidence | Residual scope |
| --- | --- | --- |
| Diagnostic helper can hang waiting on inherited pipe EOF | Nonblocking pipes, bounded output, process-group SIGKILL, nonblocking reaping; forked-pipe test | Descendants that deliberately create a new process group are outside the helper's kill scope, but the caller's wait is still bounded |
| Image overflows viewport at min zoom | True aspect fit without minimum zoom; Cairo clip; extreme 1×4096 scene regression | Real compositor scale/geometry not qualified |
| Slow JPEG/file type blocks Wayland UI | Surface is created before worker decode; loading scene and bounded poll while busy; deterministic worker gate test | A rapidly finished decode may display only the final scene; actual first-interactive latency needs on-device measurement |
| EXIF orientation missing | TIFF APP1 parser for values 1–8, safe little-/big-endian and malformed IFD cases; JPEG fixture | No full metadata-extraction or color-management support |
| Pixel conversion uses divide and modulo per pixel | Iterate checked rows and columns with no per-pixel divide/mod; orientation mappings preserved | Performance gain must be profiled on ARMv7; no measured benchmark claim |
| Any mouse button can close the viewer | Require primary-button press and release inside the close target; targeted regression | Manual input verification under Sway remains pending |

## Required acceptance evidence

Run from the repository root with a stable checkout.

```sh
cargo test --locked -p nuraloumi-shell --bin nuraloumi-viewer
cargo clippy --locked -p nuraloumi-shell --bin nuraloumi-viewer -- -D warnings
cargo fmt --all -- --check
cargo test --locked --workspace
cargo clippy --locked --workspace --all-targets -- -D warnings
git diff --check
```

**Host-only evidence on 2026-11-10:** the scoped viewer tests passed
(22/22) and strict scoped Clippy passed following the row/column and pointer-button changes.
Following the concurrent file-manager/release commits, `cargo fmt --all -- --check`,
`cargo test --locked --workspace`, and workspace-wide strict Clippy also
passed on the host. These checks are necessary but not sufficient for an
accepted SL101 release. The review was performed in a separate
follow-up session; no independent device reviewer verdict has been obtained.

## Device qualification still pending

Test on a precisely identified SL101 kernel/Mesa/Sway stack, measuring:
1. First interactive frame (`<=250 ms` target) and main-loop latency
   with a cold 4 MP JPEG.
2. Peak RSS and allocation/copy behavior, versus the device's ~1 GiB RAM.
3. Touch/back release-inside, cancellation, multi-touch, and primary mouse
   button handling.
4. Actual JPEG EXIF orientations 1–8, PNG alpha, corrupt/truncated inputs
   and over-limit files at real output geometry/scale.
5. `file(1)` helper absence, long-running helper descendants, and safe error
   presentation. Inspect process cleanup without mutating the user's files.

There is **no** ARMv7 hardware performance proof, release approval, tag,
push or SL101 deployment implied by this document. A queued changelog
edit for `0.0.4-dev.2` must be coordinated with the release notes owner.
