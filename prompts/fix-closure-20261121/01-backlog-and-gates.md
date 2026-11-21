# Konsolidiere NuraLoumi OCP-Backlog und Release-Gates
## Ziel
Erstelle den überprüfbaren Fix-Ledger aus den 339 offenen NuraLoumi-Einträgen; trenne echte reproduzierte Release-Blocker, noch nötige Tests, Dubletten, rein rekursive Experimente und unklare Zustellung, ohne erledigte Arbeit zu erfinden.
## Eigentum und Grenzen
Nur neue Berichte in docs/qualification/ oder diesem Kampagnenordner; keine Codeänderung, keine OCP-Massen-Terminalisierung. Prüfe alle 334 queued und 5 draft über bounded/native OCP-Sichten; verlorene Checkpoints anhand ID nachführen. Stichtag 2026-11-21: 325 task-IDs enthalten 20261119, 14 enthalten 20261120; drei Stichproben hatten delivery_confirmed=false und pre-delivery ABC-Verweigerung. Das beweist nicht denselben Zustand aller Einträge.
## Akzeptanz
- CSV/Markdown-gestützte, nach OCP-ID rückverfolgbare KEEP / MERGE-CANDIDATE / SUPERSEDE-PROPOSAL / BLOCKED / UNKNOWN-Klassifikation, mit Beweis und Besitzer; keine Altersregel als Abschlussbeleg.
- Release-Gates gegen aktuelle SHA prüfen: aktueller Clean-HEAD-Host-CI, Browser-Testvertrag, ARMv7/CI-Artefakt, Version-Pin, Nested-Wayland, SL101 (separat nur nach Freigabe).
- Maximal zwei konkrete nachfolgende priorisierte Fix-Vorschläge ohne neue Task-Erzeugung; independent gate 08 erst nach belegten Ergebnissen.
## Prüfungen
`chatgpt-ops ps --project nuraloumi --limit 400 --json`; `git status --short`; `cargo fmt --all -- --check`. Kein Massen-Clean/Stop.

## Verbindlicher Arbeitsvertrag
- Lies AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md und die relevanten aktuellen Qualifikationsbelege. Arbeitsbasis ist der tatsächlich angetroffene HEAD, nicht eine ältere Bericht-SHA.
- Nutze ws-bridge/projmgrauth, prüfe Status, aktive Worker und Path-Claims; übernehme keine fremden Änderungen. Bei überlappendem Eigentum nur prüfen und einen konkreten Patchvorschlag hinterlassen; getrennte Worktrees/Claims und unabhängiges Review für Implementation.
- Lokal/host-only, keine Git-Netzwerkoperationen, Git-Push, Remote-CI, Installation auf dem SL101, SSH, laufende Desktop-/Sway-/Wayland-Session-Änderung, Reboot/Suspend, Deployment oder Veröffentlichung. ARMv7 VFPv3-D16 ohne NEON, wl_shm/Cairo-Pixman-Fallback und sichere Power-State-Gates bleiben zwingend.
- Bilde reproduzierbare positive und negative Tests, dokumentiere Befehl, Exit, relevante Logs, Commit/Datei-Hashes, PASS/FAIL/PENDING, und schreibe nach Meilensteinen einen agent_checkpoint mit der **exakten injizierten OCP-ID**. Keine legacy next.md und keine falsche Release- oder Hardware-Freigabe.
- **Semi-erweiternd, endlich:** Normalfall 0 Kinder. Nur für einen neu reproduzierten, deduplizierten P0/P1 außerhalb des Scopes höchstens **einen** Kindvorschlag im Checkpoint; kein Selbstdurchdisponieren. Der Conductor entscheidet innerhalb des globalen Budgets von **4** Zusatz-Leaves. Kein Enkel, kein Mindest-Fan-out. Bei bestandener Akzeptanz sofort terminal mit Belegen. Siehe POLICY.md dieses Ordners.
