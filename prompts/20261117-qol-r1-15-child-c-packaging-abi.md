# Independent release artifact and packaging qualification: inspect 0.0.7-dev.1 tags, sources, manifests, Cargo.lock alignment, files/player/notification desktop entries, DBus activation, package manifest integrity, executable list. Build actual host and ARMv7 packages if available without any device/network access; inspect ELF architecture, ARMv7 VFPv3-D16 and NEON absence, dependencies/glibc/musl closure, wl_shm fallback. Do not edit lane14 packaging paths or changelog. Deliver docs/qualification/QOL-R1-15-PACKAGE-AUDIT.md on exclusive claim.

@projmgrauth
Set repo: nuraloumi

Canonical parent task: NURALOUMI-20261117-QOL-R1-15-RELEASE-INTEGRATION-20261117-7FB90E
Local-only, review-only unless a scoped owner hands off. NO push, remote CI, install, power/suspend/reboot, device access, Grate/GStreamer private ABI changes. Preserve untracked and dirty work. Read AGENTS.md, docs/QOL-RELEASE-20261117.md, current OCP task states and all claims before any actions. ARMv7 VFPv3-D16/no NEON, Cairo+wl_shm fallback, >=48 logical px hit targets. Every finding must include code path, executable test/fixture, severity, before/after expectation. Do not duplicate active lanes 01-14; independent verification only. Record canonical checkpoint, receipts, follow-ups; no unsupported success claims.

## Scope
package scripts/tests and artifact path/hash inventory; readelf on actual binaries; explicit untested SL101 E2E list

## Acceptance
undefined

## MANDATORY RECURSIVE DELEGATION CONTRACT (propagate verbatim in every child)
Read /home/user/code/chatgpt-tooling/packages/chatgpt-operator-skill/SKILL.md and /home/user/code/chatgpt-tooling/packages/chatgpt-fleet-skill/SKILL.md. Use canonical chatgpt-ops / ChatGPT ABC operator dispatch yourself, not improvised browser clicks or legacy queue files. When you identify a substantial independent operation field that cannot be exhaustively solved within your bounded job, dispatch 2-8 narrowly scoped child agents with explicit ownership, acceptance criteria, evidence and followup tasks. Prefer 2 initially, respect physical fleet capacity, queue rather than blind browser growth, avoid duplicate/ambiguous replay. Delegate when justified, not to create recursion for its own sake; leaf-sized work can be completed directly. YOU MUST COPY THIS ENTIRE RECURSIVE DELEGATION CONTRACT INTO EVERY CHILD PROMPT and require every child to propagate it unchanged to its children. Retain integration responsibility: worker receipt is not completion, inspect artifacts/tests, dispatch corrective followups and record durable checkpoints. No child may exceed the hardware safety boundaries above. Use operator skill yourself even if workers are not needed, for task/checkpoint evidence.

SPECIAL CAMPAIGN RULE: Dispatch AT LEAST THREE actual distinct follow-up ChatGPT ABC operator OCP prompts based on your own investigation, with precise narrower ownership, tests and verifiable deliverables; copy this entire section unchanged into all three. Prefer independent verification, edge-case tests, integration seam checks over competing source edits. Where dispatch is fail-closed due to permissions or fleet capacity, preserve 3 fully executable, uniquely identified prompt files and report the exact blocker rather than bypassing the operator. Do not recursively generate duplicate or low-value tasks just to reach a count. Never duplicate in-flight tasks or create a boundless uncontrolled fanout.
