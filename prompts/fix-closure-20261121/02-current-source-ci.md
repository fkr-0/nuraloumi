# Behebe nachgewiesene Rust- und CI-Quellfehler
## Ziel
Bringe die aktuelle committed-source Rust/Host-Testmatrix auf reproduzierbar grün; fixiere nur aktuell nachgewiesene Fehler, nicht historische Meldungen von 387c5e6.
## Scope
Primär crates/nuraloumi-shell/src/player_engine.rs und crates/nuraloumi-shell/src/bin/notifications.rs sowie eng zugehörige Tests. Browser-engine/meson, ARM-Paketierung, Sway-Switcher und uncommitted fremde Pfade sind ausgeschlossen. Die ältere Qualifikation meldet conflict markers und let_and_return; prüfe, ob sie am aktuellen HEAD längst behoben sind. Falls grün, mit Evidence terminal schließen.
## Abnahme
- Frische isolierte committed-HEAD-Reproduktion; `cargo fmt --all -- --check`, `cargo test --workspace --locked`, `cargo clippy --workspace --all-targets --locked -- -D warnings` mit Exit/Logs (ressourcenschonend, begrenzte Zeit).
- Echten aktuellen ersten Fehler minimal beheben; mindestens eine Negativkontrolle; keine Clippy-Suppression oder CI-Allow-Failure.
- Diff und Review nur in Besitz-Scope; falls globale Checks durch fremde Dirty-Tree-Dateien versagen, klar trennen.

## Verbindlicher Arbeitsvertrag
- Lies AGENTS.md, ROADMAP.md, DESIGN.md, docs/ARCHITECTURE.md, docs/CONTRACTS.md, docs/AGENT-LANES.md und die relevanten aktuellen Qualifikationsbelege. Arbeitsbasis ist der tatsächlich angetroffene HEAD, nicht eine ältere Bericht-SHA.
- Nutze ws-bridge/projmgrauth, prüfe Status, aktive Worker und Path-Claims; übernehme keine fremden Änderungen. Bei überlappendem Eigentum nur prüfen und einen konkreten Patchvorschlag hinterlassen; getrennte Worktrees/Claims und unabhängiges Review für Implementation.
- Lokal/host-only, keine Git-Netzwerkoperationen, Git-Push, Remote-CI, Installation auf dem SL101, SSH, laufende Desktop-/Sway-/Wayland-Session-Änderung, Reboot/Suspend, Deployment oder Veröffentlichung. ARMv7 VFPv3-D16 ohne NEON, wl_shm/Cairo-Pixman-Fallback und sichere Power-State-Gates bleiben zwingend.
- Bilde reproduzierbare positive und negative Tests, dokumentiere Befehl, Exit, relevante Logs, Commit/Datei-Hashes, PASS/FAIL/PENDING, und schreibe nach Meilensteinen einen agent_checkpoint mit der **exakten injizierten OCP-ID**. Keine legacy next.md und keine falsche Release- oder Hardware-Freigabe.
- **Semi-erweiternd, endlich:** Normalfall 0 Kinder. Nur für einen neu reproduzierten, deduplizierten P0/P1 außerhalb des Scopes höchstens **einen** Kindvorschlag im Checkpoint; kein Selbstdurchdisponieren. Der Conductor entscheidet innerhalb des globalen Budgets von **4** Zusatz-Leaves. Kein Enkel, kein Mindest-Fan-out. Bei bestandener Akzeptanz sofort terminal mit Belegen. Siehe POLICY.md dieses Ordners.
