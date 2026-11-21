# Implement an opt-in NuraLoumi GPU-video bundle consumer contract with audio safety

## Goal
Qualify the NuraLoumi-owned player/playerd integration seam to an externally produced SL101 private video bundle, preserving audio-only behavior and a disabled-by-default GPU path.

## Context
SL101 owner handoff docs/sl101-video-bundle-consumer-handoff-20261119.json and docs/sl101-nuraloumi-gpu-video-contract-20261113.md in sibling sl101_linux. Read NuraLoumi ROADMAP.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md. Previously shipped 414-frame evidence is not proof the current bundle is qualified.

## Scope
In: media/backend adapter contracts, host-only test doubles and isolated CI opt-in checks in NuraLoumi. Do not change separate sl101_linux sources. Negotiate existing media lane claims before edits; if source contracts are not present, deliver a reviewed design plus fixtures only.

## Acceptance
Validate exact NURALOUMI_VIDEO_BUNDLE_DIR, loader origin/ELF closure, strict GStreamer registry, v4l2slh264dec capture-stride-align=64, typed UNSUPPORTED_GPU→PROBE_ONLY→CANDIDATE gate, fail-closed negative cases and audio play/pause/seek/stop/EOS unaffected. Run targeted cargo tests/clippy; do not enable GPU video in default build or claim hardware pass.

## Deliver
Commit only scoped files on a uniquely named local worktree branch if safe; provide exact changed paths, tested commands and SHA evidence. If blocked, leave a precise prerequisite checklist with no false completion.
## Safety and working method
- Read repository AGENTS.md and README.md first. Original shared checkout is dirty and must remain unchanged; work in an isolated ws-bridge worktree with a unique branch, or do read-only investigation if admission/ownership is unavailable. Claim nonoverlapping paths and inspect active workers before writes.
- HOST-ONLY. No SSH/device access, flash, boot, reboot, unattended suspend, live Sway/Wayland mutation, deployment, publication, git push/fetch, or destructive cleanup. Preserve ignored evidence and existing patches; never overwrite them. Avoid unbounded recursive delegation and never spawn follow-up jobs solely to inflate queue size.
- An observed software/build PASS is not a hardware pixel/release PASS. Use exact source/output hashes, commands, exit codes and negative controls. Run targeted cargo tests and cargo clippy; attempt cargo test --workspace with a bounded timeout and report failures. Log honest blockers.
- Checkpoint against the exact canonical OCP task ID after milestones; final complete/blocked checkpoint must name paths, tests, provenance, remaining gates and receipt. No next.md or legacy queue-file scheduling.
