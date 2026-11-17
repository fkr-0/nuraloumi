@projmgrauth
Set repo: nuraloumi

# Independent playerd single-daemon IPC lifecycle verification

Canonical unique OCP task ID: `NURALOUMI-20261117-QOL-R1-10-CHILD-B-PLAYERD-SEAM`. Parent `NURALOUMI-20261117-QOL-R1-10-PLAYER-EXPANDED-WINDOW-20261117-AF7E58`.
Local-only. Repo /home/user/code/nuraloumi. Read AGENTS.md, docs/QOL-RELEASE-20261117.md, docs/CONTRACTS.md, docs/AGENT-LANES.md, docs/ux/QOL-PLAYER-WINDOW-20261117.md, operator and fleet skills and worker contract first. Inspect git, OCP in-flight tasks and claims. No push/PR/remote CI/install/reboot, no private GPU ABI changes, no destructive device access. Preserve concurrent files and no duplicate OCP work.

## Task and ownership
Read player.rs, playerd.rs, player_engine.rs and player_window.rs; own only NEW tests/player_window_ipc_contract.rs or docs/ux/QOL-PLAYER-IPC-SEAM-20261117.md after claim. Verify no duplicate daemon/pipeline, bounded socket writes/reads, daemon absent, daemon crash/restart, continued playback after view closes, and protocol seek limits. Do not alter lane 08/09 source. Run fixtures and record before/after evidence.

## Acceptance
Independent evidence and reproducible tests for shared playerd socket without real media device.
Report commands, results, exact artifact paths and checkpoint under the canonical child task ID. Dispatch receipts do not constitute completed verification.

## MANDATORY RECURSIVE DELEGATION CONTRACT (propagate verbatim in every child)
Read /home/user/code/chatgpt-tooling/packages/chatgpt-operator-skill/SKILL.md and /home/user/code/chatgpt-tooling/packages/chatgpt-fleet-skill/SKILL.md. Use canonical chatgpt-ops / ChatGPT ABC operator dispatch yourself, not improvised browser clicks or legacy queue files. When you identify a substantial independent operation field that cannot be exhaustively solved within your bounded job, dispatch 2-8 narrowly scoped child agents with explicit ownership, acceptance criteria, evidence and followup tasks. Prefer 2 initially, respect physical fleet capacity, queue rather than blind browser growth, avoid duplicate/ambiguous replay. Delegate when justified, not to create recursion for its own sake; leaf-sized work can be completed directly. YOU MUST COPY THIS ENTIRE RECURSIVE DELEGATION CONTRACT INTO EVERY CHILD PROMPT and require every child to propagate it unchanged to its children. Retain integration responsibility: worker receipt is not completion, inspect artifacts/tests, dispatch corrective followups and record durable checkpoints. No child may exceed the hardware safety boundaries above. Use operator skill yourself even if workers are not needed, for task/checkpoint evidence.

SPECIAL CAMPAIGN RULE: Dispatch AT LEAST THREE actual distinct follow-up ChatGPT ABC operator OCP prompts based on your own investigation, with precise narrower ownership, tests and verifiable deliverables; copy this entire section unchanged into all three. Prefer independent verification, edge-case tests, integration seam checks over competing source edits. Where dispatch is fail-closed due to permissions or fleet capacity, preserve 3 fully executable, uniquely identified prompt files and report the exact blocker rather than bypassing the operator. Do not recursively generate duplicate or low-value tasks just to reach a count. Never duplicate in-flight tasks or create a boundless uncontrolled fanout.
