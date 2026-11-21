# NuraLoumi Fix Closure (2026-11-21)

Taskset: `NURALOUMI-FIX-CLOSURE-20261121-V1`. Das ist eine **endliche** Korrektur- und Integrationskampagne, kein neuer autonom rekursiver Prompt-Stammbaum.

| Lane | Prompt | Voraussetzung | Start |
|---|---|---|---|
| 01 | Backlog, Deduplikation, Release-Gate-Ledger | keine | queue |
| 02 | aktueller Rust-Source/CI-Fix | getrennte Ownership | queue |
| 03 | Browser-Test/Engine/Wayland-Gates | getrennte Ownership | queue |
| 04 | Playerd/Video-Bundle | 02 bei geteiltem player_engine.rs | hold |
| 05 | Shell/Sway Eingabe | Schutz der bereits dirty Shell-Pfade | hold |
| 06 | Cairo-/Font-Lifetime | Renderer-Claim | queue |
| 07 | ARM/Packaging/Version-Gates | eigene Script-/Packaging-Claims | queue |
| 08 | unabhängiges Integrationsgate | terminale 01–07 Belege | hold |

## Erfasster Ausgangszustand
- `main@fd34f43` (21.11.2026), 10 Commits vor `origin/main`; vorbestehende Modifikationen an menu.rs, sway_workspace_switcher.rs, zwei Qualifikationsdokumenten und einer Sway-Konfiguration; vorbestehende untracked Dokumente/Prompts. Unverändert zu schützen.
- `chatgpt-ops ps --project nuraloumi --limit 400 --json`: 339 open/ready = 334 queued + 5 draft, derzeit kein running in diesem Projekt. ID-Datum 20261119 bei 325, 20261120 bei 14. Kein Beleg, dass alle 339 zugestellt oder ausgeführt wurden.
- Stichproben: RECORD-BOUNDARY, STDIO-SYSCALL-CORRELATION und FOLLOWUP-INTEGRATION waren queued mit `delivery_confirmed=false` und `dispatch_failure_stage=pre-delivery` aufgrund einer früheren sicheren ABC-Routenverweigerung. Diese drei sind keine Implementierungsbelege.
- 70 lokale Promptdateien, 36 mit nachlaufender 'MANDATORY RECURSIVE DELEGATION' und 'AT LEAST THREE' Anweisung. Diese 36 Quelltexte erhalten die endliche neue Regel. Bereits registrierte immutable OCP-Promptrevisionen bleiben historisch unverändert.
- `cargo fmt --all -- --check` exit 0. Vollständige Tests und Hardware/Remote-CI nicht als aktuell PASS behauptet.
- Ältere Quellenberichte nennen Rust-Konfliktmarker, Browser-Testalias und 0.0.7-Hardpin. Alle Befunde müssen gegen die **aktuelle** SHA neu geprüft werden.
- Es erfolgt keine pauschale Terminalisierung der 339 offenen Tasks. Lane 01 erzeugt eine ID-genaue Ledger-Evidenz für spätere geprüfte Supersession.

Siehe `POLICY.md`. Nach Registrierung/Queueing tatsächliche OCP-Zustände verifizieren; 'queued' niemals als 'running' beschreiben.
