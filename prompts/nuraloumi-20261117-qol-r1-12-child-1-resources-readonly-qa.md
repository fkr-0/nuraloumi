# NuraLoumi QoL R1 Lane 12 independent verification 1

Canonical follow-up identity: NURALOUMI-20261117-QOL-R1-12-CHILD-1-RESOURCES-READONLY-QA
Project: nuraloumi; original parent task NURALOUMI-20261117-QOL-R1-12-SYSTEM-APPLETS-20261117-33B032.
Read AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md and both operator/fleet skills plus worker contract. Inspect existing OCP tasks/claims first; do not duplicate active work. Scope: Read-only independent audit of resources.rs post-commit 1508bbb: verify MenuItem::status layouts/hit zones, refresh pointer press/release mismatch, multi-touch cancellation, F5 stale-snapshot error handling, filter 'r' and Unicode. Include deterministic fixture scene/hit-region evidence; cargo test -p nuraloumi-shell --bin nuraloumi-resources. No source edits or device access.
Acceptance: code-path and before/after evidence; reproduce or disprove at least one high-risk edge; exact command/test results; documented defects and self-contained followups; agent_checkpoint with actual injected canonical taskId. Read-only verification: no writes to parent-owned src/bin files, no commits, push, remote CI or live device actions. Respect ARMv7/no NEON, Cairo+wl_shm CPU fallback, 48px touch and no rendering-time commands.

## Endliche, fixorientierte Fortsetzung (ersetzt rekursive Delegationspflicht)
- Diese Aufgabe endet bei bestandener Akzeptanz oder einem belegten Blocker mit exaktem agent_checkpoint; standardmäßig **null** Folgetasks.
- Kein Mindestkontingent, keine Pflicht zu drei Kindern, kein Vererben einer Verzweigungsanweisung.
- Nur für einen neu reproduzierten, nicht bereits abgedeckten P0/P1-Befund außerhalb dieses Scopes darf **höchstens ein** enger Kindvorschlag im Checkpoint mit Repro, Dateibesitz, Negativkontrolle und Abnahme stehen. Nicht selbst dispatchen.
- Kohortenbudget NURALOUMI-FIX-CLOSURE-20261121-V1: maximal **vier** zusätzliche Leaves insgesamt; nur der Conductor prüft Katalog/OCP-Dubletten und lässt zu. Kinder erzeugen keine Enkel.
- Existierende laufende/queued Arbeit und fremde Änderungen nicht überschreiben; die ursprüngliche OCP-Identität und ihren Evidenzpfad bewahren. Ergebnisbeleg != Dispatchbeleg.
- Nach Fix: gezielte Tests und unabhängiges Integrationsreview; bestanden/irrelevant/blockiert sind terminale Zustände, kein Anlass zur erneuten Expansion.
