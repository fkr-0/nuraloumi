# NURALOUMI-967EDF-REINIT-PANGO-LIBCLIENT-20261119-D03B

Set repo: nuraloumi. Unique OCP follow-up identity: NURALOUMI-967EDF-REINIT-PANGO-LIBCLIENT-20261119-D03B.
Parent: NURALOUMI-NURALOUMI-967EDF-LEAKPROVENANCE-FCFINI-FOLLOWUP--20261119-812F9F.

Independently assess optional PangoCairoText and GTK/WPE/library coexistence risk if cairo_debug_reset_static_data is called in a library client, not process termination. Verify Pango layout/font description guards and references, host-only deterministic sanitizer fixture where available, and report safe shutdown ordering. Avoid any implementation edits. Accept: explicit dependency owner map, repeatable sanitizer results and no claim of safe global reset while another library client is active.

Required: read AGENTS.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md. Check OCP duplicates before dispatch. Baseline host Cairo 1.18.6 Fontconfig 2.18.3 GCC 16.2.1: two draws with no reset exit 1 under LSan 357 bytes/5 allocations; reset only between draws also exits 1 with same allocation; reset at end exits 0. Keep all test evidence under /tmp/nuraloumi-967edf-fcfini-reinit-pango-client-*. No Nura device, Wayland, SSH, CI, deployment, reboot, Git network, next.md, source edits, or other worker fixtures. Record an exact canonical task agent_checkpoint with evidence and structured followups. Receipt is separate from acceptance.

## Endliche, fixorientierte Fortsetzung (ersetzt rekursive Delegationspflicht)
- Diese Aufgabe endet bei bestandener Akzeptanz oder einem belegten Blocker mit exaktem agent_checkpoint; standardmäßig **null** Folgetasks.
- Kein Mindestkontingent, keine Pflicht zu drei Kindern, kein Vererben einer Verzweigungsanweisung.
- Nur für einen neu reproduzierten, nicht bereits abgedeckten P0/P1-Befund außerhalb dieses Scopes darf **höchstens ein** enger Kindvorschlag im Checkpoint mit Repro, Dateibesitz, Negativkontrolle und Abnahme stehen. Nicht selbst dispatchen.
- Kohortenbudget NURALOUMI-FIX-CLOSURE-20261121-V1: maximal **vier** zusätzliche Leaves insgesamt; nur der Conductor prüft Katalog/OCP-Dubletten und lässt zu. Kinder erzeugen keine Enkel.
- Existierende laufende/queued Arbeit und fremde Änderungen nicht überschreiben; die ursprüngliche OCP-Identität und ihren Evidenzpfad bewahren. Ergebnisbeleg != Dispatchbeleg.
- Nach Fix: gezielte Tests und unabhängiges Integrationsreview; bestanden/irrelevant/blockiert sind terminale Zustände, kein Anlass zur erneuten Expansion.
