# NURALOUMI-20261117-QOL-R1-15-VERIFY-CONTRACT-GRAPH-20261117-A

@projmgrauth
Set repo: nuraloumi

Canonical parent task: NURALOUMI-20261117-QOL-R1-15-RELEASE-INTEGRATION-20261117-7FB90E
Local-only, review-only unless a scoped owner hands off. NO push, remote CI, install, power/suspend/reboot, device access, Grate/GStreamer private ABI changes. Preserve untracked and dirty work. Read AGENTS.md, docs/QOL-RELEASE-20261117.md, current OCP task states and all claims before any actions. ARMv7 VFPv3-D16/no NEON, Cairo+wl_shm fallback, >=48 logical px hit targets. Every finding must include code path, executable test/fixture, severity, before/after expectation. Do not duplicate active lanes 01-14; independent verification only. Record canonical checkpoint, receipts, follow-ups; no unsupported success claims.

## Scope
Independent contract graph qualification: core widgets/renderer hit regions/Wayland pointer-touch/semantic shell/provider actions. Specifically reproduce and isolate layout.rs:1065,1082 strict Clippy manual_div_ceil and check 48px hit target edge cases, scroll thumb input, slider bounds, tab keyboard parity, outside dismissal, mute acknowledgement after provider refresh. Confirm whether lanes 01,02,04,05 provide handoff. Report exact failing tests and seam patch suggestions; avoid source edits in active claims. Deliver docs/qualification/QOL-R1-15-CONTRACT-AUDIT.md only after acquiring a claim.

## Acceptance
cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings; deterministic scene and event fixtures; verify edge input at endpoints

## Endliche, fixorientierte Fortsetzung (ersetzt rekursive Delegationspflicht)
- Diese Aufgabe endet bei bestandener Akzeptanz oder einem belegten Blocker mit exaktem agent_checkpoint; standardmäßig **null** Folgetasks.
- Kein Mindestkontingent, keine Pflicht zu drei Kindern, kein Vererben einer Verzweigungsanweisung.
- Nur für einen neu reproduzierten, nicht bereits abgedeckten P0/P1-Befund außerhalb dieses Scopes darf **höchstens ein** enger Kindvorschlag im Checkpoint mit Repro, Dateibesitz, Negativkontrolle und Abnahme stehen. Nicht selbst dispatchen.
- Kohortenbudget NURALOUMI-FIX-CLOSURE-20261121-V1: maximal **vier** zusätzliche Leaves insgesamt; nur der Conductor prüft Katalog/OCP-Dubletten und lässt zu. Kinder erzeugen keine Enkel.
- Existierende laufende/queued Arbeit und fremde Änderungen nicht überschreiben; die ursprüngliche OCP-Identität und ihren Evidenzpfad bewahren. Ergebnisbeleg != Dispatchbeleg.
- Nach Fix: gezielte Tests und unabhängiges Integrationsreview; bestanden/irrelevant/blockiert sind terminale Zustände, kein Anlass zur erneuten Expansion.
