@projmgrauth
Set repo: nuraloumi

# NuraLoumi Wave 1 — System Providers and Probe Binary

Canonical OCP task: NURALOUMI-PROVIDERS-R1-20260930
Lane: providers
Priority: 86

Operate under the exact OCP task and workflow packet. Confine writes to your lane.

## Mission

Build small, robust provider adapters that turn Linux system state into immutable snapshots and execute only explicit validated actions. They must degrade cleanly on machines lacking SL101 hardware or services and provide fixtures so shell development never blocks on hardware.

Read:
- AGENTS.md
- ROADMAP.md
- docs/ARCHITECTURE.md
- docs/CONTRACTS.md
- docs/SL101-QUALIFICATION.md

## Exclusive write scope

- crates/nuraloumi-providers/**
- tests/fixtures/providers/**

Do not edit core/shell/root files.

## Required implementation

1. Common provider layer
   - ProviderError with unavailable/permission/timeout/parse/io/backend categories.
   - Snapshot metadata: timestamp, health/stale/source.
   - bounded command runner abstraction with timeout, output-size limit and fixture replacement.
   - actions separate from reads.

2. Battery/power
   - sysfs power_supply discovery.
   - capacity/status/online/charging info.
   - tolerate missing fields and multiple batteries/adapters.

3. Backlight
   - discover /sys/class/backlight devices.
   - current/max brightness snapshot and percentage.
   - explicit set action with range validation.
   - do not mutate from snapshot calls.

4. Network
   - minimal NetworkManager strategy: prefer a bounded command adapter initially if DBus would materially inflate the dependency tree.
   - connected state, interface, SSID, signal if available.
   - radio/rescan/connect actions represented explicitly; safe argument passing without shell interpolation.

5. Audio
   - backend interface supporting a small command-based implementation plus fixtures.
   - volume/mute snapshot and explicit set/adjust/mute actions.
   - backend absence is a stable unavailable state.

6. Clock/session
   - local timestamp/time label provider.
   - suspend/reboot/poweroff represented as actions, but Wave 1 must default to dry-run/disabled unless explicitly enabled by caller capability.
   - never execute destructive actions during tests.

7. nuraloumi-probe binary
   - emit a stable JSON snapshot of all available providers.
   - --fixture DIR mode.
   - explicit action subcommand only when supplied; reads are mutation-free.
   - useful diagnostics without leaking sensitive network credentials.

## Dependency/resource discipline

Prefer std + serde + tiny parsing helpers. Avoid a full desktop service framework. External commands must use argv, not shell strings. Every wait/process call has a deadline.

## Acceptance

- cargo test -p nuraloumi-providers passes using temporary fake sysfs/fixtures and fake command executors.
- cargo clippy -p nuraloumi-providers --all-targets -- -D warnings passes.
- nuraloumi-probe --fixture produces deterministic JSON.
- missing battery/backlight/network/audio services are represented, not fatal.
- snapshot methods never mutate.
- destructive session actions are disabled/dry-run by default and untestable live side effects do not occur.
- command injection via SSID/device/value input is structurally impossible (argv-based execution).

## Handoff

Report actual backend commands/formats selected, fixture schema, probe examples, target assumptions and any capability that should remain disabled on first SL101 deployment. Submit typed phase result/checkpoint.
