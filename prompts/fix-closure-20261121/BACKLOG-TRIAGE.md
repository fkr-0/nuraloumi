# Vorab-Triage der offenen NuraLoumi OCP-Einträge

Stand: 2026-11-21 (UTC). Autoritativ erfasst mittels `chatgpt-ops ps --project nuraloumi --limit 400 --json`.

| Kategorie | Anzahl | Konsequenz |
|---|---:|---|
| offene Einträge | 339 | ID-spezifische Zuordnung in Lane 01 nötig |
| davon queued | 334 | wartend, nicht delivered/running |
| davon draft | 5 | hold; nicht automatisch als Fehler klassifizieren |
| Kennung enthält 20261119 | 325 | zeitliche Häufung, **kein** Supersession-Beleg |
| Kennung enthält 20261120 | 14 | Zeitgruppe, nicht zwingend neuere Sourcebasis |
| bei `ps` tatsächlich running | 0 | keine aktiven NuraLoumi Workers zum Zeitpunkt der Aufnahme |

Stichprobe: `NURALOUMI-FOLLOWUP-RECORD-BOUNDARY-20261119-F51998`, `NURALOUMI-NURALOUMI-STDIO-FU-AUDIT-SYSCALL-CORRELATION-202-20261119-649D92`, `NURALOUMI-FOLLOWUP-INTEGRATION-20261119-79F2CC` waren `queued` mit `delivery_confirmed=false`, `dispatch_failure_stage=pre-delivery`. Dies gilt nicht automatisch für alle 339.

## Dry-run Konsolidierungsentscheidungen
- **KEEP / defer:** echte aktuelle UI/Browser/Media/ARM-Release-Fixes mit eigener Acceptance; unveränderte taskId plus neue Fix-Gate-Abdeckung. Identische alte Prompts werden nicht nochmal versandt.
- **MERGE CANDIDATE:** wiederholte glibc/ELF/stdio/loader/TLS/LMID-/mapping-Negativkontrollen. Evidenz bei Lane 01 bündeln; spezifische produktionsrelevante Bugrepros nachweisen, bevor sie eine Fix-Lane belegen.
- **SUPERSEDE PROPOSAL:** reine 'must spawn 3' Nachfolger, deren einziges Ergebnis erneute Child-Prompts wären. ID und Katalog-Prompttext vor terminaler Entscheidung prüfen.
- **UNKNOWN:** Versand-/Checkpointstatus ohne eindeutige Auslieferung. Kein erzwungener Retry, kein Abschluss durch Alter.
- **RELEASE HOLD:** keine official ARM 0.0.8 exact-SHA CI-/Artefaktkette und keine SL101-Pixel/Touch/Keyboard-Qualifikation; erst Gate 08 entscheidet mit geprüfter Evidenz.

Die 36 lokalen Source-Prompt-Tails sind nach der Korrektur keine Beweise dafür, dass vorhandene OCP-Katalogrevisionen bereits geändert wurden. Bestehende ausstehende Task-Identitäten bleiben im Ledger erhalten.
