@projmgrauth
Set repo: nuraloumi

# NuraLoumi QoL R1-01 child (2026-11-17)
Canonical parent: NURALOUMI-20261117-QOL-R1-01-WIDGET-SEMANTICS-20261117-3A4458
Use the actual OCP-injected taskId for your checkpoint identity. Read AGENTS.md, architecture/contracts, docs/QOL-WIDGET-SEMANTICS-20261117.md and the operator/fleet skills and worker contract. Inspect Git and existing OCP tasks/claims before any edits. Do not modify the parent's claimed source paths. All work local-only; no push, remote CI, production install, hardware access, suspend/power, private ABI edits or release tags. ARMv7 VFPv3-D16/no NEON, Cairo/wl_shm fallback and 48 logical px minimum. Perform independent read-only review initially; scoped new test files/docs are allowable only with nonoverlapping path claims. Checkpoint actual evidence, not guesses. No duplicate dispatch of already in-flight siblings.

Unique proposed task identity: NURALOUMI-20261117-QOL-R1-01-CHILD-A-RANGE-SERDE-20261117-7E1AC1
## Range, serialization and validation boundary
Audit CompactControl and MenuAction::SetValue against min=i32::MIN/max=i32::MAX, step=0, invalid range, non-divisible max, disabled controls, duplicate IDs and old MenuModel JSON fixtures. Verify concrete source locations and test stdout; do not change model.rs/validation.rs. Create only new tests under crates/nuraloumi-core/tests/ after acquiring a claim if a P0 is found. Report a precise fix recommendation or non-overlapping test commit.

Acceptance: exact paths and line evidence, relevant targeted test stdout/exit codes, zero overlapping writes, durable agent_checkpoint receipt and corrective follow-ups if needed.

## Endliche, fixorientierte Fortsetzung (ersetzt rekursive Delegationspflicht)
- Diese Aufgabe endet bei bestandener Akzeptanz oder einem belegten Blocker mit exaktem agent_checkpoint; standardmäßig **null** Folgetasks.
- Kein Mindestkontingent, keine Pflicht zu drei Kindern, kein Vererben einer Verzweigungsanweisung.
- Nur für einen neu reproduzierten, nicht bereits abgedeckten P0/P1-Befund außerhalb dieses Scopes darf **höchstens ein** enger Kindvorschlag im Checkpoint mit Repro, Dateibesitz, Negativkontrolle und Abnahme stehen. Nicht selbst dispatchen.
- Kohortenbudget NURALOUMI-FIX-CLOSURE-20261121-V1: maximal **vier** zusätzliche Leaves insgesamt; nur der Conductor prüft Katalog/OCP-Dubletten und lässt zu. Kinder erzeugen keine Enkel.
- Existierende laufende/queued Arbeit und fremde Änderungen nicht überschreiben; die ursprüngliche OCP-Identität und ihren Evidenzpfad bewahren. Ergebnisbeleg != Dispatchbeleg.
- Nach Fix: gezielte Tests und unabhängiges Integrationsreview; bestanden/irrelevant/blockiert sind terminale Zustände, kein Anlass zur erneuten Expansion.
