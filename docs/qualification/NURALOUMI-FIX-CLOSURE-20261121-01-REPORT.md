# NuraLoumi Fix Closure 01 — OCP Ledger und Release-Gates

Stand: 2026-11-21 UTC. OCP `NURALOUMI-FIX-CLOSURE-20261121-01`. Untersuchter HEAD: `fd34f43e6dc27898047c1d95a2cc8748ff8128c7`.

## Reichweite und Nachweisgrad

- Native Abfrage: `chatgpt-ops ps --project nuraloumi --limit 400 --json` (Exit 0) meldete insgesamt 347 Zeilen, davon 8 Fix-Closure-Lanes 01–08 und 339 ältere Einträge. Gefilterte Auswertung: 334 `queued`, 5 `draft`. Die laufende Lane 01 ist **nicht** Teil des Legacy-Ledgers.
- `NURALOUMI-FIX-CLOSURE-20261121-01-LEDGER.csv`: 339 originale task IDs und der native State, Eigentümer, Beweisniveau sowie ID-spezifisch rückführbare nächste Prüfung. Der CSV-SHA256 ist `943a7d5a0c16e58a38f3ddd1fb8b2d1114b2221375b9bfe8773ae8c4bb98d3ff`.
- Vorläufige Klassen: 210 UNKNOWN, 128 MERGE-CANDIDATE, 1 BLOCKED, 0 KEEP, 0 SUPERSEDE-PROPOSAL. Die Klassen sind **Sichtung**, keine reproduzierten Defekte oder Abschlüsse. MERGE-CANDIDATE ist nur eine Themen-/Namenshypothese, kein bewiesenes Duplikat. Die Freigabe von SUPERSEDE bleibt folgerichtig leer. Da für die 339 Datensätze keine vollständige individuelle Promptbody-/Event-/Checkpoint-Prüfung gelang, ist die inhaltliche Abnahme des Ledgers **PENDING**.
- Quelle für Identität/Bucket der Altaufgaben: `prompts/fix-closure-20261121/LEGACY-339-DRY-RUN.md`, gegen den heutigen nativen Status und Zählungen abgeglichen. Drei früher beobachtete `delivery_confirmed=false`/pre-delivery-Verweigerungen sind eine **Stichprobe**, keine Verallgemeinerung. Für die übrigen Einträge ist tatsächliche Zustellung nicht belegt. Alte oder fehlende Checkpoints nicht als DONE interpretieren.
- `git status --short`: umfangreiche bestehende tracked/untracked Änderungen in Shell-/Browser-/Promptpfaden, unverändert gelassen. Aktive Claims bei Abfrage: keine. Keine Task-Transitions, Massen-Cleanups, neue OCP-Tasks oder Device-/Netzwerkhandlungen.

## Aktuelle lokale Checks

| Gate | Aufruf | Exit | Ergebnis und Grenze |
|---|---|---:|---|
| Git SHA | `git rev-parse HEAD` | 0 | fd34f43e6dc27898047c1d95a2cc8748ff8128c7 |
| Rust Format | `cargo fmt --all -- --check` | 0 | PASS auf gemeinsamem, dirty Worktree |
| Rust Tests | `cargo test --workspace --offline -q` | 0 | PASS, eine ignorierte Testgruppe; gemeinsamer Worktree |
| Rust Clippy | `cargo clippy --workspace --all-targets --offline -- -D warnings` | 0 | PASS, gemeinsamer Worktree |
| Browser Python | `python3 -m unittest discover -s components/nura-browser-engine/tests -p 'test_*.py' -q` | 0 | 76 Tests, 2 skips; gemeinsamer Worktree |
| Clean HEAD Host CI | separater `git archive HEAD`-Snapshot | — | PENDING: noch nicht auf aktuellem SHA isoliert ausgeführt |
| Native Browser/Sandbox/Nested Wayland | echter Pixel-/Input-/Lifecycle-Test | — | PENDING; Python Contracts reichen nicht |
| ARMv7/VFPv3-D16/no-NEON | offizielles exact-SHA CI-Artefakt und ELF/ABI-Laufzeitclosure | — | BLOCKED/PENDING |
| Version-Pin | `git show HEAD:Cargo.toml`, `git show HEAD:scripts/sl101_userspace_release.py` | 0 | Manifest **0.0.8-rc.1**, Installer jedoch **0.0.7-dev.1** und alter Commit pin: FAIL für 0.0.8-Release |
| SL101 qualifiziert | physische Pixel-/Touch-/Keyboard-/Power-/Rollback-Checks | — | PENDING, ausdrücklich nicht ausgeführt |

Die ältere R2-Rootcause-Notiz `docs/qualification/NURA-BROWSER-R2-CI-ROOTCAUSE.md` nennt einen früheren SHA und dortige parse-/Python-Fehler. Spätere Rust-/Python-Close-Berichte belegen auf anderen Snapshots konkrete Verbesserungen. Die heutigen PASS auf dem schmutzigen Arbeitsbaum beweisen nicht den Clean-HEAD-Erfolg und ersetzen nicht die derzeitige ARM- oder SL101-Freigabe. `docs/qualification/SL101-008-RC1-DEPLOYMENT-BLOCKER-20261119.md` dokumentiert den fehlenden official exact-SHA Nachweis und den alten Installer-Pin.

## Zwei priorisierte konkrete nächste Fix-Vorschläge (ohne Dispatch)

1. **P0 Release-Sicherheit, Lane 07:** Den 0.0.8-rc.1 vs. 0.0.7-dev.1 Pin-Konflikt auf der aktuellen SHA nach Eigentums- und Claim-Prüfung beheben. Negativtest: 0.0.8-Manifest mit altem Pin muss vor Staging fail-closed abbrechen; Positivtest: konsistenter exact-SHA-Manifest-/Version-Pin wird im trockenen Offline-Test akzeptiert. Keine Freigabe für Device-Deployment.
2. **P1 Host-CI-Beweislücke, Lane 02/03:** Clean `git archive fd34f43e` in isoliertem Host-Pfad gegen fmt, Rust-Tests, Clippy und 76 Browser-Python-Verträge ausführen; alte Source-Fehler nur bei neuer, aktueller Reproduktion erneut öffnen. Positive und absichtlich fehlerhafte private-Meson-/CI-Vertragsfixtures getrennt protokollieren.

## Entscheidungsgrenze

Lane 08 erst nach geprüfter Clean-HEAD- und Browser-/ARM-Befundlage, Besitzabgleich und unabhängiger Review. `KEEP` und `SUPERSEDE-PROPOSAL` werden nicht aus ID-Suchtreffern konstruiert. Insbesondere sind die 128 Merge-Kandidaten nicht terminalisierbar. Hardware- und Release-Freigabe: **NEIN**.
