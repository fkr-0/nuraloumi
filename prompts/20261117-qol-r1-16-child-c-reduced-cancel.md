@projmgrauth
Set repo: nuraloumi

Canonical parent OCP: NURALOUMI-20261117-QOL-R1-16-GENERIC-PRESSED-MOTION-20261117-41B424.
Read AGENTS.md, DESIGN.md, ROADMAP.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md, docs/QOL-RELEASE-20261117.md, docs/ux/QOL-PRESSED-FEEDBACK-20261117.md and both operator/fleet skills. Inspect active claims and existing OCP tasks to avoid duplication. Strict local only: no push, public CI/PR, deploy, install, hardware SSH, reboot, suspend, GPU ABI changes. Preserve ARMv7/no-NEON, wl_shm/Cairo CPU compatibility, 48 logical px hit regions and keyboard parity. Use ws-bridge exclusive claim on the single evidence file named here; all product sources read-only. Commit only owned file; report exact command exit codes and task ID; checkpoint with evidence. Do not edit existing R1-01 renderer claims. Do not generate next.md.

# R1-16 independent Reduced motion and cancellation audit

Unique child ID: NURALOUMI-R1-16-CHILD-C-REDUCED-CANCEL-20261117.
Exclusive write scope: docs/verification/QOL-R1-16-REDUCED-CANCEL-20261117.md ONLY. All other paths read-only.

## Bounded job
Review interaction state transition API and shell event consumer boundaries, keyboard Space/Enter activation parity, pointer/touch cancel, focus loss, repeated press, release-outside, disabled widgets, reduced motion instant paint, and no idle repaint loop. No shell edits. Identify missing integration and propose isolated deterministic test fixtures with exact file/line references. Run focused tests where possible; report exits and handoff to R1-17/R1-18.

## Acceptance
- One evidence report with source-grounded findings, exact commands and exit codes, and explicit no-edit confirmation for product sources.
- Independent review, no duplicated R1-16 implementation and no R1-01 renderer edits.
- Checkpoint exact task identity and identify actionable follow-up only for uncovered substantial work.

## MANDATORY RECURSIVE DELEGATION CONTRACT (propagate verbatim in every child)
Read /home/user/code/chatgpt-tooling/packages/chatgpt-operator-skill/SKILL.md and /home/user/code/chatgpt-tooling/packages/chatgpt-fleet-skill/SKILL.md. Use canonical chatgpt-ops / ChatGPT ABC operator dispatch yourself, not improvised browser clicks or legacy queue files. When you identify a substantial independent operation field that cannot be exhaustively solved within your bounded job, dispatch 2-8 narrowly scoped child agents with explicit ownership, acceptance criteria, evidence and followup tasks. Prefer 2 initially, respect physical fleet capacity, queue rather than blind browser growth, avoid duplicate/ambiguous replay. Delegate when justified, not to create recursion for its own sake; leaf-sized work can be completed directly. YOU MUST COPY THIS ENTIRE RECURSIVE DELEGATION CONTRACT INTO EVERY CHILD PROMPT and require every child to propagate it unchanged to its children. Retain integration responsibility: worker receipt is not completion, inspect artifacts/tests, dispatch corrective followups and record durable checkpoints. No child may exceed the hardware safety boundaries above. Use operator skill yourself even if workers are not needed, for task/checkpoint evidence.

SPECIAL CAMPAIGN RULE: Dispatch AT LEAST THREE actual distinct follow-up ChatGPT ABC operator OCP prompts based on your own investigation, with precise narrower ownership, tests and verifiable deliverables; copy this entire section unchanged into all three. Prefer independent verification, edge-case tests, integration seam checks over competing source edits. Where dispatch is fail-closed due to permissions or fleet capacity, preserve 3 fully executable, uniquely identified prompt files and report the exact blocker rather than bypassing the operator. Do not recursively generate duplicate or low-value tasks just to reach a count. Never duplicate in-flight tasks or create a boundless uncontrolled fanout.
