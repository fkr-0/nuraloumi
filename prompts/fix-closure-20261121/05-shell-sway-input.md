# Härte Sway-Switcher und Shell-Eingaben ohne Live-Mutation
## Ziel
Belege und behebe aktuellen Source-Level-Defekt in Super+Ziffern/Super-release, Screenshot-Previews, Touch/Keyboard und Wayland-Cancel.
## Scope
crates/nuraloumi-shell/src/bin/sway_workspace_switcher.rs, menu.rs, packaging/sway/nuraloumi-workspace-switcher.conf sowie genau passende Tests. Diese Dateien sind beim Start bereits modifiziert; bestehende uncommitted Änderungen sind fremd und dürfen nicht überschrieben oder unbemerkt übernommen werden. Ohne eindeutige Claim-/Worktree-Trennung bleibt diese Aufgabe read-only.
## Abnahme
- `cargo test --locked -p nuraloumi-shell --bin nuraloumi-sway-workspace-switcher` plus spezifische Fixture-/Property-Tests, Escape/Timeout, chord-vs-standalone Super, race bei Release vor Map, keine Doppelschaltung.
- Konfiguration nur dry-run/syntaktisch validieren; keine aktivierte Sway-Session. Super+andere Kombinationen und echtes Keyboard müssen bis zur Gerätefreigabe PENDING bleiben.
- Bei Konflikt exakten Repro/Patchplan im Checkpoint; terminal blocked statt erneuten Fan-out.

## Verbindlicher Arbeitsvertrag
- Lies AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md und die relevanten aktuellen Qualifikationsbelege. Arbeitsbasis ist der tatsächlich angetroffene HEAD, nicht eine ältere Bericht-SHA.
- Nutze ws-bridge/projmgrauth, prüfe Status, aktive Worker und Path-Claims; übernehme keine fremden Änderungen. Bei überlappendem Eigentum nur prüfen und einen konkreten Patchvorschlag hinterlassen; getrennte Worktrees/Claims und unabhängiges Review für Implementation.
- Lokal/host-only, keine Git-Netzwerkoperationen, Git-Push, Remote-CI, Installation auf dem SL101, SSH, laufende Desktop-/Sway-/Wayland-Session-Änderung, Reboot/Suspend, Deployment oder Veröffentlichung. ARMv7 VFPv3-D16 ohne NEON, wl_shm/Cairo-Pixman-Fallback und sichere Power-State-Gates bleiben zwingend.
- Bilde reproduzierbare positive und negative Tests, dokumentiere Befehl, Exit, relevante Logs, Commit/Datei-Hashes, PASS/FAIL/PENDING, und schreibe nach Meilensteinen einen agent_checkpoint mit der **exakten injizierten OCP-ID**. Keine legacy next.md und keine falsche Release- oder Hardware-Freigabe.
- **Semi-erweiternd, endlich:** Normalfall 0 Kinder. Nur für einen neu reproduzierten, deduplizierten P0/P1 außerhalb des Scopes höchstens **einen** Kindvorschlag im Checkpoint; kein Selbstdurchdisponieren. Der Conductor entscheidet innerhalb des globalen Budgets von **4** Zusatz-Leaves. Kein Enkel, kein Mindest-Fan-out. Bei bestandener Akzeptanz sofort terminal mit Belegen. Siehe POLICY.md dieses Ordners.
