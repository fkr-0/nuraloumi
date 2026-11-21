# Schließe Playerd- und Video-Bundle-Regressionen
## Ziel
Sichere Audio-only, EOS/Fehler und opt-in Video-Bundle-Verträge gegen die aktuellen Änderungen ab und behebe eng reproduzierte Defekte.
## Scope
Media/playerd-Tests, bestehende Adapter sowie zugehörige qualifikationsnahe Fixtures; player_engine.rs erst nach Task 02 und nur mit explizit freiem Claim. Kein SL101-Video-GPU-Erfolg aus Host-Checks ableiten; nicht an separatem sl101_linux-Checkout schreiben. Der Video-Consumer bleibt opt-in und fail-closed.
## Abnahme
- Host-Tests für play/pause/seek/stop, EOS/error latching, Audio-only und fehlende/inkonsistente Video-Bundle-Module; verifiziere unveränderte Backend-Gates.
- Prüfe exakte Manifest-/ELF-/ISA-Verträge soweit offline verfügbar. Fehlende Artefakte als blocker statt Dummy-PASS.
- Source-Fix nur bei minimalem aktuellen Repro, isolierte Regression und danach unabhängige Integrationsbestätigung.

## Verbindlicher Arbeitsvertrag
- Lies AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md und die relevanten aktuellen Qualifikationsbelege. Arbeitsbasis ist der tatsächlich angetroffene HEAD, nicht eine ältere Bericht-SHA.
- Nutze ws-bridge/projmgrauth, prüfe Status, aktive Worker und Path-Claims; übernehme keine fremden Änderungen. Bei überlappendem Eigentum nur prüfen und einen konkreten Patchvorschlag hinterlassen; getrennte Worktrees/Claims und unabhängiges Review für Implementation.
- Lokal/host-only, keine Git-Netzwerkoperationen, Git-Push, Remote-CI, Installation auf dem SL101, SSH, laufende Desktop-/Sway-/Wayland-Session-Änderung, Reboot/Suspend, Deployment oder Veröffentlichung. ARMv7 VFPv3-D16 ohne NEON, wl_shm/Cairo-Pixman-Fallback und sichere Power-State-Gates bleiben zwingend.
- Bilde reproduzierbare positive und negative Tests, dokumentiere Befehl, Exit, relevante Logs, Commit/Datei-Hashes, PASS/FAIL/PENDING, und schreibe nach Meilensteinen einen agent_checkpoint mit der **exakten injizierten OCP-ID**. Keine legacy next.md und keine falsche Release- oder Hardware-Freigabe.
- **Semi-erweiternd, endlich:** Normalfall 0 Kinder. Nur für einen neu reproduzierten, deduplizierten P0/P1 außerhalb des Scopes höchstens **einen** Kindvorschlag im Checkpoint; kein Selbstdurchdisponieren. Der Conductor entscheidet innerhalb des globalen Budgets von **4** Zusatz-Leaves. Kein Enkel, kein Mindest-Fan-out. Bei bestandener Akzeptanz sofort terminal mit Belegen. Siehe POLICY.md dieses Ordners.
