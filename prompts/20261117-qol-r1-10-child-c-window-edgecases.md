@projmgrauth
Set repo: nuraloumi

# High-risk full-player seek/input and window state review

Canonical unique OCP task ID: `NURALOUMI-20261117-QOL-R1-10-CHILD-C-WINDOW-EDGE`. Parent `NURALOUMI-20261117-QOL-R1-10-PLAYER-EXPANDED-WINDOW-20261117-AF7E58`.
Local-only. Repo /home/user/code/nuraloumi. Read AGENTS.md, docs/QOL-RELEASE-20261117.md, docs/CONTRACTS.md, docs/AGENT-LANES.md, docs/ux/QOL-PLAYER-WINDOW-20261117.md, operator and fleet skills and worker contract first. Inspect git, OCP in-flight tasks and claims. No push/PR/remote CI/install/reboot, no private GPU ABI changes, no destructive device access. Preserve concurrent files and no duplicate OCP work.

## Task and ownership
Read player_window.rs and campaign docs; own only NEW docs/ux/QOL-PLAYER-EDGE-REVIEW-20261117.md (optionally new isolated test file) after claim. Verify seek precision for extremely long durations, NaN, short slider bounds, pointer drag release/cancel semantics, keyboard parity, close/reopen, single-instance focus, and missing compositor capability. Do not edit owned lane 10 files. Run rustc unit tests and report correctness defects with patches proposed, not applied to others' claims.

## Acceptance
Evidence matrix with at least three independently exercised high-risk edge cases and honest untested gates.
Report commands, results, exact artifact paths and checkpoint under the canonical child task ID. Dispatch receipts do not constitute completed verification.

## Endliche, fixorientierte Fortsetzung (ersetzt rekursive Delegationspflicht)
- Diese Aufgabe endet bei bestandener Akzeptanz oder einem belegten Blocker mit exaktem agent_checkpoint; standardmäßig **null** Folgetasks.
- Kein Mindestkontingent, keine Pflicht zu drei Kindern, kein Vererben einer Verzweigungsanweisung.
- Nur für einen neu reproduzierten, nicht bereits abgedeckten P0/P1-Befund außerhalb dieses Scopes darf **höchstens ein** enger Kindvorschlag im Checkpoint mit Repro, Dateibesitz, Negativkontrolle und Abnahme stehen. Nicht selbst dispatchen.
- Kohortenbudget NURALOUMI-FIX-CLOSURE-20261121-V1: maximal **vier** zusätzliche Leaves insgesamt; nur der Conductor prüft Katalog/OCP-Dubletten und lässt zu. Kinder erzeugen keine Enkel.
- Existierende laufende/queued Arbeit und fremde Änderungen nicht überschreiben; die ursprüngliche OCP-Identität und ihren Evidenzpfad bewahren. Ergebnisbeleg != Dispatchbeleg.
- Nach Fix: gezielte Tests und unabhängiges Integrationsreview; bestanden/irrelevant/blockiert sind terminale Zustände, kein Anlass zur erneuten Expansion.
