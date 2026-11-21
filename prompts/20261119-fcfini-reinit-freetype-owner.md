# NURALOUMI-967EDF-REINIT-FTOWNER-LIFETIME-20261119-D03C

Set repo: nuraloumi. Unique OCP follow-up identity: NURALOUMI-967EDF-REINIT-FTOWNER-LIFETIME-20261119-D03C.
Parent: NURALOUMI-NURALOUMI-967EDF-LEAKPROVENANCE-FCFINI-FOLLOWUP--20261119-812F9F.

Independently examine PackagedFontText Rc<FtOwner>, Cairo FontFace user-data, scratch Context, and per-frame Context drop order; test renderer/backend destruction before terminal Cairo reset without Fontconfig family lookup. Use host-only isolated sanitizer fixture and exact font file; distinguish Fontconfig leak vs FT lifetime. Accept: code-level ownership graph, repeatable sanitized pass/fail data and concrete regression criteria.

Required: read AGENTS.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md. Check OCP duplicates before dispatch. Baseline host Cairo 1.18.6 Fontconfig 2.18.3 GCC 16.2.1: two draws with no reset exit 1 under LSan 357 bytes/5 allocations; reset only between draws also exits 1 with same allocation; reset at end exits 0. Keep all test evidence under /tmp/nuraloumi-967edf-fcfini-reinit-freetype-owner-*. No Nura device, Wayland, SSH, CI, deployment, reboot, Git network, next.md, source edits, or other worker fixtures. Record an exact canonical task agent_checkpoint with evidence and structured followups. Receipt is separate from acceptance.

## Endliche, fixorientierte Fortsetzung (ersetzt rekursive Delegationspflicht)
- Diese Aufgabe endet bei bestandener Akzeptanz oder einem belegten Blocker mit exaktem agent_checkpoint; standardmäßig **null** Folgetasks.
- Kein Mindestkontingent, keine Pflicht zu drei Kindern, kein Vererben einer Verzweigungsanweisung.
- Nur für einen neu reproduzierten, nicht bereits abgedeckten P0/P1-Befund außerhalb dieses Scopes darf **höchstens ein** enger Kindvorschlag im Checkpoint mit Repro, Dateibesitz, Negativkontrolle und Abnahme stehen. Nicht selbst dispatchen.
- Kohortenbudget NURALOUMI-FIX-CLOSURE-20261121-V1: maximal **vier** zusätzliche Leaves insgesamt; nur der Conductor prüft Katalog/OCP-Dubletten und lässt zu. Kinder erzeugen keine Enkel.
- Existierende laufende/queued Arbeit und fremde Änderungen nicht überschreiben; die ursprüngliche OCP-Identität und ihren Evidenzpfad bewahren. Ergebnisbeleg != Dispatchbeleg.
- Nach Fix: gezielte Tests und unabhängiges Integrationsreview; bestanden/irrelevant/blockiert sind terminale Zustände, kein Anlass zur erneuten Expansion.
