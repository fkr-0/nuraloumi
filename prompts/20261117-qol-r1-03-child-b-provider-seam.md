@projmgrauth
Set repo: nuraloumi

Follow canonical injected OCP taskId; never create legacy next.md. Parent: NURALOUMI-20261117-QOL-R1-03-NOTIFICATIONS-DAEMON-20261117-D9409D.
Read AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md, docs/QOL-RELEASE-20261117.md and the fleet worker contract. Isolate worktree or claim exact non-overlapping files. Local-only: no push, CI, deployment, SL101 access, suspend or reboot. No changes to unrelated dirty files, no Grate/GStreamer ABI changes. ARMv7 VFPv3-D16/no NEON, Cairo+wl_shm fallback and 48 logical-pixel hit areas are invariants. Checkpoint milestone and final status with exact OCP identity and receipts.

# Independent center/provider semantics audit

Canonical proposed OCP identity: `NURALOUMI-20261117-QOL-R1-03-B-PROVIDER-SEAM`

Read-only audit of crates/nuraloumi-providers/src/notifications.rs and crates/nuraloumi-shell/src/bin/notifications.rs. Own only docs/verification/notifications-r1-03-b.md if needed. Verify D-Bus provider selection vs passive Dunst fallback, error handling, known-ID clear atomic expectations, stale snapshot/replace races, action validation and the UI refreshing actual arrivals; test with private dbus-run-session, not the real user bus. Report whether missing dunstctl produces explicit unavailable and never a fake success; whether active and historical notifications are distinguishable; bounded operations/timeouts. Include exact commands/results/source line numbers and actionable fail-closed fixes. Do not touch parent-owned source.

Acceptance: independent reproducible evidence, at least one adversarial or failure-case test, exact paths/line numbers, no overlaps, final agent_checkpoint with artifacts and honest known limits. Child dispatch only for real uncovered independent scope, bounded by fleet capacity; never duplicate existing tasks.

## Endliche, fixorientierte Fortsetzung (ersetzt rekursive Delegationspflicht)
- Diese Aufgabe endet bei bestandener Akzeptanz oder einem belegten Blocker mit exaktem agent_checkpoint; standardmäßig **null** Folgetasks.
- Kein Mindestkontingent, keine Pflicht zu drei Kindern, kein Vererben einer Verzweigungsanweisung.
- Nur für einen neu reproduzierten, nicht bereits abgedeckten P0/P1-Befund außerhalb dieses Scopes darf **höchstens ein** enger Kindvorschlag im Checkpoint mit Repro, Dateibesitz, Negativkontrolle und Abnahme stehen. Nicht selbst dispatchen.
- Kohortenbudget NURALOUMI-FIX-CLOSURE-20261121-V1: maximal **vier** zusätzliche Leaves insgesamt; nur der Conductor prüft Katalog/OCP-Dubletten und lässt zu. Kinder erzeugen keine Enkel.
- Existierende laufende/queued Arbeit und fremde Änderungen nicht überschreiben; die ursprüngliche OCP-Identität und ihren Evidenzpfad bewahren. Ergebnisbeleg != Dispatchbeleg.
- Nach Fix: gezielte Tests und unabhängiges Integrationsreview; bestanden/irrelevant/blockiert sind terminale Zustände, kein Anlass zur erneuten Expansion.
