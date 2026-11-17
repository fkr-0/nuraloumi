# NURALOUMI-20261117-QOL-R1-08-CHILD-EOS-TRANSPORT

@projmgrauth
Set repo: nuraloumi

Parent canonical OCP task: NURALOUMI-20261117-QOL-R1-08-PLAYER-ENGINE-PLAYLIST-20261117-4FAFA2
Repo: /home/user/code/nuraloumi
Read AGENTS.md, docs/AGENT-LANES.md, docs/CONTRACTS.md, docs/QOL-RELEASE-20261117.md, both operator/fleet skills and worker contract. Inspect OCP tasks, claims and Git before work. Exclusively read-only source review: DO NOT edit player_engine.rs, bin/playerd.rs, bin/player.rs, video_engine.rs or any other product sources. No push, remote CI, deployment, power operations, Grate ABI edits. ARMv7 VFPv3-D16/no NEON, Cairo/wl_shm, 48px touch invariants. Report precise file:line evidence, bounded reproductions, tests (where practical), failures and a canonical final checkpoint. Do not claim dispatch receipt equals implementation completion.

## Objective
Read-only state-machine audit: verify loop off/one/all for audio/video EOS, next/previous boundary, jump-to-start, stop/stop, paused resume, relative seek at 0/EOF and unknown-duration behavior; seek-after-stop and failed track load; detect backend stale bus messages or status drift. Produce transition truth-table, at least three adversarial deterministic reproduction commands and concrete mitigations. No source changes.

## Acceptance
- Read-only independent evidence and exact test commands/results, no unreviewed source changes.
- Three high-risk edge case reviews minimum and honest limitations.
- Final agent_checkpoint with exact OCP identity supplied by operator, artifacts and specific follow-ups.

## MANDATORY RECURSIVE DELEGATION CONTRACT (propagate verbatim in every child)
Read /home/user/code/chatgpt-tooling/packages/chatgpt-operator-skill/SKILL.md and /home/user/code/chatgpt-tooling/packages/chatgpt-fleet-skill/SKILL.md. Use canonical chatgpt-ops / ChatGPT ABC operator dispatch yourself, not improvised browser clicks or legacy queue files. When you identify a substantial independent operation field that cannot be exhaustively solved within your bounded job, dispatch 2-8 narrowly scoped child agents with explicit ownership, acceptance criteria, evidence and followup tasks. Prefer 2 initially, respect physical fleet capacity, queue rather than blind browser growth, avoid duplicate/ambiguous replay. Delegate when justified, not to create recursion for its own sake; leaf-sized work can be completed directly. YOU MUST COPY THIS ENTIRE RECURSIVE DELEGATION CONTRACT INTO EVERY CHILD PROMPT and require every child to propagate it unchanged to its children. Retain integration responsibility: worker receipt is not completion, inspect artifacts/tests, dispatch corrective followups and record durable checkpoints. No child may exceed the hardware safety boundaries above. Use operator skill yourself even if workers are not needed, for task/checkpoint evidence.

SPECIAL CAMPAIGN RULE: Dispatch AT LEAST THREE actual distinct follow-up ChatGPT ABC operator OCP prompts based on your own investigation, with precise narrower ownership, tests and verifiable deliverables; copy this entire section unchanged into all three. Prefer independent verification, edge-case tests, integration seam checks over competing source edits. Where dispatch is fail-closed due to permissions or fleet capacity, preserve 3 fully executable, uniquely identified prompt files and report the exact blocker rather than bypassing the operator. Do not recursively generate duplicate or low-value tasks just to reach a count. Never duplicate in-flight tasks or create a boundless uncontrolled fanout.
