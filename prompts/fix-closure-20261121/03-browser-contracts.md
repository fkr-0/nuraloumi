# Fixiere NuraBrowser-Engine-Verträge und lokale E2E-Gates
## Ziel
Schließe die am aktuellen HEAD reproduzierbaren NuraBrowser-Test-/ABI-/Sandbox- und compositor-nahen Defekte; akzeptiere keinen Screenshot-Fixture-PASS als echte Wayland-Qualifikation.
## Scope
components/nura-browser-engine/tests/**, testnahe Meson-Kontrakte und genau belegte Browser-engine Schnittstellen, nach Path-Claim. Keine player_engine.rs, keine Remote-CI, kein aktiver Desktop. Alte Report-SHA 387c5e6 hatte eine veraltete Meson-Alias-Assertion; prüfe gegen aktuellen Code. Halte unqualifizierte WPE Sandbox/Wayland/ARM als explizit PENDING.
## Abnahme
- `python3 -m unittest discover -s components/nura-browser-engine/tests -p 'test_*.py' -v` und relevante Host-Cargo-Browser-Tests mit positiven/negativen Fällen.
- Prüfe private nur-einmalige Engine-Installation, Auth/Unix-Socket-Grenze und fail-closed Browser-Chrome-Actions; kein Test-Skip als Fix.
- Wenn nested Wayland nicht sicher isoliert ausführbar ist: konkretes ausführbares E2E-Protokoll und BLOCKED/PENDING, kein Hardware-PASS.

## Verbindlicher Arbeitsvertrag
- Lies AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md und die relevanten aktuellen Qualifikationsbelege. Arbeitsbasis ist der tatsächlich angetroffene HEAD, nicht eine ältere Bericht-SHA.
- Nutze ws-bridge/projmgrauth, prüfe Status, aktive Worker und Path-Claims; übernehme keine fremden Änderungen. Bei überlappendem Eigentum nur prüfen und einen konkreten Patchvorschlag hinterlassen; getrennte Worktrees/Claims und unabhängiges Review für Implementation.
- Lokal/host-only, keine Git-Netzwerkoperationen, Git-Push, Remote-CI, Installation auf dem SL101, SSH, laufende Desktop-/Sway-/Wayland-Session-Änderung, Reboot/Suspend, Deployment oder Veröffentlichung. ARMv7 VFPv3-D16 ohne NEON, wl_shm/Cairo-Pixman-Fallback und sichere Power-State-Gates bleiben zwingend.
- Bilde reproduzierbare positive und negative Tests, dokumentiere Befehl, Exit, relevante Logs, Commit/Datei-Hashes, PASS/FAIL/PENDING, und schreibe nach Meilensteinen einen agent_checkpoint mit der **exakten injizierten OCP-ID**. Keine legacy next.md und keine falsche Release- oder Hardware-Freigabe.
- **Semi-erweiternd, endlich:** Normalfall 0 Kinder. Nur für einen neu reproduzierten, deduplizierten P0/P1 außerhalb des Scopes höchstens **einen** Kindvorschlag im Checkpoint; kein Selbstdurchdisponieren. Der Conductor entscheidet innerhalb des globalen Budgets von **4** Zusatz-Leaves. Kein Enkel, kein Mindest-Fan-out. Bei bestandener Akzeptanz sofort terminal mit Belegen. Siehe POLICY.md dieses Ordners.
