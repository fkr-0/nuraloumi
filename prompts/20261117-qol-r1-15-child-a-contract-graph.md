# NURALOUMI-20261117-QOL-R1-15-VERIFY-CONTRACT-GRAPH-20261117-A

@projmgrauth
Set repo: nuraloumi

Canonical parent task: NURALOUMI-20261117-QOL-R1-15-RELEASE-INTEGRATION-20261117-7FB90E
Local-only, review-only unless a scoped owner hands off. NO push, remote CI, install, power/suspend/reboot, device access, Grate/GStreamer private ABI changes. Preserve untracked and dirty work. Read AGENTS.md, docs/QOL-RELEASE-20261117.md, current OCP task states and all claims before any actions. ARMv7 VFPv3-D16/no NEON, Cairo+wl_shm fallback, >=48 logical px hit targets. Every finding must include code path, executable test/fixture, severity, before/after expectation. Do not duplicate active lanes 01-14; independent verification only. Record canonical checkpoint, receipts, follow-ups; no unsupported success claims.

## Scope
Independent contract graph qualification: core widgets/renderer hit regions/Wayland pointer-touch/semantic shell/provider actions. Specifically reproduce and isolate layout.rs:1065,1082 strict Clippy manual_div_ceil and check 48px hit target edge cases, scroll thumb input, slider bounds, tab keyboard parity, outside dismissal, mute acknowledgement after provider refresh. Confirm whether lanes 01,02,04,05 provide handoff. Report exact failing tests and seam patch suggestions; avoid source edits in active claims. Deliver docs/qualification/QOL-R1-15-CONTRACT-AUDIT.md only after acquiring a claim.

## Acceptance
cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; deterministic scene and event fixtures; verify edge input at endpoints

## MANDATORY RECURSIVE DELEGATION CONTRACT (propagate verbatim in every child)
Read /home/user/code/chatgpt-tooling/packages/chatgpt-operator-skill/SKILL.md and /home/user/code/chatgpt-tooling/packages/chatgpt-fleet-skill/SKILL.md. Use canonical chatgpt-ops / ChatGPT ABC operator dispatch yourself, not improvised browser clicks or legacy queue files. When you identify a substantial independent operation field that cannot be exhaustively solved within your bounded job, dispatch 2-8 narrowly scoped child agents with explicit ownership, acceptance criteria, evidence and followup tasks. Prefer 2 initially, respect physical fleet capacity, queue rather than blind browser growth, avoid duplicate/ambiguous replay. Delegate when justified, not to create recursion for its own sake; leaf-sized work can be completed directly. YOU MUST COPY THIS ENTIRE RECURSIVE DELEGATION CONTRACT INTO EVERY CHILD PROMPT and require every child to propagate it unchanged to its children. Retain integration responsibility: worker receipt is not completion, inspect artifacts/tests, dispatch corrective followups and record durable checkpoints. No child may exceed the hardware safety boundaries above. Use operator skill yourself even if workers are not needed, for task/checkpoint evidence.

SPECIAL CAMPAIGN RULE: Dispatch AT LEAST THREE actual distinct follow-up ChatGPT ABC operator OCP prompts based on your own investigation, with precise narrower ownership, tests and verifiable deliverables; copy this entire section unchanged into all three. Prefer independent verification, edge-case tests, integration seam checks over competing source edits. Where dispatch is fail-closed due to permissions or fleet capacity, preserve 3 fully executable, uniquely identified prompt files and report the exact blocker rather than bypassing the operator. Do not recursively generate duplicate or low-value tasks just to reach a count. Never duplicate in-flight tasks or create a boundless uncontrolled fanout.
