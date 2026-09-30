# Canonical Wave-1 prompts

These files are the source text registered into the native ChatGPT OCP prompt catalog for the first NuraLoumi implementation wave.

Task identity is durable. If a worker must retry or continue, reuse the same OCP task rather than creating a legacy queue item or replacement task.

All prompts require:
- @projmgrauth project tooling;
- exact write-scope ownership;
- transactional writes/claims where the project workflow requires them;
- narrow tests first and workspace checks before handoff;
- a durable phase result/checkpoint with changed paths and remaining risk;
- no direct modification of another lane.
