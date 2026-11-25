# Qualify native WPE browser navigation, VNC pixels and SL101 touch

## Goal
Turn host-tested NuraBrowser UI/navigation into on-device evidence for real WPE pages, tab operations, HTTPS, actual Wayland/WayVNC pixels, physical touch, bounded memory/thermal soak and clean process exit/recovery.

## Current evidence
- Start from current NuraLoumi HEAD, `docs/qualification/NURABROWSER-LIVE-WAYVNC-20261125.md`, and the latest private WPE runtime and sandbox qualification reports.
- Host tests previously passed: navigation 8/8, browser IPC 13/13, Rust browser UI 8/8. These results do NOT establish native HTTPS, real tabs, physical touch or framebuffer pixels.
- The persistent local canary remains installed and usable, but the private D16 runtime and NetworkProcess sandbox are not yet release-qualified.

## Scope
1. With a supervised native canary session, capture pixels through the existing WayVNC path and prove initial page rendering, clicking `Open second page`, real page change, back/forward and address editing. Contrast visual observation with Wayland surface and WPE process logs.
2. Exercise actual multiple tabs, tab activation/close/focus, navigation history, HTTP and HTTPS success, invalid certificate, unreachable host and network error state. Attribute failures to WPE runtime, UI, IPC or sandbox with concrete logs.
3. Qualify keyboard/pointer and then physical touchscreen taps and gestures; distinguish VNC-only clicks from touch hardware proof. Collect bounded RSS, temperatures and render latency over a longer session with transparent thresholds.
4. Verify WPEWebProcess/WPENetworkProcess start and clean exit, no zombies/stale sockets, compositor persistence, and supervised crash recovery. Preserve a stable known-good canary and record rollback.

## Boundaries
- Use authenticated projmgrauth, AGENTS.md, workspace claims and the existing supervised device session; no recursive prompt dispatch.
- Do not promote to normal Browser, replace `/opt/nuraloumi/current`, reboot, poweroff, restart Sway/WayVNC, or switch the live canary without an explicitly qualified candidate. If runtime/sandbox gates fail, limit work to host fixtures and an explicit blocked checkpoint.
- No remote push or public publication.

## Acceptance and delivery
- Device-specific logs/screenshots and exact positive/negative navigation, tab and HTTPS cases demonstrate actual WPE behavior.
- Separate physical touch evidence from VNC click evidence; provide memory/thermal/soak and crash-recovery results with process IDs, exit codes and elapsed time.
- Distinguish pass, fail, blocked and not-tested for each gate; checkpoint exact tests and artifact references. Do not mark a canary production-ready from host tests alone.
