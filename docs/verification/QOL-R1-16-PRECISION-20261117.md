# R1-16 — independent precision and finite-progress audit (2026-11-17)

- Canonical OCP: `NURALOUMI-20261117-QOL-R1-16-CHILD-A-PRECISION-20261117-937F31`
- Parent: `NURALOUMI-20261117-QOL-R1-16-GENERIC-PRESSED-MOTION-20261117-41B424`
- Source reviewed read-only: `crates/nuraloumi-core/src/motion.rs` at HEAD `c9d63c256ba11c6af2bada4a0021907b154dd059` (current workspace included unrelated working-tree changes).
- Exclusive evidence scope: this file only. No product source, renderer, R1-01 claim, test, deployment, or hardware mutations.

## Commands and recorded exit codes

| Exact command | Exit | Evidence |
|---|---:|---|
| `cargo test -p nuraloumi-core --lib` | 0 | 36 passed; 0 failed; motion tests include pressed endpoints, cancellation, nonfinite bounds, reduced motion |
| `cargo clippy -p nuraloumi-core --all-targets -- -D warnings` | 0 | Finished dev profile, no diagnostics |
| `git rev-parse HEAD; git status --short; rg -n 'PressPhase\\|PressTransition' crates/nuraloumi-core` | 0 | HEAD above; unrelated local edits preserved; symbols located in motion.rs |

The host checks do not establish ARMv7/no-NEON runtime, compositor, wl_shm or Cairo raster qualification.

## Source-grounded findings

1. **PASS: exact finite release endpoints and termination.** `MotionClass::Fast.duration_ms()` is 120 (lines 9–17); `press_transition_ms` uses `normalized_progress` (lines 81–93), which clamps elapsed/120 to 0..1 (lines 106–109). `press_transition` (lines 54–76) yields full held treatment at zero release elapsed, settles with exactly zero accent and displacement at elapsed >=120, and sets `animating=false` for settled/full and all reduced-motion transitions. Unit coverage (lines 172–194) checks 0/60/120, but not 119/121 or `u32::MAX`.
2. **PASS: bounded finite paint for corrupt `f32` inputs.** At lines 55–60, NaN and both infinities are converted to progress 1.0 before cubic easing. The resulting `accent_mix` stays in [0,0.12], `translate_y` in [0,1], and `animating` becomes false for Released. Tests cover finite output for NaN, both infinities, and out-of-range values (lines 203–209), but do not explicitly assert `animating=false` on each nonfinite input. Defensive settling of -Inf rather than interpreting it as an early timestamp is documented policy, not a memory-safety bug.
3. **PASS with integration caveat: monotone finite progress for a single correctly computed nonnegative elapsed duration.** For `p∈[0,1]`, released amount is `1 - (1-(1-p)^3)=(1-p)^3` (lines 62–64 and 112–115), so it is nonincreasing; accent/inset are nonnegative and finite. Neither phase sequencing nor clock sampling is stored in `PressTransition`; those belong to shell callers. The pure function alone cannot guarantee no animation restart under repeated down/release or terminal frame scheduling. The most pertinent missing test samples all 0..=121 elapsed milliseconds, verifies nonincreasing values and ensures the first settled frame does not request another redraw.
4. **Integration hazard, not demonstrated core defect: wrapping `u32` timestamp subtraction.** `press_transition_ms` receives already-computed `elapsed_ms: u32` (lines 81–90). Passing `u32::MAX` is harmless (clamps to 1); computing a difference from timestamps using ordinary `u32` subtraction may panic in debug or wrap in release at a clock rollover. Caller must use a monotonic `Instant`/u64 duration with saturating conversion, or deliberately specified `wrapping_sub` only with a known bounded interval. Repeated release must reset the release anchor exactly once; cancelled/drag-out sequences must clear phase and prevent activation in shell input state.
5. **Potential visual discontinuity (design decision):** `Held` ignores progress (line 62), `Released` starts at full amount for zero elapsed (line 63), so a conventional held→release boundary is continuous. `Cancelled` immediately clears styling (line 64). Reduced motion suppresses translation, but keeps the held `accent_mix=0.12` (lines 67–75); reduced release becomes immediate. These match the documented contract. Hit-test geometry is not changed by this pure model, but must be verified at the renderer integration seam.

## Minimal corrective suggestions (not applied)

- Add parameterized exact edge assertions for release at 0, 1, 60, 119, 120, 121, `u32::MAX`; test `animating` false for NaN/+Inf/-Inf and all Idle/Cancelled/Reduced cases.
- Add a deterministic sweep over 0..=121 ms asserting finite, clamped, nonincreasing `accent_mix`/`translate_y`, and no further animation request after 120 ms. Assert transition continuity between Held and Released(0).
- In the shell integration owner, make the down/up/cancel state machine own release epochs and elapsed computation (monotonic clock, saturating duration), verify multiple rapid releases and rollover using a synthetic clock, and test that `animating=false` stops frame scheduling. Do not change hit regions or activation semantics.

## OCP follow-up dispatch boundary

Existing in-flight sister tasks were observed: R1-16 CHILD-B RASTER-GEOMETRY and CHILD-C REDUCED-CANCEL. Do not replay them. Three narrower proposed **distinct** follow-up identities are recorded here because the authorized write scope forbids additional prompt files, and available typed project tools have `ocp_tasks`, `ocp_task_create`, and `ocp_prompt_catalog` but expose no `ocp_task_dispatch` endpoint or writable prompt-catalog creation schema. Creating tasks without catalog prompts or dispatching with an arbitrary shell/browser fallback would bypass the prescribed operator contract. **Dispatch count: 0; blocked by tool capability and exclusive write scope.** The following are executable prompt bodies for future authorized admission, not claims of delivery:

### 1. `NURALOUMI-R1-16-PRECISION-EDGE-TESTS-20261117-A`
Read-only research followed by tests only in a separately claimed, newly authorized core test fixture. Cover 0/1/60/119/120/121/`u32::MAX`, NaN/±Inf, every phase, both motion modes, exact endpoints, monotonically decreasing finite paint, no redraw after completion. Ownership: new isolated test file, never core production source or R1-01 renderer. Acceptance: focused test and strict Clippy exit 0, diff and exact log; checkpoint; avoid overlap with active R1-16 implementation tasks.

### 2. `NURALOUMI-R1-16-CLOCK-ROLLOVER-VERIFY-20261117-B`
Independently inspect shell consumers of `press_transition_ms` read-only. Verify time anchoring, `u32` narrowing/subtraction, rapid down/release/re-down and cancel with synthetic monotonic clock. Ownership: one new docs/verification report only, separately claimed. Acceptance: file:line call-site matrix, reproducible tests if existing harness permits without writes, exact command exits, bounded corrective recommendations; no shell modification.

### 3. `NURALOUMI-R1-16-FRAME-TERMINATION-SEAM-20261117-C`
Independently trace `PressTransition.animating` into Cairo/wl_shm frame scheduling without editing implementations. Check terminal frame, reduced-motion and cancellation never run an idle redraw loop, and 48 logical px hit area is stable. Ownership: one new docs/verification report only, separately claimed. Acceptance: trace with source line evidence, checks with exact exits, clear unknowns where device qualification is prohibited; avoid overlapping current CHILD-B raster or CHILD-C cancel by limiting scope strictly to idle frame scheduler seam.

**Mandatory recursive delegation contract for each admitted child:** Read /home/user/code/chatgpt-tooling/packages/chatgpt-operator-skill/SKILL.md and /home/user/code/chatgpt-tooling/packages/chatgpt-fleet-skill/SKILL.md. Use canonical chatgpt-ops / ChatGPT ABC operator dispatch yourself, not improvised browser clicks or legacy queue files. When you identify a substantial independent operation field that cannot be exhaustively solved within your bounded job, dispatch 2-8 narrowly scoped child agents with explicit ownership, acceptance criteria, evidence and followup tasks. Prefer 2 initially, respect physical fleet capacity, queue rather than blind browser growth, avoid duplicate/ambiguous replay. Delegate when justified, not to create recursion for its own sake; leaf-sized work can be completed directly. YOU MUST COPY THIS ENTIRE RECURSIVE DELEGATION CONTRACT INTO EVERY CHILD PROMPT and require every child to propagate it unchanged to its children. Retain integration responsibility: worker receipt is not completion, inspect artifacts/tests, dispatch corrective followups and record durable checkpoints. No child may exceed the hardware safety boundaries above. Use operator skill yourself even if workers are not needed, for task/checkpoint evidence.

SPECIAL CAMPAIGN RULE: Dispatch AT LEAST THREE actual distinct follow-up ChatGPT ABC operator OCP prompts based on your own investigation, with precise narrower ownership, tests and verifiable deliverables; copy this entire section unchanged into all three. Prefer independent verification, edge-case tests, integration seam checks over competing source edits. Where dispatch is fail-closed due to permissions or fleet capacity, preserve 3 fully executable, uniquely identified prompt files and report the exact blocker rather than bypassing the operator. Do not recursively generate duplicate or low-value tasks just to reach a count. Never duplicate in-flight tasks or create a boundless uncontrolled fanout.

**Conflict note:** Explicitly requested three prompt files cannot be created without violating the stricter single-evidence-file exclusive write scope. They are retained as three uniquely identified prompt sections in this permitted evidence file, requiring separate authorization to materialize/dispatch.
