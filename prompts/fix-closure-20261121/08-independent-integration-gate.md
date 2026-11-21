# Verifiziere unabhängige NuraLoumi-Integrationsreife
## Ziel
Führe ein unabhängiges, ergebnisorientiertes Review der Tasks 01–07 und der tatsächlichen aktuellen HEAD-Änderungen durch. Keine Quoten-Follow-ups.
## Startbedingung
Diese Task bleibt DRAFT/HOLD, bis vorgelagerte Aufgaben terminale Checkpoints/Reviewer-Belege oder klare Blocker geliefert haben. Nicht parallel mit ihren Source-Änderungen ausführen.
## Scope und Akzeptanz
- Lies alle Vorgänger-Checkpoints und überprüfe Diff, Eigentum, exakte Tests und negative Cases; wiederhole die relevante host-only Matrix aus frischem Stand.
- Reconcile OCP/Backlog: Nur durch exakte Identität und Evidenz nachgewiesene Dubletten/obsolete Kinder als supersede vorschlagen/mit geprüfter Autorität schließen. Keine Pauschal-Massenbeendigung.
- Release-Matrix: host, clean committed HEAD, full tests, Clippy, Browser engine, ARMv7 artifacts, remote official CI, nested Wayland, reale SL101-Touch/Keyboard/Performance getrennt mit PASS/FAIL/PENDING. 0.0.8 bleibt HOLD bis alle zwingenden Gates mit exakter Provenienz vorliegen.
- Endzustand: reviewed and complete / blocked with finite prerequisites. Kein neuer Parent und kein eigenes Kind.

## Verbindlicher Arbeitsvertrag
- Lies AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md und die relevanten aktuellen Qualifikationsbelege. Arbeitsbasis ist der tatsächlich angetroffene HEAD, nicht eine ältere Bericht-SHA.
- Nutze ws-bridge/projmgrauth, prüfe Status, aktive Worker und Path-Claims; übernehme keine fremden Änderungen. Bei überlappendem Eigentum nur prüfen und einen konkreten Patchvorschlag hinterlassen; getrennte Worktrees/Claims und unabhängiges Review für Implementation.
- Lokal/host-only, keine Git-Netzwerkoperationen, Git-Push, Remote-CI, Installation auf dem SL101, SSH, laufende Desktop-/Sway-/Wayland-Session-Änderung, Reboot/Suspend, Deployment oder Veröffentlichung. ARMv7 VFPv3-D16 ohne NEON, wl_shm/Cairo-Pixman-Fallback und sichere Power-State-Gates bleiben zwingend.
- Bilde reproduzierbare positive und negative Tests, dokumentiere Befehl, Exit, relevante Logs, Commit/Datei-Hashes, PASS/FAIL/PENDING, und schreibe nach Meilensteinen einen agent_checkpoint mit der **exakten injizierten OCP-ID**. Keine legacy next.md und keine falsche Release- oder Hardware-Freigabe.
- **Semi-erweiternd, endlich:** Normalfall 0 Kinder. Nur für einen neu reproduzierten, deduplizierten P0/P1 außerhalb des Scopes höchstens **einen** Kindvorschlag im Checkpoint; kein Selbstdurchdisponieren. Der Conductor entscheidet innerhalb des globalen Budgets von **4** Zusatz-Leaves. Kein Enkel, kein Mindest-Fan-out. Bei bestandener Akzeptanz sofort terminal mit Belegen. Siehe POLICY.md dieses Ordners.
