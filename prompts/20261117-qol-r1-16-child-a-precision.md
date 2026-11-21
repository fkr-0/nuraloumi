@projmgrauth
Set repo: nuraloumi

Canonical parent OCP: NURALOUMI-20261117-QOL-R1-16-GENERIC-PRESSED-MOTION-20261117-41B424.
Read AGENTS.md, DESIGN.md, ROADMAP.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md, docs/QOL-RELEASE-20261117.md, docs/ux/QOL-PRESSED-FEEDBACK-20261117.md and both operator/fleet skills. Inspect active claims and existing OCP tasks to avoid duplication. Strict local only: no push, public CI/PR, deploy, install, hardware SSH, reboot, suspend, GPU ABI changes. Preserve ARMv7/no-NEON, wl_shm/Cairo CPU compatibility, 48 logical px hit regions and keyboard parity. Use ws-bridge exclusive claim on the single evidence file named here; all product sources read-only. Commit only owned file; report exact command exit codes and task ID; checkpoint with evidence. Do not edit existing R1-01 renderer claims. Do not generate next.md.

# R1-16 independent Precision and finite-progress audit

Unique child ID: NURALOUMI-R1-16-CHILD-A-PRECISION-20261117.
Exclusive write scope: docs/verification/QOL-R1-16-PRECISION-20261117.md ONLY. All other paths read-only.

## Bounded job
Review motion.rs new PressPhase/PressTransition for stable 120ms exact endpoints, f32 NaN/Inf and >u32 timestamp overflow, repeated down/release timing, monotonic finite paint values, animating flag termination. Run focused cargo test -p nuraloumi-core --lib and cargo clippy -p nuraloumi-core --all-targets -- -D warnings. Document concrete findings with file:line, test outputs/exit codes, minimal corrective patch suggestions (do not modify source).

## Acceptance
- One evidence report with source-grounded findings, exact commands and exit codes, and explicit no-edit confirmation for product sources.
- Independent review, no duplicated R1-16 implementation and no R1-01 renderer edits.
- Checkpoint exact task identity and identify actionable follow-up only for uncovered substantial work.

## Endliche, fixorientierte Fortsetzung (ersetzt rekursive Delegationspflicht)
- Diese Aufgabe endet bei bestandener Akzeptanz oder einem belegten Blocker mit exaktem agent_checkpoint; standardmäßig **null** Folgetasks.
- Kein Mindestkontingent, keine Pflicht zu drei Kindern, kein Vererben einer Verzweigungsanweisung.
- Nur für einen neu reproduzierten, nicht bereits abgedeckten P0/P1-Befund außerhalb dieses Scopes darf **höchstens ein** enger Kindvorschlag im Checkpoint mit Repro, Dateibesitz, Negativkontrolle und Abnahme stehen. Nicht selbst dispatchen.
- Kohortenbudget NURALOUMI-FIX-CLOSURE-20261121-V1: maximal **vier** zusätzliche Leaves insgesamt; nur der Conductor prüft Katalog/OCP-Dubletten und lässt zu. Kinder erzeugen keine Enkel.
- Existierende laufende/queued Arbeit und fremde Änderungen nicht überschreiben; die ursprüngliche OCP-Identität und ihren Evidenzpfad bewahren. Ergebnisbeleg != Dispatchbeleg.
- Nach Fix: gezielte Tests und unabhängiges Integrationsreview; bestanden/irrelevant/blockiert sind terminale Zustände, kein Anlass zur erneuten Expansion.
