# SL101 WPE ARMv7/musl CI throughput and checkpoint (2026-11-25)

## Evidence and failure mode

The GitHub Actions native Alpine/QEMU WPE 2.54.2 candidate producer run
`45536119636` on source commit `6272ed9ef3d61f01e31f5afdc43116a95915bec9`
was cancelled at the six-hour job ceiling, after Ninja `7571/8546`.
No compiler failure was established from its tail. More importantly, no
candidate or diagnostic artifact survived that cancellation, so **the old
7,571 compiled steps cannot be resumed**. Do not represent that run as a
successful WPE runtime producer.

The private ARMHF/glibc browser-engine workflow and the ordinary musl ELF
audit workflow are separate. Their green checks do not establish full
ARMv7/musl WebKit build or qualified browser-runtime closure.

## Implemented recovery contract

The candidate workflow now stops the emulated compile after **4h30m**,
before the GitHub six-hour job limit. A build timeout remains a **failed**
producer run. It cannot publish a qualified release, runtime manifest, or
ordinary APK. After the failed build step, the workflow:

1. Preserves the bounded Ninja log and builder exit status.
2. Stops its own Docker container before copying build state.
3. Compresses the source tree, CMake/Ninja graph, object files and
   `BUILD-PROVENANCE.txt` on the **x86 runner**, using zstd.
4. Uploads an explicitly **unqualified** checkpoint plus SHA256 file.

The checkpoint is not a browser runtime or deployment artifact. On a new
runner, a manual `workflow_dispatch` at the **identical source commit**
accepts both the failed run ID and the SHA256 of its
`build-state.tar.zst` member. Both inputs must be provided together.
The workflow verifies the previous GitHub Actions run path, completed failed
status, exact commit, artifact name and archive digest. The container also
checks source SHA, builder script SHA and exact Alpine package-version
inventory before continuing the restored Ninja graph. A mismatch is a
hard failure requiring a clean build.

The archive helper rejects unrelated path members and refuses to overwrite
existing build state. The standalone tests exercise hash mismatch, no
overwrite, path traversal and a small archive round-trip.

## Operator steps after a failed bounded compile

1. Open the failed `NuraBrowser WPE ARMv7 musl candidate` GitHub run.
   Confirm that `BUILD-STATUS.txt`, `BUILD-LOG.txt` and
   `nurabrowser-wpe-musl-checkpoint-<run-id>-<commit>` exist. If not,
   **stop**: there is no reproducible resume state.
2. Download the checkpoint artifact, verify its included
   `build-state.tar.zst.sha256` independently, and retain the exact
   `build-state.tar.zst` digest. Never reuse an unverified digest from a
   different producer run or branch.
3. Dispatch the same workflow on the **exact same commit** (normally a
   pinned branch or tag, not an advanced floating `main`) with
   `resume_run_id` and `resume_archive_sha256`.
4. Verify the subsequent run logs `WPE_CHECKPOINT=RESTORED` and
   `WPE_RESTORED_NINJA=VERIFIED`. Each subsequent bounded failure may
   produce the next independently verified checkpoint.
5. A completed compile only publishes **unqualified candidate evidence**.
   Independently audit ARMv7 VFPv3-D16/no-NEON instructions, musl ELF
   dependency closure, WPE subprocesses, sandbox and pixel/input behavior.
   The qualified producer manifest and release integration remain separate.

## Limitations and next architecture step

- CI script/unit/actionlint checks are host verified; a real multi-hour
  GitHub checkpoint/upload/resume cycle has **not** been exercised by this
  source commit.
- A whole-tree checkpoint can be several gigabytes; runner free disk,
  artifact-size limits and upload throughput are explicit runtime risks.
  A failure to upload means **no** resumable state, never an implicit pass.
- Checkpoint reuse is purposefully same-source and same package set only;
  the already-cancelled 2026-11-25 run did not export a checkpoint.
- The build still invokes native ARMv7 compilers under QEMU. Moving hot C++
  compilation to a cross toolchain or native ARM runner may materially
  improve throughput, but must be qualified against the exact Alpine musl
  sysroot, generated build tools, WebKit JSC config, no-NEON/D16 limits and
  downstream ELF closure first. Do not silently swap the compiler in the
  release workflow.

No GitHub run, tag, package, device service or SL101 system state was
changed by this source-only CI adaptation.
