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

## Endliche, fixorientierte Fortsetzung (ersetzt rekursive Delegationspflicht)
- Diese Aufgabe endet bei bestandener Akzeptanz oder einem belegten Blocker mit exaktem agent_checkpoint; standardmäßig **null** Folgetasks.
- Kein Mindestkontingent, keine Pflicht zu drei Kindern, kein Vererben einer Verzweigungsanweisung.
- Nur für einen neu reproduzierten, nicht bereits abgedeckten P0/P1-Befund außerhalb dieses Scopes darf **höchstens ein** enger Kindvorschlag im Checkpoint mit Repro, Dateibesitz, Negativkontrolle und Abnahme stehen. Nicht selbst dispatchen.
- Kohortenbudget NURALOUMI-FIX-CLOSURE-20261121-V1: maximal **vier** zusätzliche Leaves insgesamt; nur der Conductor prüft Katalog/OCP-Dubletten und lässt zu. Kinder erzeugen keine Enkel.
- Existierende laufende/queued Arbeit und fremde Änderungen nicht überschreiben; die ursprüngliche OCP-Identität und ihren Evidenzpfad bewahren. Ergebnisbeleg != Dispatchbeleg.
- Nach Fix: gezielte Tests und unabhängiges Integrationsreview; bestanden/irrelevant/blockiert sind terminale Zustände, kein Anlass zur erneuten Expansion.
