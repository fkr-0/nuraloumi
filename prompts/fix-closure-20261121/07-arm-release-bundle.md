# Fixiere ARMv7-Artefakt- und lokale Release-Gates
## Ziel
Schließe nur lokal reparierbare Packaging-/ARMv7-Vertragsfehler und liefere eine wahrheitsgetreue 0.0.8-Gate-Matrix; keine Veröffentlichung und kein Geräteeingriff.
## Scope
scripts/sl101_userspace_release.py, scripts/verify-ci-artifact.py, ARM build/audit scripts, packaging/** und docs/qualification/** unter Claim. Keine Runtime-Crate-Änderungen. Älterer Blocker: EXPECTED_VERSION 0.0.7-dev.1, kein offizielles 0.0.8 exact-SHA-Artefakt; aktuelle Versionsquelle neu prüfen.
## Abnahme
- Offline Quell-/Paket-Fixtures für Version-Pin, SHA/Manifest/ELF-Verifikation und no-NEON VFPv3-D16, negative Cases (falsche SHA, falsche ABI, Teilbundle, missing deps).
- Release helper zunächst ausschließlich dry-run und ohne remote Git/CI/SL101 SSH. Fehlt exact-SHA-Artefakt: PENDING mit exakter Freigabeanforderung, kein Ersatz durch Host-Build.
- Isolation, minimaler Fix, Tests und independent-review-Handoff.

## Verbindlicher Arbeitsvertrag
- Lies AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md und die relevanten aktuellen Qualifikationsbelege. Arbeitsbasis ist der tatsächlich angetroffene HEAD, nicht eine ältere Bericht-SHA.
- Nutze ws-bridge/projmgrauth, prüfe Status, aktive Worker und Path-Claims; übernehme keine fremden Änderungen. Bei überlappendem Eigentum nur prüfen und einen konkreten Patchvorschlag hinterlassen; getrennte Worktrees/Claims und unabhängiges Review für Implementation.
- Lokal/host-only, keine Git-Netzwerkoperationen, Git-Push, Remote-CI, Installation auf dem SL101, SSH, laufende Desktop-/Sway-/Wayland-Session-Änderung, Reboot/Suspend, Deployment oder Veröffentlichung. ARMv7 VFPv3-D16 ohne NEON, wl_shm/Cairo-Pixman-Fallback und sichere Power-State-Gates bleiben zwingend.
- Bilde reproduzierbare positive und negative Tests, dokumentiere Befehl, Exit, relevante Logs, Commit/Datei-Hashes, PASS/FAIL/PENDING, und schreibe nach Meilensteinen einen agent_checkpoint mit der **exakten injizierten OCP-ID**. Keine legacy next.md und keine falsche Release- oder Hardware-Freigabe.
- **Semi-erweiternd, endlich:** Normalfall 0 Kinder. Nur für einen neu reproduzierten, deduplizierten P0/P1 außerhalb des Scopes höchstens **einen** Kindvorschlag im Checkpoint; kein Selbstdurchdisponieren. Der Conductor entscheidet innerhalb des globalen Budgets von **4** Zusatz-Leaves. Kein Enkel, kein Mindest-Fan-out. Bei bestandener Akzeptanz sofort terminal mit Belegen. Siehe POLICY.md dieses Ordners.
