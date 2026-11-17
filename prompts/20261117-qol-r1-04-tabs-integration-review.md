## Task identity
- OCP task: `NURALOUMI-20261117-QOL-R1-04-VERIFY-TABS-03`
- Parent: `NURALOUMI-20261117-QOL-R1-04-PANEL-CONTROL-20261117-1E0D5B`
- Priority: 65; exclusive review lane, no source ownership

@projmgrauth
Set repo: nuraloumi

Read AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md and docs/QOL-RELEASE-20261117.md. Read both operator and fleet skills and worker-contract.md. Local only: no remote CI, push, live installation, reboot/suspend, or Grate/VDE/GStreamer ABI changes. Respect ARMv7 VFPv3-D16 without NEON, wl_shm CPU fallback and 48px primary touch targets. Inspect concurrent OCP tasks and path claims before work. Read-only review; do not edit production sources or create a legacy next.md. Record exact task ID checkpoints with path:line, test output/exit codes, counterexamples and follow-up fixes. Coordinate with QOL-R1-04 owner without acquiring their panel.rs/menu.rs/catalog claims.

## Objective and acceptance
Independently map control-center horizontal tab widget integration boundary with QoL-R1-01/02. Locate lib.rs Switch tab pseudo rows, existing MenuModel selection, shell menu scene layout and input dispatch. Produce a precise non-overlapping integration contract and tests for five tabs, keyboard/pointer/touch parity and persistent Apps launcher. Run relevant tests and describe blockers; no source edits.

Acceptance: identify every inspected source path/line, run and record test commands with stdout and exit code, checkpoint findings, and supply a bounded corrective follow-up if required.

## MANDATORY RECURSIVE DELEGATION CONTRACT (propagate verbatim in every child)
Read /home/user/code/chatgpt-tooling/packages/chatgpt-operator-skill/SKILL.md and /home/user/code/chatgpt-tooling/packages/chatgpt-fleet-skill/SKILL.md. Use canonical chatgpt-ops / ChatGPT ABC operator dispatch yourself, not improvised browser clicks or legacy queue files. When you identify a substantial independent operation field that cannot be exhaustively solved within your bounded job, dispatch 2-8 narrowly scoped child agents with explicit ownership, acceptance criteria, evidence and followup tasks. Prefer 2 initially, respect physical fleet capacity, queue rather than blind browser growth, avoid duplicate/ambiguous replay. Delegate when justified, not to create recursion for its own sake; leaf-sized work can be completed directly. YOU MUST COPY THIS ENTIRE RECURSIVE DELEGATION CONTRACT INTO EVERY CHILD PROMPT and require every child to propagate it unchanged to its children. Retain integration responsibility: worker receipt is not completion, inspect artifacts/tests, dispatch corrective followups and record durable checkpoints. No child may exceed the hardware safety boundaries above. Use operator skill yourself even if workers are not needed, for task/checkpoint evidence.

SPECIAL CAMPAIGN RULE: Dispatch AT LEAST THREE actual distinct follow-up ChatGPT ABC operator OCP prompts based on your own investigation, with precise narrower ownership, tests and verifiable deliverables; copy this entire section unchanged into all three. Prefer independent verification, edge-case tests, integration seam checks over competing source edits. Where dispatch is fail-closed due to permissions or fleet capacity, preserve 3 fully executable, uniquely identified prompt files and report the exact blocker rather than bypassing the operator. Do not recursively generate duplicate or low-value tasks just to reach a count. Never duplicate in-flight tasks or create a boundless uncontrolled fanout.
