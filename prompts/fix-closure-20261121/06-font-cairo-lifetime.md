# Behebe belegte Cairo-Font-Lebensdauerfehler
## Ziel
Prüfe und repariere nur einen reproduzierbaren Renderer/Font-Lifetime-Bug, der Startup, Shutdown oder Sanitizer-Qualifikation blockiert; beende spekulative glibc/loader-Folgeprüfketten.
## Scope
crates/nuraloumi-render-cairo/** und neue host-only Tests, mit Claim. Prüfe Bericht zur Fontconfig/Cairo reset-static-data-Anomalie: ältere LSan 357 Bytes / 5 Allokationen, Reset am Prozessende PASS, ist noch kein bewiesener NuraLoumi-Produktbug. Kein globales Cairo-Reset in aktiven Prozessen, keine Gerätetests.
## Abnahme
- Isolierte drei-Prozess-Repro mit/ohne terminalem Reset und finalizer owner ordering; klare Library-vs-App Attribution, Exitstatus und Sanitizer-Logs.
- Bei NuraLoumi-Repro minimaler Fix + Regression; sonst PROVEN-NONBLOCKER oder konkrete Grenze mit Checkpoint, keine 2–8-Children-Diagnosekaskade.
- Narrow renderer tests und workspace clippy/fmt wenn praktisch.

## Verbindlicher Arbeitsvertrag
- Lies AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md und die relevanten aktuellen Qualifikationsbelege. Arbeitsbasis ist der tatsächlich angetroffene HEAD, nicht eine ältere Bericht-SHA.
- Nutze ws-bridge/projmgrauth, prüfe Status, aktive Worker und Path-Claims; übernehme keine fremden Änderungen. Bei überlappendem Eigentum nur prüfen und einen konkreten Patchvorschlag hinterlassen; getrennte Worktrees/Claims und unabhängiges Review für Implementation.
- Lokal/host-only, keine Git-Netzwerkoperationen, Git-Push, Remote-CI, Installation auf dem SL101, SSH, laufende Desktop-/Sway-/Wayland-Session-Änderung, Reboot/Suspend, Deployment oder Veröffentlichung. ARMv7 VFPv3-D16 ohne NEON, wl_shm/Cairo-Pixman-Fallback und sichere Power-State-Gates bleiben zwingend.
- Bilde reproduzierbare positive und negative Tests, dokumentiere Befehl, Exit, relevante Logs, Commit/Datei-Hashes, PASS/FAIL/PENDING, und schreibe nach Meilensteinen einen agent_checkpoint mit der **exakten injizierten OCP-ID**. Keine legacy next.md und keine falsche Release- oder Hardware-Freigabe.
- **Semi-erweiternd, endlich:** Normalfall 0 Kinder. Nur für einen neu reproduzierten, deduplizierten P0/P1 außerhalb des Scopes höchstens **einen** Kindvorschlag im Checkpoint; kein Selbstdurchdisponieren. Der Conductor entscheidet innerhalb des globalen Budgets von **4** Zusatz-Leaves. Kein Enkel, kein Mindest-Fan-out. Bei bestandener Akzeptanz sofort terminal mit Belegen. Siehe POLICY.md dieses Ordners.
