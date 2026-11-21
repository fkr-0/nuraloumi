## Task identity
- OCP task: `NURALOUMI-20261117-QOL-R1-04-VERIFY-MEDIA-02`
- Parent: `NURALOUMI-20261117-QOL-R1-04-PANEL-CONTROL-20261117-1E0D5B`
- Priority: 65; exclusive review lane, no source ownership

@projmgrauth
Set repo: nuraloumi

Read AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md and docs/QOL-RELEASE-20261117.md. Read both operator and fleet skills and worker-contract.md. Local only: no remote CI, push, live installation, reboot/suspend, or Grate/VDE/GStreamer ABI changes. Respect ARMv7 VFPv3-D16 without NEON, wl_shm CPU fallback and 48px primary touch targets. Inspect concurrent OCP tasks and path claims before work. Read-only review; do not edit production sources or create a legacy next.md. Record exact task ID checkpoints with path:line, test output/exit codes, counterexamples and follow-up fixes. Coordinate with QOL-R1-04 owner without acquiring their panel.rs/menu.rs/catalog claims.

## Objective and acceptance
Independently audit persistent Player panel affordance and audio mute truthfulness. Verify provider snapshot/ack/status/stale transitions, mute toggles at nonzero and 0% volume, and missing player fallback. Inspect panel.rs and menu.rs, provider audio action mapping. Run targeted panel/audio tests and document untested edge cases. Provide concrete failures and patch recommendations only (no source edits).

Acceptance: identify every inspected source path/line, run and record test commands with stdout and exit code, checkpoint findings, and supply a bounded corrective follow-up if required.

## Endliche, fixorientierte Fortsetzung (ersetzt rekursive Delegationspflicht)
- Diese Aufgabe endet bei bestandener Akzeptanz oder einem belegten Blocker mit exaktem agent_checkpoint; standardmäßig **null** Folgetasks.
- Kein Mindestkontingent, keine Pflicht zu drei Kindern, kein Vererben einer Verzweigungsanweisung.
- Nur für einen neu reproduzierten, nicht bereits abgedeckten P0/P1-Befund außerhalb dieses Scopes darf **höchstens ein** enger Kindvorschlag im Checkpoint mit Repro, Dateibesitz, Negativkontrolle und Abnahme stehen. Nicht selbst dispatchen.
- Kohortenbudget NURALOUMI-FIX-CLOSURE-20261121-V1: maximal **vier** zusätzliche Leaves insgesamt; nur der Conductor prüft Katalog/OCP-Dubletten und lässt zu. Kinder erzeugen keine Enkel.
- Existierende laufende/queued Arbeit und fremde Änderungen nicht überschreiben; die ursprüngliche OCP-Identität und ihren Evidenzpfad bewahren. Ergebnisbeleg != Dispatchbeleg.
- Nach Fix: gezielte Tests und unabhängiges Integrationsreview; bestanden/irrelevant/blockiert sind terminale Zustände, kein Anlass zur erneuten Expansion.
